//! Aoxn compiler driver.
//!
//! Usage:
//!   Aoxn build <file.ax> [-o out.exe] [--O0|--O1|--O2|--O3] [--json]
//!   Aoxn run   <file.ax> [args...] [--O0|--O1] [--json]
//!   Aoxn c     <file.ax> [--json]
//!   Aoxn check <file.ax> [--json]
//!   Aoxn symbols <file.ax> [--json]
//!   Aoxn --help
//!
//! Optimizer selection (default O3, the documented "parity with clang -O3"):
//! the optimization level selects the clang `-O` level used to compile the
//! generated C (the C text itself does not depend on it).
//! `AOXN_PASSES` no longer has any effect (the LLVM pass pipeline is gone).

use std::path::{Path, PathBuf};
use std::process::Command;

use aoxn::Diag;

mod doctor;

/// default optimization level (the documented O3 promise)
const DEFAULT_OPT_LEVEL: u8 = 3;

struct Opts {
    out: Option<String>,
    /// write fingerprinted CSS/asset files here (v0.36.0). Relative paths
    /// resolve against the *output executable's* directory, so
    /// `aoxn build app.ax --emit-assets assets` means "next to app.exe"
    emit_assets: Option<String>,
    /// generate Tailwind-compatible utilities by scanning the sources, and
    /// prepend them to the global bundle (v0.36.0)
    tailwind: bool,
    /// clang optimization level passed to the C compile: 0 = O0 .. 3 = O3
    opt_level: u8,
    json: bool,
    cpu: Option<String>,
    positional: Vec<String>,
    // everything after "--" (used by `run` to pass args to the compiled program)
    passthrough: Vec<String>,
    /// additional libraries to link (`-l user32`), repeatable
    libs: Vec<String>,
    /// additional library search paths (`-L C:\...\lib`), repeatable
    lib_paths: Vec<String>,
    /// codegen backend; only "c" exists since v0.29.0 (the flag is accepted
    /// for compatibility with v0.27.x/v0.28.x command lines)
    backend: Option<String>,
    /// extra flags forwarded verbatim to every clang invocation (v0.42.0),
    /// e.g. `-g`, `-fsanitize=undefined`, `-fno-strict-aliasing`
    clang_args: Vec<String>,
    /// show clang's own warnings about the generated C (drops the hard-coded
    /// `-w`) — this is how a wrong `extern def` signature becomes visible
    cc_warnings: bool,
}

fn parse_opts(args: &[String]) -> Opts {
    let mut opts = Opts {
        out: None,
        emit_assets: None,
        tailwind: false,
        opt_level: DEFAULT_OPT_LEVEL,
        json: false,
        cpu: None,
        positional: Vec::new(),
        passthrough: Vec::new(),
        libs: Vec::new(),
        lib_paths: Vec::new(),
        backend: None,
        clang_args: Vec::new(),
        cc_warnings: false,
    };
    let mut level_flags = 0usize;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" if i + 1 < args.len() => {
                opts.out = Some(args[i + 1].clone());
                i += 2;
            }
            "--emit-assets" if i + 1 < args.len() => {
                opts.emit_assets = Some(args[i + 1].clone());
                i += 2;
            }
            "--tailwind" => {
                opts.tailwind = true;
                i += 1;
            }
            "-l" if i + 1 < args.len() => {
                opts.libs.push(args[i + 1].clone());
                i += 2;
            }
            "-L" if i + 1 < args.len() => {
                opts.lib_paths.push(args[i + 1].clone());
                i += 2;
            }
            "--O0" | "-O0" => {
                opts.opt_level = 0;
                level_flags += 1;
                i += 1;
            }
            "--O1" | "-O1" => {
                opts.opt_level = 1;
                level_flags += 1;
                i += 1;
            }
            "--O2" | "-O2" => {
                opts.opt_level = 2;
                level_flags += 1;
                i += 1;
            }
            "--O3" | "-O3" => {
                opts.opt_level = 3;
                level_flags += 1;
                i += 1;
            }
            "--cpu" if i + 1 < args.len() => {
                opts.cpu = Some(args[i + 1].clone());
                i += 2;
            }
            "--backend" if i + 1 < args.len() => {
                let b = args[i + 1].to_ascii_lowercase();
                if b != "c" {
                    eprintln!(
                        "error: --backend must be 'c' (got '{}'); the LLVM backend was \
                         removed in v0.29.0 and the C-emitting backend is the only backend",
                        args[i + 1]
                    );
                    std::process::exit(2);
                }
                opts.backend = Some(b);
                i += 2;
            }
            "--json" => {
                opts.json = true;
                i += 1;
            }
            // clang passthrough (v0.42.0): `--clang-arg -g` reaches both the
            // compile and the link step; `-g` alone is the common case, so it
            // is spelled as its own flag
            "--clang-arg" if i + 1 < args.len() => {
                opts.clang_args.push(args[i + 1].clone());
                i += 2;
            }
            "-g" => {
                opts.clang_args.push("-g".to_string());
                i += 1;
            }
            "--cc-warnings" => {
                opts.cc_warnings = true;
                i += 1;
            }
            "--" => {
                opts.passthrough = args[i + 1..].to_vec();
                break;
            }
            other => {
                opts.positional.push(other.to_string());
                i += 1;
            }
        }
    }
    if level_flags > 1 {
        eprintln!("error: at most one of --O0/--O1/--O2/--O3 may be given");
        std::process::exit(2);
    }
    // target CPU for the C compile (e.g. `native`); also settable via AOXN_CPU
    if let Some(cpu) = &opts.cpu {
        std::env::set_var("AOXN_CPU", cpu);
    }
    // `--clang-arg` / `-g` travel to codegen and the link step through one
    // env var (`AOXN_CLANG_ARGS`, newline-separated so a flag may contain
    // spaces), the same way `--cpu` does — no pipeline signature has to grow
    // an argument for it.
    if !opts.clang_args.is_empty() {
        std::env::set_var("AOXN_CLANG_ARGS", opts.clang_args.join("\n"));
    }
    if opts.cc_warnings {
        std::env::set_var("AOXN_CC_WARNINGS", "1");
    }
    // `--tailwind` is a build-time source scan, not a codegen switch, so it
    // travels to the loader the same way --cpu does rather than threading a
    // new parameter through every pipeline entry point.
    if opts.tailwind {
        std::env::set_var("AOXN_TAILWIND", "1");
    }
    opts
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        print_help();
        std::process::exit(2);
    }
    // package-manager commands: `aoxn pkg <cmd>` plus the direct aliases
    // (`aoxn add`, `aoxn install`, ...). Handled by the aoxn-pkg crate; the
    // compiler paths below are untouched.
match args[0].as_str() {
        "--help" | "-h" | "help" => print_help(),
        "--version" | "-V" | "version" => {
            println!("aoxn {}", env!("CARGO_PKG_VERSION"));
        }
        "build" => cmd_build(&args[1..]),
        "run" => cmd_run(&args[1..]),
        "c" => cmd_c(&args[1..]),
        "check" => cmd_check(&args[1..]),
        "symbols" => cmd_symbols(&args[1..]),
        // `ir` was the LLVM-IR dump until v0.28.0; it now forwards to `c`
        "ir" => cmd_c(&args[1..]),
        "doctor" => std::process::exit(doctor::cmd(&args[1..])),
        "pkg" => std::process::exit(aoxn_pkg::run(&args[1..])),
        "init" | "add" | "remove" | "install" | "update" | "outdated" | "tree" | "why"
        | "publish" | "yank" | "audit" | "uninstall" | "cache" | "npm-import" | "list"
        | "freeze" | "trust" => std::process::exit(aoxn_pkg::run(&args)),
        other => {
            eprintln!("error: unknown command '{other}' (try: Aoxn --help)");
            std::process::exit(2);
        }
    }
}

fn print_help() {
    println!(
        "Aoxn compiler v{}\n\n\
         USAGE:\n  \
         Aoxn build <file.ax> [-o out] [--O0|--O1] [--json]   compile to a native executable\n  \
         Aoxn run <file.ax> [--O0|--O1] [--json] [-- args...]  compile and run in one step\n  \
         Aoxn c <file.ax> [--json]                            print the generated C\n  \
         Aoxn check <file.ax> [--json]                        type-check: run the pipeline, print only\n  \
                                                           diagnostics (no C text, no clang)\n  \
         Aoxn symbols <file.ax> [--json]                      list top-level declarations with positions\n  \
                                                           (the editor's language service: no clang)\n  \
         Aoxn doctor [--json] [--no-smoke]                   check the installed toolchain\n  \
         Aoxn version                                         print the compiler version\n\n\
         FLAGS:\n  \
         -o <path>   output executable path (default: <file>.exe)\n  \
         --O0        no optimization (clang -O0)\n  \
         --O1        fast compile (clang -O1): for iteration and compile-sensitive CI\n  \
         --O2        clang -O2 (compile time ~= O3)\n  \
         --O3        clang -O3 (default: best runtime performance)\n  \
         --cpu <c>   target CPU for codegen, e.g. native (default: generic)\n  \
         --backend <b>  codegen backend; only 'c' exists since v0.29.0 (accepted\n  \
                     for compatibility with older command lines)\n  \
         --emit-assets <dir>  write fingerprinted CSS + url() targets here, so a server can\n  \
                     serve them over <link> (a relative path resolves against the\n  \
                     output executable's directory). build only\n  \
         --tailwind    scan the sources for Tailwind-style class names and generate a\n\
                     utility subset into the CSS bundle (see docs/css-assets.md)\n  \
         --json      emit diagnostics as JSON (AI-agent friendly)\n\n\
         PACKAGES:\n  \
         Aoxn pkg <cmd>         package management (same as the direct aliases below)\n  \
         Aoxn init|add|remove|install|update|outdated|tree|why|publish|yank|audit|cache|npm-import\n\n\
         ENV:\n  \
         AOXN_CPU=native         same as --cpu native\n  \
         AOXN_NO_CACHE=1         disable the `Aoxn run` build cache\n  \
         AOXN_CACHE_DIR=<dir>    build-cache location (default: <cwd>/target/cache)\n  \
         AOXN_HOME=<dir>         toolchain root (default: the parent of this executable's bin/)\n  \
         AOXN_STDLIB=<dir>       stdlib sources (default: <root>/lib/stdlib)\n  \
         AOXN_CLANG=<path>       clang driver to compile and link with",
        env!("CARGO_PKG_VERSION")
    );
}

fn report(diags: &[Diag], json: bool) {
    // a failed compile still owns any warnings collected before the error
    let warnings = aoxn::take_warnings();
    if json {
        eprintln!("{}", aoxn::report_to_json(diags, &warnings));
    } else {
        for d in &warnings {
            eprintln!("{}", aoxn::diag_to_string(d));
        }
        for d in diags {
            eprintln!("{}", aoxn::diag_to_string(d));
        }
    }
}

/// Report a SUCCESSFUL compile: warnings only, plus the report document when
/// `--json` asked for machine output. Always on stderr — stdout belongs to the
/// program (`run`) or to the C text (`c`).
fn report_ok(json: bool) {
    let warnings = aoxn::take_warnings();
    if json {
        eprintln!("{}", aoxn::report_to_json(&[], &warnings));
    } else {
        for d in &warnings {
            eprintln!("{}", aoxn::diag_to_string(d));
        }
    }
}

fn cmd_build(args: &[String]) {
    let opts = parse_opts(args);
    if opts.positional.is_empty() {
        eprintln!("error: 'Aoxn build' needs an input file (.ax)");
        std::process::exit(2);
    }
    // the first file is the entry; `import "..."` pulls in the rest
    let exe = opts
        .out
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| default_exe(&opts.positional[0]));

    // Content-hash cache (F3, shared with `run`): a repeated build of
    // unchanged sources copies the cached executable instead of recompiling
    // and re-linking. Same key as `run`, so a `run` followed by a `build` of
    // the same program shares one cache entry.
    let cache_path = cache_key(&opts).map(|k| cache_dir().join(format!("{k}{}", exe_suffix())));
    if let Some(cached) = &cache_path {
        if cached.is_file() {
            // bump the mtime so a hot entry survives `prune_cache`'s
            // oldest-first pass (approximate LRU, best-effort)
            let _ = std::fs::OpenOptions::new()
                .write(true)
                .open(cached)
                .and_then(|f| f.set_modified(std::time::SystemTime::now()));
            publish_from_cache(cached, &exe);
            // Assets are emitted even on a cache hit: the cached exe is
            // byte-identical, so the asset files are too, but they live
            // beside the *output* exe, which the cache does not know about.
            emit_assets(&opts, &exe);
            println!("{}", exe.display());
            return;
        }
    }

    // Build into the cache directory under a unique name, then publish it,
    // so concurrent invocations never race on the same output file.
    let build_path = match &cache_path {
        Some(final_path) => {
            let stem = final_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            cache_dir().join(format!("{stem}.{}{}", std::process::id(), exe_suffix()))
        }
        None => exe.clone(),
    };
    if let Err(diags) = aoxn::build_paths_opts_lvl(
        &opts.positional,
        &build_path,
        opts.opt_level,
        &opts.libs,
        &opts.lib_paths,
    ) {
        report(&diags, opts.json);
        std::process::exit(1);
    }
    // the compile produced the warnings; drain them before the asset step
    // (which re-runs the loader and would otherwise be reported against)
    report_ok(opts.json);

    match &cache_path {
        Some(final_path) => {
            let _ = std::fs::remove_file(final_path);
            if std::fs::rename(&build_path, final_path).is_ok() {
                prune_cache();
                publish_from_cache(final_path, &exe);
            } else {
                // publish failed (e.g. AV lock): copy the unique build out
                publish_from_cache(&build_path, &exe);
            }
        }
        None => {} // cache disabled: build_path IS the output
    }
    emit_assets(&opts, &exe);
    println!("{}", exe.display());
}

/// Write fingerprinted CSS and `url(...)` targets next to the output
/// executable when `--emit-assets` asked for it.
///
/// A relative `--emit-assets` path resolves against the *executable's*
/// directory, not the cwd, so `aoxn build app.ax --emit-assets assets` means
/// the same thing regardless of where the command was run from.
///
/// Failure here is not fatal: the executable is already built and correct, and
/// an asset directory is a deployment convenience. But it must be loud — a
/// silently missing `app.<hash>.css` shows up as an unstyled page much later.
fn emit_assets(opts: &Opts, exe: &Path) {
    let Some(target) = &opts.emit_assets else { return };
    let dir = {
        let p = PathBuf::from(target);
        if p.is_absolute() {
            p
        } else {
            exe.parent().unwrap_or_else(|| Path::new(".")).join(p)
        }
    };
    match aoxn::collect_assets(&opts.positional) {
        Ok(assets) => {
            if assets.is_empty() {
                return;
            }
            match assets.emit(&dir) {
                Ok(written) => {
                    for path in &written {
                        println!("asset: {}", path.display());
                    }
                }
                Err(d) => report(&[d], opts.json),
            }
        }
        Err(diags) => report(&diags, opts.json),
    }
}

/// Copy a cache entry to the user-visible output path. `build` cannot just
/// rename: the cache entry must survive for the next invocation.
fn publish_from_cache(cached: &Path, exe: &Path) {
    if cached == exe {
        return; // pathological -o inside the cache dir itself
    }
    if let Some(dir) = exe.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::copy(cached, exe).is_err() {
        eprintln!("error: failed to write output {}", exe.display());
        std::process::exit(1);
    }
}

fn cmd_run(args: &[String]) {
    let opts = parse_opts(args);
    if opts.positional.is_empty() {
        eprintln!("error: 'Aoxn run' needs an input file (.ax)");
        std::process::exit(2);
    }
    // program args: anything after "--"
    let prog_args = &opts.passthrough;

    // Build cache (F3): `Aoxn run` is usually "edit one line, run again", and
    // for small programs the link + process startup dominate (the compile
    // itself is ~18ms). Key the cached executable on the *content* of the
    // whole program (entry + all transitive imports), the compiler binary
    // itself, and every option that changes codegen.
    let cache_path = cache_key(&opts).map(|k| cache_dir().join(format!("{k}{}", exe_suffix())));
    if let Some(cached) = &cache_path {
        if cached.is_file() {
            // bump the mtime so a hot entry survives `prune_cache`'s
            // oldest-first pass (approximate LRU, best-effort)
            let _ = std::fs::OpenOptions::new()
                .write(true)
                .open(cached)
                .and_then(|f| f.set_modified(std::time::SystemTime::now()));
            std::process::exit(run_exe(cached, prog_args));
        }
    }

    // Build into the cache directory under a unique name, then publish it, so
    // concurrent `Aoxn run` invocations never race on the same output file.
    let build_path = match &cache_path {
        Some(final_path) => {
            let stem = final_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            cache_dir().join(format!("{stem}.{}{}", std::process::id(), exe_suffix()))
        }
        None => temp_exe(&opts.positional[0]),
    };
    if let Err(diags) = aoxn::build_paths_opts_lvl(
        &opts.positional,
        &build_path,
        opts.opt_level,
        &opts.libs,
        &opts.lib_paths,
    ) {
        report(&diags, opts.json);
        std::process::exit(1);
    }
    // warnings belong to the compile, not to the program's own output
    report_ok(opts.json);

    let exe = match &cache_path {
        Some(final_path) => {
            let _ = std::fs::remove_file(final_path);
            if std::fs::rename(&build_path, final_path).is_err() {
                build_path // publish failed (e.g. AV lock): run the unique copy
            } else {
                prune_cache();
                final_path.clone()
            }
        }
        None => build_path,
    };
    let status = run_exe(&exe, prog_args);
    if cache_path.is_none() {
        let _ = std::fs::remove_file(&exe); // temp copy, no cache entry
    }
    std::process::exit(status);
}

fn cmd_c(args: &[String]) {
    let opts = parse_opts(args);
    if opts.positional.is_empty() {
        eprintln!("error: 'Aoxn c' needs an input file (.ax)");
        std::process::exit(2);
    }
    match aoxn::compile_paths_to_c_lvl(&opts.positional, opts.opt_level) {
        Ok(text) => {
            // warnings first: stdout is the C text, stderr is the metadata
            report_ok(opts.json);
            print!("{text}")
        }
        Err(diags) => {
            report(&diags, opts.json);
            std::process::exit(1);
        }
    }
}

/// `Aoxn check`: run the pipeline up to codegen and throw the C text away —
/// the product is the diagnostics and the exit code, not the output. The
/// Aoxn IDE's Check button calls exactly this: `aoxn c` would work too, but
/// it prints the generated C to stdout, which is noise in an editor panel.
fn cmd_check(args: &[String]) {
    let opts = parse_opts(args);
    if opts.positional.is_empty() {
        eprintln!("error: 'Aoxn check' needs an input file (.ax)");
        std::process::exit(2);
    }
    match aoxn::compile_paths_to_c_lvl(&opts.positional, opts.opt_level) {
        Ok(_) => report_ok(opts.json),
        Err(diags) => {
            report(&diags, opts.json);
            std::process::exit(1);
        }
    }
}

/// `Aoxn symbols`: list the top-level declarations of a program and every
/// module it imports, with signatures and source positions.
///
/// This is the language-service export the IDE builds its outline, its
/// "go to definition" and its symbol search on. It runs the loader and the
/// parser and stops there — no typecheck, no codegen, no clang — because an
/// editor re-asks this question on every save and after every keystroke
/// pause, and a check that shells out to clang would be far too slow.
///
/// A file that does not parse reports diagnostics and exits 1, exactly as
/// `check` does: a program with a syntax error has no outline worth showing,
/// and half an outline is worse than none.
fn cmd_symbols(args: &[String]) {
    let opts = parse_opts(args);
    if opts.positional.is_empty() {
        eprintln!("error: 'Aoxn symbols' needs an input file (.ax)");
        std::process::exit(2);
    }
    match aoxn::program_symbols(&opts.positional) {
        Ok(symbols) => {
            if opts.json {
                println!("{}", aoxn::symbols::to_json(&symbols));
            } else {
                print!("{}", aoxn::symbols::to_text(&symbols));
            }
        }
        Err(diags) => {
            report(&diags, opts.json);
            std::process::exit(1);
        }
    }
}

fn default_exe(input: &str) -> PathBuf {
    let mut p = PathBuf::from(input);
    p.set_extension(aoxn::platform::exe_ext());
    p
}

/// Executable suffix with the leading dot (`".exe"` on Windows, `""` elsewhere)
/// for use in `format!`; `platform::exe_ext()` is dot-less for `set_extension`.
fn exe_suffix() -> String {
    let e = aoxn::platform::exe_ext();
    if e.is_empty() {
        String::new()
    } else {
        format!(".{e}")
    }
}

fn temp_exe(input: &str) -> PathBuf {
    let stem = PathBuf::from(input)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "program".into());
    let dir = std::env::temp_dir().join("Aoxn-run");
    let _ = std::fs::create_dir_all(&dir);
    let unique = format!("{}-{}{}", stem, std::process::id(), exe_suffix());
    dir.join(unique)
}

/// Run a compiled program, inheriting stdio, and return its exit code.
fn run_exe(exe: &Path, args: &[String]) -> i32 {
    let status = Command::new(exe).args(args).status().unwrap_or_else(|e| {
        eprintln!("error: failed to run compiled program {}: {e}", exe.display());
        std::process::exit(1);
    });
    status.code().unwrap_or(1)
}

// ---- `Aoxn run` build cache ----

/// Directory holding cached executables: `AOXN_CACHE_DIR`, else
/// `<cwd>/target/cache`, falling back to the temp dir when that is not
/// writable (read-only source trees).
fn cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("AOXN_CACHE_DIR") {
        let p = PathBuf::from(dir);
        let _ = std::fs::create_dir_all(&p);
        return p;
    }
    let local = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("target")
        .join("cache");
    if std::fs::create_dir_all(&local).is_ok() {
        local
    } else {
        let fallback = std::env::temp_dir().join("aoxn-cache");
        let _ = std::fs::create_dir_all(&fallback);
        fallback
    }
}

/// Cache key for this invocation, or `None` when caching is disabled or the
/// dependency set cannot be read (then we always compile).
fn cache_key(opts: &Opts) -> Option<String> {
    use std::hash::{BuildHasher, Hash, Hasher};

    if std::env::var("AOXN_NO_CACHE").is_ok() {
        return None;
    }
    // every source file the compiler will read, entry + transitive imports
    let sources = aoxn::dependency_files(&opts.positional)?;

    let mut h = aoxn::hashing::FastBuild.build_hasher();
    h.write(b"aoxn-run-cache-v1");
    // source content: the only input that usually changes
    for path in &sources {
        path.to_string_lossy().hash(&mut h);
        std::fs::read(path).ok()?.hash(&mut h);
    }
    // the compiler binary itself: rebuilding `aoxn` must invalidate the cache
    if let Ok(exe) = std::env::current_exe() {
        if let Ok(md) = std::fs::metadata(&exe) {
            md.len().hash(&mut h);
            if let Ok(t) = md.modified() {
                if let Ok(d) = t.duration_since(std::time::UNIX_EPOCH) {
                    d.as_nanos().hash(&mut h);
                }
            }
        }
    }
    // every option that changes generated code
    opts.opt_level.hash(&mut h);
    std::env::var("AOXN_CPU").unwrap_or_default().hash(&mut h);
    // clang flags change the OBJECT, so they belong in the key (v0.42.0)
    opts.clang_args.hash(&mut h);
    std::env::var("AOXN_CC_WARNINGS").unwrap_or_default().hash(&mut h);
    opts.libs.hash(&mut h);
    opts.lib_paths.hash(&mut h);
    // linked-in toolchain identity (cheap: the resolved clang path)
    aoxn::find_clang().map(|p| p.display().to_string()).hash(&mut h);
    // `--emit-assets` does not change the executable, but the cache-hit path
    // returns before the emit step runs, so an emit dir that changed since the
    // entry was written would silently keep publishing into the old directory.
    // Hash it (resolved against the cwd, since that is what a relative path
    // means at key time) so a different destination is a different entry.
    opts.emit_assets.hash(&mut h);
    // `--tailwind` changes the *generated* CSS, so it belongs with the codegen
    // options rather than beside --emit-assets
    opts.tailwind.hash(&mut h);
    if let Some(target) = &opts.emit_assets {
        std::fs::canonicalize(target)
            .unwrap_or_else(|_| PathBuf::from(target))
            .to_string_lossy()
            .hash(&mut h);
    }
    Some(format!("{:016x}", h.finish()))
}

/// Keep the cache bounded: after a publish, drop the oldest entries beyond
/// `MAX_CACHE_ENTRIES` (cache hits bump the entry's mtime, so this is an
/// approximate LRU). Best-effort — failures are ignored.
fn prune_cache() {
    const MAX_CACHE_ENTRIES: usize = 64;
    let dir = cache_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            // only published entries (16 hex digits, optionally + the exe
            // suffix) — never the `<key>.<pid>` temp file of a concurrent
            // invocation
            let name = path.file_name()?.to_string_lossy().into_owned();
            let suffix = exe_suffix();
            let core = name.strip_suffix(suffix.as_str()).unwrap_or(&name);
            if core.len() != 16 || !core.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            let md = e.metadata().ok()?;
            if !md.is_file() {
                return None;
            }
            Some((md.modified().unwrap_or(std::time::UNIX_EPOCH), path))
        })
        .collect();
    if files.len() <= MAX_CACHE_ENTRIES {
        return;
    }
    files.sort_by_key(|(t, _)| *t);
    for (_, path) in files.iter().take(files.len() - MAX_CACHE_ENTRIES) {
        let _ = std::fs::remove_file(path);
    }
}
