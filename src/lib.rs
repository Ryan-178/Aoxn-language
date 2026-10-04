//! Aoxn compiler pipeline: lex -> parse -> typecheck -> C-text codegen ->
//! native object (clang) -> link.

pub mod ast;
pub mod assets;
pub mod codegen_c;
pub mod files;
pub mod hashing;
pub mod lexer;
pub mod parser;
pub mod paths;
pub mod pkg_manifest;
pub mod platform;
pub mod symbols;
pub mod tailwind;
pub mod ts;
pub mod typecheck;

use crate::ast::{FnDecl, Program, StructDecl};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Diag {
    pub stage: &'static str, // "lex" | "parse" | "type" | "internal" | "link" | "io" | "asset" | "cc"
    pub file: u32,           // index into the compilation file registry
    pub line: usize,
    pub col: usize,
    pub message: String,
    /// errors stop the pipeline; warnings are reported and dropped (v0.42.0)
    pub severity: Severity,
    /// stable machine-readable tag (`W001`), for tools that must not match on
    /// message text; `None` for the unclassified majority
    pub code: Option<&'static str>,
}

impl Diag {
    /// constructor at a source position (B1 in docs/p2-compiler-performance.md:
    /// one place to build diagnostics, keeping error-site diffs small)
    pub fn at(stage: &'static str, file: u32, line: usize, col: usize, message: impl Into<String>) -> Diag {
        Diag { stage, file, line, col, message: message.into(), severity: Severity::Error, code: None }
    }

    /// a warning at a source position (never stops the pipeline)
    pub fn warn(stage: &'static str, file: u32, line: usize, col: usize, code: &'static str, message: impl Into<String>) -> Diag {
        Diag { stage, file, line, col, message: message.into(), severity: Severity::Warning, code: Some(code) }
    }

    pub(crate) fn internal(message: impl Into<String>) -> Diag {
        Diag {
            stage: "internal",
            file: u32::MAX,
            line: 0,
            col: 0,
            message: message.into(),
            severity: Severity::Error,
            code: None,
        }
    }

    /// the C compiler rejected the text we emitted (v0.42.0). This is NOT an
    /// internal error: the usual cause is a user-written `extern def` whose
    /// signature does not match the C function, which is why it gets its own
    /// stage instead of blaming the compiler.
    pub(crate) fn cc(message: impl Into<String>) -> Diag {
        Diag {
            stage: "cc",
            file: u32::MAX,
            line: 0,
            col: 0,
            message: message.into(),
            severity: Severity::Error,
            code: None,
        }
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    fn to_json(&self) -> String {
        fn esc(s: &str) -> String {
            let mut out = String::with_capacity(s.len() + 8);
            for c in s.chars() {
                match c {
                    '\\' => out.push_str("\\\\"),
                    '"' => out.push_str("\\\""),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                    c => out.push(c),
                }
            }
            out
        }
        let code = match self.code {
            Some(c) => format!("\"{c}\""),
            None => "null".to_string(),
        };
        format!(
            "{{\"stage\":\"{}\",\"severity\":\"{}\",\"code\":{},\"file\":\"{}\",\"line\":{},\"col\":{},\"message\":\"{}\"}}",
            self.stage,
            self.severity.as_str(),
            code,
            esc(&files::name(self.file)),
            self.line,
            self.col,
            esc(&self.message)
        )
    }
}

pub fn diags_to_json(diags: &[Diag]) -> String {
    let items: Vec<String> = diags.iter().map(|d| d.to_json()).collect();
    format!("{{\"ok\":false,\"errors\":[{}]}}", items.join(","))
}

/// The full machine-readable report: errors AND warnings, with `ok` derived
/// from the error list. This is what `--json` prints (v0.42.0).
pub fn report_to_json(errors: &[Diag], warnings: &[Diag]) -> String {
    let errs: Vec<String> = errors.iter().filter(|d| d.is_error()).map(|d| d.to_json()).collect();
    let warns: Vec<String> = warnings.iter().filter(|d| !d.is_error()).map(|d| d.to_json()).collect();
    format!(
        "{{\"ok\":{},\"errors\":[{}],\"warnings\":[{}]}}",
        errs.is_empty(),
        errs.join(","),
        warns.join(",")
    )
}

// ---- warning channel -------------------------------------------------------
//
// Warnings do not fail a compile, so they cannot ride the `Result` that every
// pipeline entry point returns without changing ~15 signatures. They are
// collected in a thread-local instead — the same shape `files.rs` uses for the
// file registry — and drained by whoever reports the result. The collector is
// cleared once per `typecheck::check`, so a compile can never see another
// compile's warnings.

thread_local! {
    static WARNINGS: std::cell::RefCell<Vec<Diag>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub fn push_warning(d: Diag) {
    debug_assert!(!d.is_error(), "push_warning takes warnings only");
    WARNINGS.with(|w| w.borrow_mut().push(d));
}

/// Drain the warnings collected since the last `clear_warnings`.
pub fn take_warnings() -> Vec<Diag> {
    WARNINGS.with(|w| std::mem::take(&mut *w.borrow_mut()))
}

pub fn clear_warnings() {
    WARNINGS.with(|w| w.borrow_mut().clear());
}


/// human-readable one-line form used by the CLI
pub fn diag_to_string(d: &Diag) -> String {
    let file = files::name(d.file);
    let loc = if d.line > 0 {
        format!("{file}:{}:{}: ", d.line, d.col)
    } else if file != "?" {
        format!("{file}: ")
    } else {
        String::new()
    };
    format!("[{}] {}{}", d.stage, loc, d.message)
}

/// AOXN_TIME=1 prints per-pipeline-stage wall-clock to stderr (lex / parse /
/// typecheck / codegen / link), in the style of AOXN_TC_TRACE / AOXN_CG_TRACE.
fn timed<T>(label: &str, f: impl FnOnce() -> T) -> T {
    if std::env::var("AOXN_TIME").is_ok() {
        let t = std::time::Instant::now();
        let out = f();
        eprintln!("[time] {label}: {:?}", t.elapsed());
        out
    } else {
        f()
    }
}

/// Full compile pipeline from Aoxn source text to a native object file.
pub fn compile_to_object(src: &str, obj_path: &Path, opt: bool) -> Result<(), Vec<Diag>> {
    compile_sources_to_object(&[src.to_string()], obj_path, opt)
}

/// Optimization-level form of [`compile_to_object`] (`0` = O0 .. `3` = O3).
pub fn compile_to_object_lvl(src: &str, obj_path: &Path, opt_level: u8) -> Result<(), Vec<Diag>> {
    compile_sources_to_object_lvl(&[src.to_string()], obj_path, opt_level)
}

/// legacy `bool` -> optimization level (`true` = O3, `false` = O0)
pub fn level_of(opt: bool) -> u8 {
    if opt {
        3
    } else {
        0
    }
}

/// Compile multiple Aoxn sources as one program (merged namespace).
pub fn compile_sources_to_object(sources: &[String], obj_path: &Path, opt: bool) -> Result<(), Vec<Diag>> {
    compile_sources_to_object_lvl(sources, obj_path, level_of(opt))
}

/// Optimization-level form of [`compile_sources_to_object`].
pub fn compile_sources_to_object_lvl(sources: &[String], obj_path: &Path, opt_level: u8) -> Result<(), Vec<Diag>> {
    let program = parse_sources(sources)?;
    finish_to_object(program, obj_path, opt_level)
}

/// Compile from file paths, resolving `import "..."` recursively
/// (include-once per canonical path, circular imports rejected).
pub fn compile_paths_to_object(paths: &[String], obj_path: &Path, opt: bool) -> Result<(), Vec<Diag>> {
    compile_paths_to_object_lvl(paths, obj_path, level_of(opt))
}

/// Optimization-level form of [`compile_paths_to_object`].
pub fn compile_paths_to_object_lvl(paths: &[String], obj_path: &Path, opt_level: u8) -> Result<(), Vec<Diag>> {
    let program = load_program(paths)?;
    finish_to_object(program, obj_path, opt_level)
}

/// Full compile pipeline from Aoxn source text to generated C text
/// (for `Aoxn c`). The optimization level does not change the text; it
/// selects the clang `-O` level used when the text is compiled.
pub fn compile_to_c(src: &str, opt: bool) -> Result<String, Vec<Diag>> {
    compile_sources_to_c(&[src.to_string()], opt)
}

/// Multiple sources → generated C text.
pub fn compile_sources_to_c(sources: &[String], opt: bool) -> Result<String, Vec<Diag>> {
    compile_sources_to_c_lvl(sources, level_of(opt))
}

/// Optimization-level form of [`compile_sources_to_c`].
pub fn compile_sources_to_c_lvl(sources: &[String], opt_level: u8) -> Result<String, Vec<Diag>> {
    let program = parse_sources(sources)?;
    finish_to_c(program, opt_level)
}

/// File paths (with imports) → generated C text.
pub fn compile_paths_to_c(paths: &[String], opt: bool) -> Result<String, Vec<Diag>> {
    compile_paths_to_c_lvl(paths, level_of(opt))
}

/// Optimization-level form of [`compile_paths_to_c`].
pub fn compile_paths_to_c_lvl(paths: &[String], opt_level: u8) -> Result<String, Vec<Diag>> {
    let program = load_program(paths)?;
    finish_to_c(program, opt_level)
}

fn finish_to_object(program: Program, obj_path: &Path, opt_level: u8) -> Result<(), Vec<Diag>> {
    let out = timed("typecheck", || typecheck::check(&program)).map_err(|d| vec![d])?;
    let mut program = program;
    // only concrete functions reach codegen: drop generic declarations,
    // append their monomorphized instances
    program.funcs.retain(|f| f.type_params.is_empty());
    program.funcs.extend(out.instances);
    timed("codegen", || {
        codegen_c::generate_to_object(&program, obj_path, opt_level, &out.call_map)
    })
    .map_err(|d| vec![d])?;
    Ok(())
}

fn finish_to_c(program: Program, _opt_level: u8) -> Result<String, Vec<Diag>> {
    let out = timed("typecheck", || typecheck::check(&program)).map_err(|d| vec![d])?;
    let mut program = program;
    program.funcs.retain(|f| f.type_params.is_empty());
    program.funcs.extend(out.instances);
    codegen_c::generate_c_text(&program, &out.call_map).map_err(|m| vec![Diag::internal(m)])
}

/// source-code based entry (no import resolution; imports are an error)
pub(crate) fn parse_sources(sources: &[String]) -> Result<Program, Vec<Diag>> {
    files::clear();
    let mut imports = Vec::new();
    let mut structs = Vec::new();
    let mut funcs = Vec::new();
    for src in sources {
        let file_id = files::register("<source>");
        let tokens = timed("lex", || lexer::lex(src, file_id)).map_err(|d| vec![d])?;
        let program = timed("parse", || parser::parse(tokens)).map_err(|d| vec![d])?;
        imports.extend(program.imports);
        structs.extend(program.structs);
        funcs.extend(program.funcs);
    }
    if let Some(imp) = imports.first() {
        return Err(vec![Diag::at(
            "io",
            imp.pos.file,
            imp.pos.line,
            imp.pos.col,
            format!(
                "import \"{}\" requires compiling from files (imports resolve relative to the importing file)",
                imp.path
            ),
        )]);
    }
    Ok(Program { imports: vec![], structs, funcs, assets: assets::AssetSet::default() })
}

/// Every top-level declaration reachable from the given entry files,
/// including those in their transitive imports.
///
/// The editor's language service is built on this: an outline, a
/// "go to definition" that crosses an `import`, and a workspace symbol
/// search all need the same thing the compiler already knows. Parse errors
/// come back as diagnostics exactly as they do for a build — a file that
/// does not parse has no reliable outline.
pub fn program_symbols(paths: &[String]) -> Result<Vec<symbols::Symbol>, Vec<Diag>> {
    let program = load_program(paths)?;
    Ok(symbols::collect(&program))
}

/// file-path based entry: resolve imports recursively
fn load_program(entries: &[String]) -> Result<Program, Vec<Diag>> {
    files::clear();
    let mut state = LoadState {
        visited: HashSet::new(),
        stack: Vec::new(),
        structs: Vec::new(),
        funcs: Vec::new(),
        assets: assets::AssetSet::default(),
        imports: assets::ImportState::default(),
    };
    for entry in entries {
        load_file(Path::new(entry), &mut state)?;
    }
    // `--tailwind` (env-carried, like --cpu/AOXN_CPU) scans everything the
    // program imports and prepends the generated utilities to the bundle, so
    // `styles()` carries them and `--emit-assets` writes them out with
    // everything else.
    if std::env::var("AOXN_TAILWIND").is_ok() {
        let mut sources: Vec<String> = Vec::new();
        for path in &state.visited {
            if let Ok(text) = std::fs::read_to_string(path) {
                if matches!(path.extension().and_then(|e| e.to_str()), Some("ts") | Some("tsx") | Some("ax")) {
                    sources.push(text);
                }
            }
        }
        let refs: Vec<&str> = sources.iter().map(|s| s.as_str()).collect();
        let generated = tailwind::generate(&refs);
        if !generated.unknown.is_empty() {
            let preview: Vec<&str> = generated.unknown.iter().take(8).map(|s| s.as_str()).collect();
            eprintln!(
                "[asset] warning: --tailwind covers a utility subset; {} class(es) not generated: {}{}",
                generated.unknown.len(),
                preview.join(", "),
                if generated.unknown.len() > preview.len() { ", ..." } else { "" }
            );
        }
        if !generated.css.is_empty() {
            state.assets.prepend_tw(&generated.css);
        }
    }
    // CSS accessors become ordinary functions before typecheck, so an unused
    // `import "./x.css"` costs nothing and codegen needs no asset awareness.
    let mut funcs = state.funcs;
    let mut structs = state.structs;
    assets::inject_all(&state.assets, Some(&mut structs), &mut funcs);
    Ok(Program { imports: vec![], structs, funcs, assets: state.assets })
}

/// The CSS assets `entries` pulls in, bundled and fingerprinted, without
/// compiling anything. `--emit-assets` needs the asset set to write files, and
/// the build itself does not hand it back; re-running the loader here is cheap
/// next to a clang invocation and keeps the emit step out of the compile path.
pub fn collect_assets(entries: &[String]) -> Result<assets::AssetSet, Vec<Diag>> {
    Ok(load_program(entries)?.assets)
}

struct LoadState {
    visited: HashSet<PathBuf>,
    stack: Vec<PathBuf>,
    structs: Vec<StructDecl>,
    funcs: Vec<FnDecl>,
    assets: assets::AssetSet,
    /// include-once + cycle stack for `@import` inside stylesheets
    imports: assets::ImportState,
}

fn load_file(path: &Path, state: &mut LoadState) -> Result<(), Vec<Diag>> {
    let canonical = std::fs::canonicalize(path)
        .map_err(|e| vec![Diag::at("io", u32::MAX, 0, 0, format!("cannot open '{}': {e}", path.display()))])?;
    if state.stack.contains(&canonical) {
        let cycle: Vec<String> = state
            .stack
            .iter()
            .chain(std::iter::once(&canonical))
            .map(|p| p.display().to_string())
            .collect();
        return Err(vec![Diag::at("io", u32::MAX, 0, 0, format!("circular import: {}", cycle.join(" -> ")))]);
    }
    if state.visited.contains(&canonical) {
        return Ok(()); // include-once
    }
    state.visited.insert(canonical.clone());
    state.stack.push(canonical.clone());

    let src = std::fs::read_to_string(path)
        .map_err(|e| vec![Diag::at("io", u32::MAX, 0, 0, format!("cannot read '{}': {e}", path.display()))])?;
    // Register the CANONICAL path, not the one we were called with: import
    // resolution canonicalizes, so a name registered from `path` would be
    // spelled `stdlib\ui.ax` when the entry was spelled as an absolute path
    // and `\\?\D:\...\stdlib\ui.ax` when it came through a canonicalize.
    // Diagnostics, asset records and exported symbols all resolve through
    // this registry, so ONE spelling here is what lets an editor match a
    // diagnostic about an imported file against the file it opened.
    let pretty_path = pretty(canonical.clone());
    let file_id = files::register(pretty_path.display().to_string());
    let label = pretty_path.display().to_string();
    // A `.css` file is an asset, not source. It must be diverted here, after
    // include-once and cycle detection but before the front-end dispatch:
    // otherwise the extension check below falls through to the Aoxn lexer and
    // a stylesheet reports `unexpected character '{'`, which says nothing
    // about the real problem (docs/css-assets.md).
    if assets::is_css(&canonical) {
        state.stack.pop();
        return timed(&format!("asset {label}"), || {
            assets::load(&canonical, file_id, &mut state.assets, &mut state.imports)
        })
        .map_err(|d| vec![d]);
    }
    // .ts/.tsx go through the TS-M1 front end, everything else the Aoxn one;
    // both lower into the same AST (docs/ts-m1-spec.md)
    let is_ts = path.extension().map(|e| e == "ts" || e == "tsx").unwrap_or(false);
    let program = if is_ts {
        timed(&format!("parse {label}"), || ts::parser::parse(file_id, &src)).map_err(|d| vec![d])?
    } else {
        let tokens = timed(&format!("lex {label}"), || lexer::lex(&src, file_id)).map_err(|d| vec![d])?;
        timed(&format!("parse {label}"), || parser::parse(tokens)).map_err(|d| vec![d])?
    };

    // resolve this file's imports relative to its own directory
    let dir = canonical.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    for imp in &program.imports {
        let imp_path = resolve_import(&dir, &imp.path);
        load_file(&imp_path, state).map_err(|diags| {
            // attach the import site to resolution errors that lack one
            let out: Vec<Diag> = diags
                .into_iter()
                .map(|mut d| {
                    if d.file == u32::MAX && d.line == 0 {
                        d.file = imp.pos.file;
                        d.line = imp.pos.line;
                        d.col = imp.pos.col;
                    }
                    d
                })
                .collect();
            out
        })?;
    }

    state.structs.extend(program.structs);
    state.funcs.extend(program.funcs);
    state.stack.pop();
    Ok(())
}

/// Drop the `\\?\` verbatim prefix Windows `canonicalize` adds.
///
/// Paths are spelled ONE way everywhere they are shown or compared — in
/// diagnostics, in `Aoxn symbols` output, in the IDE's explorer — so this
/// lives in one place rather than at each call site.
fn pretty(p: PathBuf) -> PathBuf {
    paths::strip_verbatim(p)
}

fn resolve_import(dir: &Path, import: &str) -> PathBuf {
    let p = Path::new(import);
    if p.is_absolute() {
        return complete_module_path(p.to_path_buf());
    }
    if import.starts_with("./") || import.starts_with("../") {
        return complete_module_path(dir.join(p));
    }
    // Bare identifiers are package imports (`import * from "http"`). Since
    // v0.29.1 the loader consults the package's `aox_modules/<name>/aoxn.json`
    // manifest: it resolves the entry via `main` / `exports` (incl. subpath
    // imports like `"http/client"`), so an installed package whose entry is
    // not literally `index.ax` is now importable. A package without a
    // manifest — or one whose manifest has no resolvable entry for this
    // subpath — falls back to the legacy directory probe below.
    let (pkg_name, subpath) = match import.split_once('/') {
        Some((name, rest)) => (name, Some(rest)),
        None => (import, None),
    };
    let pkg_dir = dir.join("aox_modules").join(pkg_name);
    if let Some(entry) = pkg_manifest::resolve_pkg_entry(&pkg_dir, subpath) {
        return entry;
    }
    // Legacy fallback: probe `aox_modules/<name>` (and the subpath, if any)
    // directly for `name.ax` / `index.ax`. Keeps pre-manifest packages working.
    let mut probe = pkg_dir;
    if let Some(s) = subpath {
        probe = probe.join(s);
    }
    let legacy = complete_module_path(probe);
    if legacy.is_file() {
        return legacy;
    }
    // v0.30.0: no local package of that name — try the *installed standard
    // library*, so `import * from "stdlib"` (or `"stdlib/ui"`) works from any
    // project directory against a one-click install, without a relative path
    // back into the toolchain. A specifier with a subpath drops its package
    // segment (`stdlib/ui` -> <stdlib>/ui.ax); a bare one keeps it
    // (`stdlib` -> <stdlib>/stdlib.ax). Local packages always win: they
    // matched above.
    if let Some(stdlib) = paths::stdlib_dir() {
        let mut base = stdlib;
        match subpath {
            Some(sub) => {
                for segment in sub.split('/') {
                    base = base.join(segment);
                }
            }
            None => base = base.join(pkg_name),
        }
        let found = complete_module_path(base);
        if found.is_file() {
            return found;
        }
    }
    // Nothing matched: return the package path anyway so the diagnostic names
    // the place the import was looked for.
    legacy
}

/// module-style specifier completion: try the exact path, then the source
/// extensions, then `index.<ext>` inside a directory
fn complete_module_path(base: PathBuf) -> PathBuf {
    // `is_file()` (not `exists()`): a directory at `base` must NOT short-
    // circuit the probe, or `index.<ext>` inside it would never be reached.
    // Pre-v0.29.1 this was latent because every call site passed paths with
    // an explicit extension; package directory imports now exercise it.
    if base.is_file() {
        return base;
    }
    for ext in ["ax", "ts", "tsx"] {
        let cand = base.with_extension(ext);
        if cand.is_file() {
            return cand;
        }
    }
    for ext in ["ax", "ts", "tsx"] {
        let cand = base.join(format!("index.{ext}"));
        if cand.is_file() {
            return cand;
        }
    }
    base
}

/// Every source file `load_program` would read for `entries`, imports
/// included (include-once, resolved relative to the importing file's
/// directory). Used by the `Aoxn run` build cache to key on the content of the
/// whole program; returns `None` when a file cannot be read, which disables
/// the cache for that invocation.
pub fn dependency_files(entries: &[String]) -> Option<Vec<PathBuf>> {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut out: Vec<PathBuf> = Vec::new();
    let mut stack: Vec<PathBuf> = entries.iter().map(PathBuf::from).collect();
    while let Some(path) = stack.pop() {
        let canonical = std::fs::canonicalize(&path).ok()?;
        if !seen.insert(canonical.clone()) {
            continue; // include-once, exactly like `load_file`
        }
        let src = std::fs::read_to_string(&canonical).ok()?;
        let dir = canonical.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        if assets::is_css(&canonical) {
            // A stylesheet is hashed like any other input, and so is
            // everything it pulls in: an `@import`ed partial and a `url(...)`
            // target both change the emitted artifact, so both must
            // invalidate the cache exactly as editing the entry file does.
            for dep in assets::scan_css_imports(&src)
                .into_iter()
                .chain(assets::scan_css_urls(&src))
            {
                let target = dir.join(&dep);
                if target.is_file() {
                    stack.push(target);
                }
            }
        } else {
            for imp in scan_imports(&src) {
                stack.push(resolve_import(&dir, &imp));
            }
        }
        out.push(canonical);
    }
    out.sort();
    Some(out)
}

/// Paths of every top-level import declaration in `src`, for every module
/// form (`import * from "p"`, `import { a } from "p"`, `import d from "p"`,
/// and the removed legacy `import "p"` which may linger in un-migrated
/// sources). The grammar only allows imports at top level, so a line-based
/// scan is exact for well-formed programs and may only *over*-include on
/// malformed input (safe for a cache key: more dependencies means fewer
/// cache hits, never a stale hit).
fn scan_imports(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in src.lines() {
        let rest = match line.trim_start().strip_prefix("import") {
            Some(r) => r,
            None => continue,
        };
        // word boundary: `imports` / `important` are not import declarations
        if rest.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
            continue;
        }
        let rest = rest.trim_start();
        if let Some(quoted) = rest.strip_prefix('"') {
            if let Some(end) = quoted.find('"') {
                out.push(quoted[..end].to_string());
            }
            continue;
        }
        // module form: the quoted path follows `from`
        if let Some(fi) = rest.find("from") {
            let after = rest[fi + 4..].trim_start();
            if let Some(quoted) = after.strip_prefix('"') {
                if let Some(end) = quoted.find('"') {
                    out.push(quoted[..end].to_string());
                }
            }
        }
    }
    out
}

/// Extra flags forwarded verbatim to every clang invocation (v0.42.0).
///
/// The CLI's `--clang-arg <flag>` (repeatable) and `-g` land here as one
/// newline-separated `AOXN_CLANG_ARGS`, following the `AOXN_CPU` /
/// `AOXN_TAILWIND` precedent: an env var travels through all ~15 pipeline
/// entry points without adding a parameter to each. Newline separation means a
/// single flag may contain spaces (`-Xclang -load`).
pub fn extra_clang_args() -> Vec<String> {
    match std::env::var("AOXN_CLANG_ARGS") {
        Ok(s) => parse_clang_args(&s),
        Err(_) => Vec::new(),
    }
}

/// Split the newline-separated flag list (`-Xclang -load` is ONE flag).
pub fn parse_clang_args(s: &str) -> Vec<String> {
    s.lines().filter(|l| !l.trim().is_empty()).map(|l| l.to_string()).collect()
}

/// Whether clang's own warnings should be shown (`--cc-warnings`, or
/// `AOXN_CC_WARNINGS=1`). Off by default: the emitted C is machine text.
pub fn cc_warnings_enabled() -> bool {
    match std::env::var("AOXN_CC_WARNINGS") {
        Ok(v) => v != "0" && !v.is_empty(),
        Err(_) => false,
    }
}

/// Locate the clang driver used for final linking.
/// Order: AOXN_CLANG env -> PATH -> the toolchain root's own `toolchain/bin`
/// (a portable LLVM dropped in by the installer) -> repo-local LLVM ->
/// standard install dir.
pub fn find_clang() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("AOXN_CLANG") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }
    for name in paths::clang_names() {
        if let Some(path) = which(name) {
            return Some(path);
        }
    }
    if let Some(bundled) = paths::bundled_clang() {
        return Some(bundled);
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    // compile-time repo-local toolchain layout: <repo>/LLVM/bin/clang.exe
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("LLVM").join("bin").join("clang.exe"));
    candidates.push(PathBuf::from(r"C:\Program Files\LLVM\bin\clang.exe"));
    candidates.into_iter().find(|p| p.is_file())
}

fn which(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var("PATH").ok()?;
    let sep = if cfg!(windows) { ';' } else { ':' };
    for dir in path_var.split(sep) {
        if dir.is_empty() {
            continue;
        }
        let candidate = PathBuf::from(dir).join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Compile Aoxn source files to an executable at `exe_path` (links via clang).
pub fn build_exe(src: &str, exe_path: &Path, opt: bool) -> Result<(), Vec<Diag>> {
    build_sources_exe(&[src.to_string()], exe_path, opt)
}

/// Optimization-level form of [`build_exe`].
pub fn build_exe_lvl(src: &str, exe_path: &Path, opt_level: u8) -> Result<(), Vec<Diag>> {
    build_sources_exe_lvl(&[src.to_string()], exe_path, opt_level)
}

/// Compile multiple sources (merged namespace) into an executable.
pub fn build_sources_exe(sources: &[String], exe_path: &Path, opt: bool) -> Result<(), Vec<Diag>> {
    build_sources_exe_lvl(sources, exe_path, level_of(opt))
}

/// Optimization-level form of [`build_sources_exe`].
pub fn build_sources_exe_lvl(sources: &[String], exe_path: &Path, opt_level: u8) -> Result<(), Vec<Diag>> {
    let obj_path = exe_path.with_extension(crate::platform::obj_ext());
    compile_sources_to_object_lvl(sources, &obj_path, opt_level)?;
    let link_result = link(&obj_path, exe_path);
    if link_result.is_ok() {
        let _ = std::fs::remove_file(&obj_path);
    }
    link_result
}

/// Compile from file paths (with import resolution) into an executable.
pub fn build_paths_exe(paths: &[String], exe_path: &Path, opt: bool) -> Result<(), Vec<Diag>> {
    build_paths_opts(paths, exe_path, opt, &[], &[])
}

/// like `build_paths_exe` with extra link libraries and search paths
pub fn build_paths_opts(
    paths: &[String],
    exe_path: &Path,
    opt: bool,
    libs: &[String],
    lib_paths: &[String],
) -> Result<(), Vec<Diag>> {
    build_paths_opts_lvl(paths, exe_path, level_of(opt), libs, lib_paths)
}

/// Optimization-level form of [`build_paths_opts`] (`0` = O0 .. `3` = O3).
pub fn build_paths_opts_lvl(
    paths: &[String],
    exe_path: &Path,
    opt_level: u8,
    libs: &[String],
    lib_paths: &[String],
) -> Result<(), Vec<Diag>> {
    let obj_path = exe_path.with_extension(crate::platform::obj_ext());
    compile_paths_to_object_lvl(paths, &obj_path, opt_level)?;
    let link_result = link_opts(&obj_path, exe_path, libs, lib_paths);
    if link_result.is_ok() {
        let _ = std::fs::remove_file(&obj_path);
    }
    link_result
}

/// Link a native object file into an executable using clang.
pub fn link(obj_path: &Path, exe_path: &Path) -> Result<(), Vec<Diag>> {
    link_opts(obj_path, exe_path, &[], &[])
}

/// Link with additional libraries and library search paths.
pub fn link_opts(obj_path: &Path, exe_path: &Path, libs: &[String], lib_paths: &[String]) -> Result<(), Vec<Diag>> {
    let clang = find_clang().ok_or_else(|| {
        vec![Diag::at(
            "link",
            u32::MAX,
            0,
            0,
            "cannot find clang for linking. Set AOXN_CLANG to the clang executable \
             or add LLVM's bin directory to PATH.",
        )]
    })?;

    let mut cmd = Command::new(&clang);
    cmd.arg(obj_path).arg("-o").arg(exe_path);
    // 8 MB stack: large fixed-size arrays live on the stack (allocas)
    if let Some(flag) = crate::platform::stack_link_flag() {
        cmd.arg(flag);
    }
    // `--clang-arg` flags reach the link step too (`-g` is a compile AND a
    // link flag; `-fsanitize=*` needs to be on both sides)
    for a in extra_clang_args() {
        cmd.arg(a);
    }
    for dir in lib_paths {
        cmd.arg(format!("-L{dir}"));
    }
    for lib in libs {
        cmd.arg(format!("-l{lib}"));
    }
    // C math (fmod/sqrt/...) is folded into the Windows CRT link, so there is
    // no `-lm` to add here.
    // Windows Defender / Smart App Control routinely hold a freshly written
    // .obj for a few hundred milliseconds, and the linker then fails with
    // "could not open ...obj" or "unable to remove file: permission
    // denied". That is a transient environmental race, not a link error, so
    // retry briefly before reporting. Real problems (undefined references,
    // bad flags) fail identically every time and surface right after the
    // short backoff. `output()` is used instead of `status()` only to read
    // the message and decide; stderr is re-emitted so the user still sees
    // "undefined reference to ..." exactly as before.
    const LINK_ATTEMPTS: u32 = 4;
    let mut attempt = 0u32;
    loop {
        let out = timed("link", || {
            cmd.output().map_err(|e| {
                vec![Diag::at("link", u32::MAX, 0, 0, format!("failed to spawn {}: {e}", clang.display()))]
            })
        })?;

        if out.status.success() {
            break;
        }
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        let transient = stderr.contains("could not open")
            || stderr.contains("permission denied")
            || stderr.contains("unable to remove file");
        attempt += 1;
        if !stderr.trim().is_empty() {
            eprintln!("{}", stderr.trim_end());
        }
        if !transient || attempt >= LINK_ATTEMPTS {
            return Err(vec![Diag::at(
                "link",
                u32::MAX,
                0,
                0,
                format!("clang linking failed with exit code {:?}", out.status.code()),
            )]);
        }
        std::thread::sleep(std::time::Duration::from_millis(250 * u64::from(attempt)));
    }
    Ok(())
}



