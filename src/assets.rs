//! CSS asset pipeline: `.css` as a first-class build input.
//!
//! A `.css` file reached through `import * from "./x.css"` (or `import "./x.css"`)
//! is not source code and never reaches the lexer. `load_file` diverts it here,
//! and the pipeline produces, for each asset, a bundle: `@import`s inlined in
//! order, comments and redundant whitespace removed, and — for `*.module.css`
//! — every local class name rewritten to a stable hashed name.
//!
//! The result is embedded into the program as ordinary functions whose bodies
//! are compile-time string literals (`styles()`, `styles_fingerprint()`,
//! `<stem>_class()`), assembled in [`inject`]. Going through real `FnDecl`s
//! rather than a codegen builtin is deliberate: `src/codegen_c.rs` stays
//! untouched, so the self-hosted emitter (`selfhost/codegen.ax`) needs no
//! mirror of this module and the bootstrap fixed point is unaffected.
//!
//! Everything here is hand-rolled std-only, per the zero-external-crate rule
//! in SECURITY.md. The minifier is deliberately conservative: it removes
//! comments and redundant whitespace, and nothing else. No selector merging, no
//! reordering, no empty-rule elision — the emitted text must depend only on the
//! source text, never on a structural transformation whose edge cases could
//! silently change meaning.

use crate::ast::{Block, Expr, FnDecl, Param, Pos, Stmt, StructDecl, Type};
use crate::hashing::FastBuild;
use std::collections::HashSet;
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};

/// Diagnostic stage for everything this module reports. Constructed directly in
/// `lib.rs`/`load_file` rather than routed through `codegen_c.rs`, whose
/// stringly-typed `Result<_, String>` would collapse every one of them into
/// `"internal"`.
pub const STAGE: &str = "asset";

fn err(file: u32, line: usize, col: usize, message: impl Into<String>) -> crate::Diag {
    crate::Diag::at(STAGE, file, line, col, message)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    /// a plain `.css` file: its text joins the single global bundle
    Plain,
    /// a `*.module.css` file: class names are scoped, and the text is *not*
    /// part of the global bundle (it is fetched by class name)
    Module,
}

#[derive(Debug, Clone)]
pub struct Asset {
    /// stable index into [`AssetSet::assets`]
    pub id: u32,
    pub path: PathBuf,
    pub kind: AssetKind,
    /// final CSS text: imports inlined, minified, module classes rewritten,
    /// `url(...)` targets rewritten to emitted names
    pub text: String,
    /// the name this asset is emitted under: `<stem>.<16 hex>.css`
    pub fingerprint: String,
    /// module only: `(original, hashed)` in first-appearance order
    pub classes: Vec<(String, String)>,
    /// module only: the identifier prefix for the generated accessor
    /// (`page.module.css` -> `page_module`)
    pub stem: String,
}

/// A non-CSS file a stylesheet points at (`url(...)`): a font, an image, an SVG.
/// Kept as bytes, because these are binary and must never be read as text.
#[derive(Debug, Clone)]
pub struct Referenced {
    /// the file on disk, as written in the stylesheet
    pub path: PathBuf,
    /// the name it is emitted under: `<stem>.<16 hex>.<ext>`
    pub emitted: String,
    /// file contents
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct AssetSet {
    pub assets: Vec<Asset>,
    /// `url(...)` targets, deduplicated, in first-reference order
    pub referenced: Vec<Referenced>,
}

impl AssetSet {
    pub fn is_empty(&self) -> bool {
        self.assets.is_empty() && self.referenced.is_empty()
    }

    /// The concatenated text of every plain (non-module) asset, in import
    /// order. This is what `styles()` returns.
    pub fn bundle(&self) -> String {
        let mut out = String::new();
        for a in &self.assets {
            if a.kind == AssetKind::Plain {
                out.push_str(&a.text);
            }
        }
        out
    }

    /// The bundle's emitted name: `<16 hex>.css`. This is exactly what
    /// `--emit-assets` writes and what a `<link href>` must name, so the two
    /// can never drift. (Each asset's own `fingerprint` keeps its stem, which
    /// is what makes an emitted directory browsable; the *bundle* is a
    /// separate artifact with its own name.)
    pub fn bundle_fingerprint(&self) -> String {
        let bundle = self.bundle();
        let h = FastBuild.build_hasher();
        let mut h = h;
        h.write(b"aoxn-css-bundle-v1");
        h.write(bundle.as_bytes());
        format!("{:016x}.css", h.finish())
    }

    /// Every file `--emit-assets` must write: each plain stylesheet (under its
    /// own fingerprinted name), the bundle itself, and every `url(...)`
    /// target. Module stylesheets are included too — their rules are scoped,
    /// but the file is still what a `<link>` should load.
    pub fn emitted_files(&self) -> Vec<(String, Vec<u8>)> {
        let mut out: Vec<(String, Vec<u8>)> = Vec::new();
        let push = |name: String, bytes: Vec<u8>, out: &mut Vec<(String, Vec<u8>)>| {
            if !out.iter().any(|(n, _)| *n == name) {
                out.push((name, bytes));
            }
        };
        let bundle = self.bundle();
        for a in &self.assets {
            push(a.fingerprint.clone(), a.text.clone().into_bytes(), &mut out);
        }
        // an empty bundle is not a file: a build that imports nothing must not
        // drop a stray `<hash>.css` into the asset directory
        if !bundle.is_empty() {
            push(self.bundle_fingerprint(), bundle.into_bytes(), &mut out);
        }
        for r in &self.referenced {
            push(r.emitted.clone(), r.bytes.clone(), &mut out);
        }
        out
    }

    /// Write every emitted artifact into `dir`, creating it if needed.
    ///
    /// Emitting beside the executable (rather than into the build cache) is
    /// deliberate: `prune_cache` walks a flat directory and only ever removes
    /// 16-hex-named cache entries, so anything written into the cache dir
    /// would leak forever.
    pub fn emit(&self, dir: &Path) -> Result<Vec<PathBuf>, crate::Diag> {
        std::fs::create_dir_all(dir).map_err(|e| {
            err(u32::MAX, 0, 0, format!("cannot create asset directory '{}': {e}", dir.display()))
        })?;
        let mut written = Vec::new();
        for (name, bytes) in self.emitted_files() {
            // A stylesheet controls the text it references, never a path:
            // names are compiler-generated (`<stem>.<hash>.<ext>`), and the
            // one guard below rejects anything that could escape the dir.
            if !is_safe_emitted_name(&name) {
                return Err(err(
                    u32::MAX,
                    0,
                    0,
                    format!("refusing to emit '{name}': not a plain file name"),
                ));
            }
            let path = dir.join(&name);
            std::fs::write(&path, &bytes).map_err(|e| {
                err(u32::MAX, 0, 0, format!("cannot write '{}': {e}", path.display()))
            })?;
            written.push(path);
        }
        Ok(written)
    }

    /// Insert generated utility CSS as a synthetic plain asset at the FRONT of
    /// the bundle. Order matters: the generated block must lose to a
    /// hand-written stylesheet, so it goes first and an authored `import` can
    /// override it — the same layering a real Tailwind build has.
    pub fn prepend_tw(&mut self, css: &str) {
        if css.is_empty() {
            return;
        }
        let text = minify(css);
        let name = format!("tailwind.{}", fingerprint_of(Path::new("tailwind"), &text));
        // shift the ids so `Asset::id` stays a dense 0..n index
        for (i, a) in self.assets.iter_mut().enumerate() {
            a.id = i as u32 + 1;
        }
        self.assets.insert(
            0,
            Asset {
                id: 0,
                path: PathBuf::from("<tailwind>"),
                kind: AssetKind::Plain,
                text,
                fingerprint: name,
                classes: Vec::new(),
                stem: "tailwind".to_string(),
            },
        );
    }
}

/// An emitted name is always `<stem>.<16 hex>[.ext]`. Anything else — a
/// separator, a `..`, an absolute path — is rejected rather than sanitized, so
/// a stylesheet can never talk the compiler into writing outside `dir`.
fn is_safe_emitted_name(name: &str) -> bool {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return false;
    }
    name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

/// Rewrite `url(...)` targets to the names `--emit-assets` will write, and
/// record each target so it is emitted too.
///
/// Left untouched, deliberately:
/// - `data:` URIs — the payload is inline already;
/// - absolute URLs (`http:`, `//cdn…`) — another origin owns them;
/// - fragment-only (`#icon`) — a reference into the same document;
/// - a target that does not resolve to a file — reported, because a broken
///   image is a bug the author wants to see, not something to paper over.
///
/// A `url()` inside a string or comment is not a `url()` token and is skipped
/// by the same scanner discipline used elsewhere in this module.
fn rewrite_urls(
    src: &str,
    dir: &Path,
    file: u32,
    assets: &mut AssetSet,
) -> Result<String, crate::Diag> {
    let b = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0usize;
    let mut depth_paren = 0usize;
    while i < b.len() {
        match b[i] {
            b'/' if i + 1 < b.len() && b[i + 1] == b'*' => {
                let end = src[i + 2..].find("*/").map(|p| i + 2 + p + 2).unwrap_or(b.len());
                out.push_str(&src[i..end]);
                i = end;
            }
            b'"' | b'\'' => {
                let end = skip_string(src, i);
                out.push_str(&src[i..end]);
                i = end;
            }
            b'(' => {
                depth_paren += 1;
                out.push('(');
                i += 1;
            }
            b')' => {
                depth_paren = depth_paren.saturating_sub(1);
                out.push(')');
                i += 1;
            }
            _ if starts_word_ci(src, i, "url") => {
                // `url` must be a bare function name: not `myurl(`, and the
                // `(` must follow with no whitespace for it to be a url token
                let after = i + 3;
                if after < b.len() && b[after] == b'(' {
                    let close = match src[after..].find(')') {
                        Some(p) => after + p,
                        None => {
                            out.push_str(&src[i..]);
                            break;
                        }
                    };
                    let inner = &src[after + 1..close];
                    out.push_str("url(");
                    out.push_str(&rewrite_one_url(inner, dir, file, assets)?);
                    out.push(')');
                    i = close + 1;
                    continue;
                }
                let ch = src[i..].chars().next().unwrap();
                out.push(ch);
                i += ch.len_utf8();
            }
            _ => {
                let ch = src[i..].chars().next().unwrap_or('\u{fffd}');
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    let _ = depth_paren;
    Ok(out)
}

/// Rewrite a single `url()` payload (the text between the parentheses),
/// returning its replacement.
///
/// A target that does not resolve to a file is left VERBATIM rather than
/// failing the build. That is the same call `@import url(...)` makes, and it
/// is the right one: a stylesheet legitimately references assets it does not
/// own — a CDN font, a file another tool copies in, an icon sprite shipped
/// beside the binary. Erroring would reject valid CSS over an asset the
/// compiler was never asked to manage, and silently rewriting it to a
/// fingerprint we cannot compute is not an option either. A warning goes to
/// stderr so the case is visible without being fatal.
fn rewrite_one_url(
    inner: &str,
    dir: &Path,
    file: u32,
    assets: &mut AssetSet,
) -> Result<String, crate::Diag> {
    let trimmed = inner.trim();
    // quoted form keeps its quotes in the output, unquoted is emitted unquoted
    let (quote, spec) = match trimmed.chars().next() {
        Some(q @ ('"' | '\'')) => {
            let body = &trimmed[1..trimmed.len().saturating_sub(1)];
            (Some(q), body)
        }
        _ => (None, trimmed),
    };
    let q = quote.map(|c| c.to_string()).unwrap_or_default();

    // not ours to rewrite
    if spec.is_empty()
        || spec.starts_with('#')
        || spec.starts_with("data:")
        || spec.starts_with("//")
        || spec.contains("://")
    {
        return Ok(format!("{q}{spec}{q}"));
    }

    let target = dir.join(spec);
    if !target.is_file() {
        eprintln!(
            "[asset] warning: url('{spec}') not found in '{}'; passing it through unchanged",
            dir.display()
        );
        return Ok(format!("{q}{spec}{q}"));
    }
    let bytes = std::fs::read(&target).map_err(|e| {
        err(file, 0, 0, format!("cannot read '{}': {e}", target.display()))
    })?;
    let name = referenced_name(&target, &bytes);

    // include-once: the same image referenced twice is emitted (and hashed)
    // once, and both url()s get the same name
    if !assets.referenced.iter().any(|r| r.emitted == name) {
        assets.referenced.push(Referenced {
            path: target,
            emitted: name.clone(),
            bytes,
        });
    }
    Ok(format!("{q}{name}{q}"))
}

/// `<stem>.<16 hex>.<ext>` for a `url()` target. The hash covers the bytes, so
/// a changed image gets a new name — which is the entire point of emitting
/// assets at all.
fn referenced_name(path: &Path, bytes: &[u8]) -> String {
    let mut h = FastBuild.build_hasher();
    h.write(b"aoxn-asset-v1");
    h.write(bytes);
    let digest = format!("{:016x}", h.finish());
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "asset".into());
    let ext = path.extension().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    if ext.is_empty() {
        format!("{stem}.{digest}")
    } else {
        format!("{stem}.{digest}.{ext}")
    }
}

/// Case-insensitive ASCII word match at `i`, with a word boundary after.
fn starts_word_ci(src: &str, i: usize, word: &str) -> bool {
    let rest = &src[i..];
    if rest.len() < word.len() || !rest[..word.len()].eq_ignore_ascii_case(word) {
        return false;
    }
    let before_ok = i == 0 || !is_ident_byte(src.as_bytes()[i - 1]);
    let after = rest.as_bytes().get(word.len()).copied();
    let after_ok = !matches!(after, Some(c) if is_ident_byte(c) || c == b'-');
    before_ok && after_ok
}

/// Is this path a CSS asset? `.css` and any `*.module.css`.
pub fn is_css(path: &Path) -> bool {
    path.extension().map(|e| e.eq_ignore_ascii_case("css")).unwrap_or(false)
}

/// The `.css` specifiers of every `@import "..."` in `src`, for the build
/// cache's dependency walk. Mirrors what `inline_imports` follows; a stylesheet
/// that `inline_imports` would pass through (a `url(...)` or media-query form)
/// contributes nothing here, which can only cost a cache hit, never cause a
/// stale one.
pub fn scan_css_imports(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = src;
    while let Some(idx) = find_at_import(rest) {
        let at = &rest[idx..];
        let consumed = match parse_import_spec(at) {
            Some((spec, n)) => {
                if is_css(Path::new(&spec)) {
                    out.push(spec);
                }
                n
            }
            None => skip_at_rule(at),
        };
        rest = &at[consumed.min(at.len())..];
    }
    out
}

/// The local file targets of every `url(...)` in `src`, for the build cache.
/// A `data:`/absolute/fragment reference is skipped for the same reason
/// `rewrite_urls` skips it: it names no local file.
pub fn scan_css_urls(src: &str) -> Vec<String> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        match b[i] {
            b'/' if i + 1 < b.len() && b[i + 1] == b'*' => {
                i = src[i + 2..].find("*/").map(|p| i + 2 + p + 2).unwrap_or(b.len());
            }
            b'"' | b'\'' => i = skip_string(src, i),
            _ if starts_word_ci(src, i, "url") => {
                let after = i + 3;
                if after < b.len() && b[after] == b'(' {
                    if let Some(p) = src[after..].find(')') {
                        let inner = src[after + 1..after + p].trim();
                        let spec = match inner.chars().next() {
                            Some(q @ ('"' | '\'')) => {
                                let _ = q;
                                inner[1..inner.len().saturating_sub(1)].to_string()
                            }
                            _ => inner.to_string(),
                        };
                        if !spec.is_empty()
                            && !spec.starts_with('#')
                            && !spec.starts_with("data:")
                            && !spec.starts_with("//")
                            && !spec.contains("://")
                        {
                            out.push(spec);
                        }
                        i = after + p + 1;
                        continue;
                    }
                }
                i += 3;
            }
            _ => {
                let ch = src[i..].chars().next().unwrap_or('\u{fffd}');
                i += ch.len_utf8();
            }
        }
    }
    out
}

/// `page.module.css` -> `page_module`; used as the generated accessor prefix
/// and disambiguated on collision.
fn module_stem(path: &Path) -> String {
    let raw = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "style".to_string());
    let trimmed = raw.strip_suffix(".module").unwrap_or(&raw).to_string();
    let mut out = String::with_capacity(trimmed.len());
    for c in trimmed.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() || out.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
        out.insert(0, '_');
    }
    out
}

/// Per-load bookkeeping for `@import` resolution: include-once plus a cycle
/// stack, mirroring `load_file`'s discipline.
#[derive(Default)]
pub struct ImportState {
    visited: HashSet<PathBuf>,
    stack: Vec<PathBuf>,
}

/// Load one `.css` asset: read it, inline its `@import`s, minify, and (for
/// modules) hash its class names. `file` is the registry index of `path`,
/// used so diagnostics name the offending stylesheet.
pub fn load(path: &Path, file: u32, assets: &mut AssetSet, imports: &mut ImportState) -> Result<(), crate::Diag> {
    let canonical = std::fs::canonicalize(path)
        .map_err(|e| err(file, 0, 0, format!("cannot open '{}': {e}", path.display())))?;
    // A stylesheet already pulled in by an earlier import contributes once;
    // its own text is inlined at that first position, which keeps cascade
    // order equal to a browser's first-seen order.
    if !imports.visited.insert(canonical.clone()) {
        return Ok(());
    }
    if imports.stack.contains(&canonical) {
        let mut chain: Vec<String> = imports
            .stack
            .iter()
            .chain(std::iter::once(&canonical))
            .map(|p| p.display().to_string())
            .collect();
        chain.dedup();
        return Err(err(file, 0, 0, format!("circular @import: {}", chain.join(" -> "))));
    }
    imports.stack.push(canonical.clone());

    let src = std::fs::read_to_string(&canonical)
        .map_err(|e| err(file, 0, 0, format!("cannot read '{}': {e}", canonical.display())))?;

    let dir = canonical.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let kind = if canonical
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase().ends_with(".module.css"))
        .unwrap_or(false)
    {
        AssetKind::Module
    } else {
        AssetKind::Plain
    };

// 1. inline @import, keeping the at-rule's position in the cascade
    let inlined = inline_imports(&src, &dir, file, imports)?;

    // 2. minify (text-preserving: comments only)
    let minified = minify(&inlined);

    // 3. rewrite url(...) to the names `--emit-assets` will write, and record
    //    the targets so they are emitted alongside the stylesheet
    let minified = rewrite_urls(&minified, &dir, file, assets)?;

    // 4. module class scoping
    let stem = module_stem(&canonical);
    let (text, classes) = match kind {
        AssetKind::Plain => (minified, Vec::new()),
        AssetKind::Module => {
            let seed = stable_seed(&canonical);
            scope_classes(&minified, seed)
        }
    };

    let fingerprint = fingerprint_of(&canonical, &text);
    imports.stack.pop();
    assets.assets.push(Asset {
        id: assets.assets.len() as u32,
        path: canonical,
        kind,
        text,
        fingerprint,
        classes,
        stem,
    });
    Ok(())
}

/// A seed derived from the stylesheet's own path, so the same class name in two
/// different modules hashes differently.
fn stable_seed(path: &Path) -> u64 {
    let mut h = FastBuild.build_hasher();
    h.write(b"aoxn-css-module-v1");
    h.write(path.to_string_lossy().to_lowercase().as_bytes());
    h.finish()
}

/// `<stem>.<16 hex>.css`, mirroring the shape `cache_key` uses so a build's
/// asset names and its cache key are visibly the same family.
fn fingerprint_of(path: &Path, text: &str) -> String {
    let mut h = FastBuild.build_hasher();
    h.write(b"aoxn-css-fingerprint-v1");
    h.write(text.as_bytes());
    let digest = format!("{:016x}", h.finish());
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "style".to_string());
    format!("{stem}.{digest}.css")
}

/// Replace every `@import "x.css";` with the (recursively processed) contents
/// of `x.css`. Non-CSS and unresolvable imports are left in place verbatim:
/// dropping them silently would change rendering, and failing the build on a
/// `@import url(...)` of a font would reject valid CSS.
fn inline_imports(
    src: &str,
    dir: &Path,
    file: u32,
    state: &mut ImportState,
) -> Result<String, crate::Diag> {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(idx) = find_at_import(rest) {
        let (line, col) = line_col(rest, idx);
        let (head, tail) = rest.split_at(idx);
        // head includes everything before the '@'
        out.push_str(head);
        let at = &tail[..];
        let Some((spec, consumed)) = parse_import_spec(at) else {
            // Not a form we inline (`@import url(...)`, `@import "a.css" screen`).
            // Copy the at-rule through untouched and continue after it.
            let end = skip_at_rule(at);
            out.push_str(&at[..end]);
            rest = &at[end..];
            continue;
        };
        if !is_css(Path::new(&spec)) {
            let end = skip_at_rule(at);
            out.push_str(&at[..end]);
            rest = &at[end..];
            continue;
        }
        let target = dir.join(&spec);
        if !target.is_file() {
            return Err(err(
                file,
                line,
                col,
                format!("@import '{spec}' does not resolve to a file in '{}'", dir.display()),
            ));
        }
        let inner = inline_file(&target, file, state)?;
        out.push_str(&inner);
        rest = &at[consumed..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Read + recursively inline one imported stylesheet.
fn inline_file(path: &Path, file: u32, state: &mut ImportState) -> Result<String, crate::Diag> {
    let canonical = std::fs::canonicalize(path)
        .map_err(|e| err(file, 0, 0, format!("cannot open '{}': {e}", path.display())))?;
    if state.stack.contains(&canonical) {
        return Err(err(
            file,
            0,
            0,
            format!("circular @import involving '{}'", canonical.display()),
        ));
    }
    let already = state.visited.contains(&canonical);
    let src = std::fs::read_to_string(&canonical)
        .map_err(|e| err(file, 0, 0, format!("cannot read '{}': {e}", canonical.display())))?;
    if already {
        // Included once already; a repeat import contributes no text, which
        // keeps the cascade identical to include-once module loading.
        return Ok(String::new());
    }
    state.visited.insert(canonical.clone());
    state.stack.push(canonical.clone());
    let dir = canonical.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let inner = inline_imports(&src, &dir, file, state)?;
    state.stack.pop();
    Ok(inner)
}

/// Find the next `@import` that is not inside a comment or a string.
fn find_at_import(src: &str) -> Option<usize> {
    let b = src.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'/' if i + 1 < b.len() && b[i + 1] == b'*' => {
                i = src[i..].find("*/").map(|p| i + p + 2).unwrap_or(b.len());
            }
            b'"' | b'\'' => {
                i = skip_string(src, i);
            }
            b'@' => {
                let after = &src[i + 1..];
                let trimmed = after.trim_start();
                if trimmed.len() < 6 {
                    return None;
                }
                let lead = after.len() - trimmed.len();
                if trimmed[..6].eq_ignore_ascii_case("import")
                    && !trimmed[6..].starts_with(|c: char| c.is_alphanumeric() || c == '-')
                {
                    return Some(i);
                }
                i += 1 + lead + 6;
            }
            _ => i += 1,
        }
    }
    None
}

/// If the at-rule at `at` is `@import "x.css"` (optionally with a media query
/// tail), return the specifier and how many bytes to skip. `url(...)` and
/// single-quoted forms return `None` and are passed through untouched.
fn parse_import_spec(at: &str) -> Option<(String, usize)> {
    let rest = at.trim_start_matches('@');
    let rest = rest.trim_start();
    if rest.len() < 6 || !rest[..6].eq_ignore_ascii_case("import") {
        return None;
    }
    let after = rest[6..].trim_start();
    let lead = rest.len() - 6 - after.len();
    let quote = after.chars().next()?;
    if quote != '"' {
        return None; // `url(...)` or `'...'` — passed through
    }
    let body = &after[1..];
    let end = body.find('"')?;
    let spec = body[..end].to_string();
    let consumed = 1 /* @ */ + lead + 6 + (after.len() - body.len()) + 1 /* quote */ + end + 1;
    // A media-query tail is legal CSS and has no asset to inline; treat the
    // whole at-rule as passed-through rather than guessing.
    let after_spec = &body[end + 1..];
    if !after_spec.trim_start().starts_with(';') {
        return None;
    }
    Some((spec, consumed.min(at.len())))
}

/// Length of the at-rule starting at `at` (up to and including `;`, or the
/// matching `{}` block).
fn skip_at_rule(at: &str) -> usize {
    let b = at.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' | b'\'' => i = skip_string(at, i),
            b'{' => {
                let mut depth = 0usize;
                while i < b.len() {
                    match b[i] {
                        b'{' => depth += 1,
                        b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                return i + 1;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
                return b.len();
            }
            b';' => return i + 1,
            _ => i += 1,
        }
    }
    b.len()
}

/// Index just past the string literal starting at `i` (which is the quote).
fn skip_string(s: &str, i: usize) -> usize {
    let b = s.as_bytes();
    let quote = b[i];
    let mut j = i + 1;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 2,
            c if c == quote => return j + 1,
            _ => j += 1,
        }
    }
    b.len()
}

fn line_col(src: &str, idx: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut last_nl = 0usize;
    for (i, c) in src.char_indices() {
        if i >= idx {
            break;
        }
        if c == '\n' {
            line += 1;
            last_nl = i + 1;
        }
    }
    (line, src[last_nl..idx].chars().count() + 1)
}

/// Conservative minifier: drop comments, collapse runs of whitespace to a
/// single space, and remove spaces that sit next to `{`, `}`, `;` and `,`.
///
/// Deliberately *not* done: selector merging, rule reordering, dropping the
/// last `;` in a block, dropping empty rules, shortening colors. Each is a
/// transformation that can change meaning in some CSS corner; the win is
/// cosmetic, and the emitted text stays a pure function of the source.
pub fn minify(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0usize;
    // pending whitespace: emitted lazily, so trailing space never appears and
    // a space next to a dropped character is never written
    let mut pending_ws = false;
    while i < b.len() {
        match b[i] {
            b'/' if i + 1 < b.len() && b[i + 1] == b'*' => {
                let end = src[i + 2..].find("*/").map(|p| i + 2 + p + 2).unwrap_or(b.len());
                // a comment acts as whitespace: `a/**/b` must not become `ab`
                pending_ws = true;
                i = end;
            }
            b'"' | b'\'' => {
                let end = skip_string(src, i);
                if pending_ws && needs_space(&out, b[end - 1]) {
                    out.push(' ');
                }
                pending_ws = false;
                out.push_str(&src[i..end]);
                i = end;
            }
            c if c.is_ascii_whitespace() => {
                pending_ws = true;
                i += 1;
            }
            c => {
                if pending_ws && needs_space(&out, c) {
                    out.push(' ');
                }
                pending_ws = false;
                out.push(c as char);
                i += 1;
            }
        }
    }
    out
}

/// Whether a space is required between the accumulated output and the next
/// character. Whitespace is insignificant next to these, and required
/// everywhere else (descendant combinators, `0 8px`, keyword sequences).
fn needs_space(out: &str, next: u8) -> bool {
    if out.is_empty() {
        return false;
    }
    if matches!(next, b'{' | b'}' | b';' | b',' | b')' | b':') {
        return false;
    }
    // `.a{color:red}` — a space before `{` is never needed, but a space after
    // it is, unless the next char can start a value directly.
    let last = out.as_bytes()[out.len() - 1];
    if matches!(last, b'{' | b'}' | b';' | b',' | b'(') {
        return false;
    }
    if last == b':' {
        return false;
    }
    true
}

/// Rewrite `.class` selectors to `.class_<hash>` throughout a module, and
/// return the discovered `(original, hashed)` pairs in first-appearance order.
///
/// The scan is context-aware, which is the whole difficulty of CSS Modules: a
/// `.` inside a string, inside an `@media` prelude, or inside a declaration
/// value is *not* a class selector and must survive untouched. Only a `.`
/// appearing in selector position — outside strings, outside comments, outside
/// parentheses, and before a block's `{` — is rewritten.
pub fn scope_classes(src: &str, seed: u64) -> (String, Vec<(String, String)>) {
    let b = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut classes: Vec<(String, String)> = Vec::new();
    let mut i = 0usize;
    // brace depth: at depth 0 we are between rules (at-rule preludes live
    // here); a `.` there is not a class selector either.
    let mut depth = 0usize;
    // paren depth: `@media (min-width: 30px)`, `url(...)`, `:not(...)`
    let mut paren = 0usize;
    // inside a declaration value, i.e. after the first `:` of a block, a `.`
    // is a decimal point or part of a value, never a selector
    let mut in_value = false;
    while i < b.len() {
        match b[i] {
            b'/' if i + 1 < b.len() && b[i + 1] == b'*' => {
                let end = src[i + 2..].find("*/").map(|p| i + 2 + p + 2).unwrap_or(b.len());
                out.push_str(&src[i..end]);
                i = end;
            }
            b'"' | b'\'' => {
                let end = skip_string(src, i);
                out.push_str(&src[i..end]);
                i = end;
            }
            b'(' => {
                paren += 1;
                out.push('(');
                i += 1;
            }
            b')' => {
                paren = paren.saturating_sub(1);
                out.push(')');
                i += 1;
            }
            b'{' => {
                depth += 1;
                in_value = false;
                out.push('{');
                i += 1;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                in_value = false;
                out.push('}');
                i += 1;
            }
            b';' => {
                in_value = false;
                out.push(';');
                i += 1;
            }
            b':' => {
                // only a declaration separator when inside a block and not in
                // a paren (pseudo-selectors like `:hover` sit at depth 0 or
                // inside parens)
                if depth > 0 && paren == 0 {
                    in_value = true;
                }
                out.push(':');
                i += 1;
            }
            b'.' if depth > 0 || paren > 0 || in_value || !is_ident_start(b.get(i + 1).copied()) => {
                // not selector position, or not a class name at all (a decimal
                // like `.5` in a value)
                out.push('.');
                i += 1;
            }
            b'.' => {
                let start = i + 1;
                let mut end = start;
                while end < b.len() && is_ident_byte(b[end]) {
                    end += 1;
                }
                let name = src[start..end].to_string();
                let scoped = scope_name(&name, seed);
                if !classes.iter().any(|(o, _)| *o == name) {
                    classes.push((name, scoped.clone()));
                }
                out.push('.');
                out.push_str(&scoped);
                i = end;
            }
            _ => {
                // copy one whole UTF-8 char
                let ch = src[i..].chars().next().unwrap_or('\u{fffd}');
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    (out, classes)
}

/// `title` -> `title_a1b2c3`. The suffix is derived from the module seed and
/// the class name, so it is stable across builds and distinct per module.
fn scope_name(name: &str, seed: u64) -> String {
    let mut h = FastBuild.build_hasher();
    h.write_u64(seed);
    h.write(name.as_bytes());
    let digest = h.finish();
    format!("{name}_{:08x}", (digest & 0xffff_ffff) as u32)
}

fn is_ident_start(c: Option<u8>) -> bool {
    matches!(c, Some(c) if c.is_ascii_alphabetic() || c == b'_' || c >= 0x80)
}

fn is_ident_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c >= 0x80
}

/// Assemble the asset accessors as ordinary functions and append them to
/// `funcs`. Each body is a compile-time string literal, so `styles()` costs
/// nothing at run time beyond returning a pointer.
///
/// Generated:
/// - `styles() -> string` — the whole plain-CSS bundle
/// - `styles_fingerprint() -> string` — `<bundlehash>.css`, for `<link>` busting
/// - `<stem>_class(name: string) -> string` — one per `*.module.css`
pub fn inject(assets: &AssetSet, funcs: &mut Vec<FnDecl>) {
    inject_all(assets, None, funcs);
}

/// As [`inject`], and additionally synthesize the per-module struct types that
/// back `styles.title` dot access. The loader passes the struct list so a TS
/// module's `import styles from "./page.module.css"` can be rewritten to read
/// a real typed value; the Aoxn front end has no dot-access sugar and does not
/// need it.
pub fn inject_all(assets: &AssetSet, mut structs: Option<&mut Vec<StructDecl>>, funcs: &mut Vec<FnDecl>) {
    if assets.is_empty() {
        return;
    }
    let pos = Pos { line: 0, col: 0, file: u32::MAX };

    funcs.push(const_fn("styles", assets.bundle(), pos));

    // `bundle_fingerprint()` already ends in `.css` — it is the literal name
    // `emit()` writes, so the `<link href>` and the file on disk cannot drift.
    funcs.push(const_fn("styles_fingerprint", assets.bundle_fingerprint(), pos));

    for (accessor, module) in module_accessors(assets) {
        funcs.push(class_lookup_fn(&accessor, &module.classes, pos));
        if let Some(structs) = structs.as_deref_mut() {
            // `<stem>_module()` returns a value struct with one field per
            // class, which is what makes `styles.title` a typed string read
            // rather than a dynamic lookup. The struct type is named
            // `<stem>_Classes` — distinct from the accessor, because a
            // function and a struct may not share a name in one namespace.
            let type_name = format!("{}_Classes", module.stem);
            let fields: Vec<(String, String)> = module
                .classes
                .iter()
                .map(|(orig, scoped)| (orig.clone(), scoped.clone()))
                .collect();
            structs.push(StructDecl {
                name: type_name.clone(),
                fields: fields
                    .iter()
                    .map(|(name, _)| Param { name: name.clone(), ty: Type::Str, pos })
                    .collect(),
                pos,
            });
            let lit_fields = fields
                .into_iter()
                .map(|(name, scoped)| (name, Expr::Str(scoped, pos)))
                .collect();
            funcs.push(FnDecl {
                name: format!("{}_module", module.stem),
                type_params: Vec::new(),
                len_param: None,
                params: Vec::new(),
                ret: Type::Struct(type_name.clone()),
                body: Block {
                    stmts: vec![Stmt::Return {
                        expr: Some(Expr::StructLit { name: type_name, fields: lit_fields, lit_id: 0, pos }),
                        pos,
                    }],
                },
                is_extern: false,
                pos,
            });
        }
    }
}

/// The accessor name and accessor payload for every `*.module.css`, in a
/// stable order, with stem collisions disambiguated. Exposed so the loader can
/// bind `import styles from "./page.module.css"` to the same name the
/// compiler generated (see `bind_module_names`).
pub fn module_accessors(assets: &AssetSet) -> Vec<(String, &Asset)> {
    let mut used: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for a in &assets.assets {
        if a.kind != AssetKind::Module || a.classes.is_empty() {
            continue;
        }
        let mut name = format!("{}_class", a.stem);
        let mut n = 2;
        while !used.insert(name.clone()) {
            name = format!("{}_class_{n}", a.stem);
            n += 1;
        }
        out.push((name, a));
    }
    out
}

/// `def NAME() -> string: return "<text>"`
fn const_fn(name: &str, text: String, pos: Pos) -> FnDecl {
    FnDecl {
        name: name.to_string(),
        type_params: Vec::new(),
        len_param: None,
        params: Vec::new(),
        ret: Type::Str,
        body: Block { stmts: vec![Stmt::Return { expr: Some(Expr::Str(text, pos)), pos }] },
        is_extern: false,
        pos,
    }
}

/// `def NAME(name: string) -> string:` walking an if-chain over the class map.
/// A linear chain rather than a data structure: the map is small, and this
/// keeps the emitter free of any runtime container.
fn class_lookup_fn(name: &str, classes: &[(String, String)], pos: Pos) -> FnDecl {
    let param = "name".to_string();
    let mut stmts = Vec::with_capacity(classes.len() * 2);
    for (orig, scoped) in classes {
        stmts.push(Stmt::If {
            cond: Expr::Binary {
                op: crate::ast::BinOp::Eq,
                lhs: Box::new(Expr::Var { name: param.clone(), pos }),
                rhs: Box::new(Expr::Str(orig.clone(), pos)),
                pos,
            },
            then_block: Block { stmts: vec![Stmt::Return { expr: Some(Expr::Str(scoped.clone(), pos)), pos }] },
            else_block: None,
            pos,
        });
    }
    stmts.push(Stmt::Return { expr: Some(Expr::Str(String::new(), pos)), pos });
    FnDecl {
        name: name.to_string(),
        type_params: Vec::new(),
        len_param: None,
        params: vec![Param { name: param, ty: Type::Str, pos }],
        ret: Type::Str,
        body: Block { stmts },
        is_extern: false,
        pos,
    }
}