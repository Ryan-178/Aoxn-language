//! Install-layout tests (v0.30.0): the stdlib must resolve by name from any
//! directory, `AOXN_STDLIB` must be able to redirect it, and the `doctor`
//! command must describe the toolchain it can actually see.
//!
//! These drive the real `aoxn` binary (`CARGO_BIN_EXE_aoxn`) in a scratch
//! directory rather than the library, because the point is what a *user's*
//! machine resolves — including the executable-relative fallbacks that only
//! exist once a process is running.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

const AOXN: &str = env!("CARGO_BIN_EXE_aoxn");
static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Scratch directory outside the repo, removed when the test ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("aoxn-install-test-{}-{tag}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        Scratch(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let p = self.0.join(name);
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).expect("create scratch subdir");
        }
        std::fs::write(&p, text).expect("write scratch file");
        p
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Run the driver in `dir` with extra environment; returns (status, stdout).
fn run_in(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> (Option<i32>, String) {
    let mut cmd = Command::new(AOXN);
    cmd.current_dir(dir).args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("failed to spawn aoxn");
    (
        out.status.code(),
        format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)),
    )
}

fn cache(dir: &Path) -> PathBuf {
    dir.join("cache")
}

/// The three tests below compile and run programs (or ask `doctor` to), which
/// needs clang. On a host without one — a Linux CI box that only wants the
/// pure-Rust surface, for example — they skip, exactly like the SDK / UI /
/// self-hosting tests do.
fn have_clang() -> bool {
    aoxn::find_clang().is_some()
}

/// `import * from "stdlib"` works from an unrelated working directory — the
/// whole point of a one-click install (docs/install.md).
#[test]
fn stdlib_imports_resolve_by_name_from_any_directory() {
    if !have_clang() {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    }
    let scratch = Scratch::new("byname");
    let prog = scratch.write(
        "prog.ax",
        "import * from \"stdlib\"\n\ndef main() -> int:\n    print(isqrt(99))\n    return 0\n",
    );
    let cache_dir = cache(scratch.path());
    let (code, out) = run_in(
        scratch.path(),
        &["run", &prog.display().to_string()],
        &[("AOXN_CACHE_DIR", &cache_dir.display().to_string())],
    );
    assert_eq!(code, Some(0), "aoxn run failed: {out}");
    assert_eq!(out.trim(), "9", "unexpected output: {out}");
}

/// `AOXN_STDLIB` redirects the bare `stdlib` specifier (used to test an
/// unreleased stdlib without touching the install).
#[test]
fn aoxn_stdlib_env_redirects_bare_imports() {
    if !have_clang() {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    }
    let scratch = Scratch::new("env");
    scratch.write(
        "mystdlib/mystdlib.ax",
        "def answer() -> int:\n    return 42\n",
    );
    let prog = scratch.write(
        "prog.ax",
        "import * from \"mystdlib\"\n\ndef main() -> int:\n    print(answer())\n    return 0\n",
    );
    let custom = scratch.path().join("mystdlib");
    let (code, out) = run_in(
        scratch.path(),
        &["run", &prog.display().to_string()],
        &[
            ("AOXN_STDLIB", &custom.display().to_string()),
            ("AOXN_CACHE_DIR", &cache(scratch.path()).display().to_string()),
        ],
    );
    assert_eq!(code, Some(0), "AOXN_STDLIB run failed: {out}");
    assert_eq!(out.trim(), "42", "unexpected output: {out}");

    // ...and without it the same program cannot resolve (nothing named
    // `mystdlib` in aox_modules/, and the checkout has no such module)
    let (code2, _out2) = run_in(
        scratch.path(),
        &["run", &prog.display().to_string()],
        &[("AOXN_CACHE_DIR", &cache(scratch.path()).display().to_string())],
    );
    assert_ne!(code2, Some(0), "an unresolvable import must fail");
}

/// The bundled examples import the stdlib by NAME, so they must compile from
/// wherever the archive was unpacked (this is what CI's installer job runs).
#[test]
fn bundled_examples_resolve_stdlib_by_name() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for name in ["stdlib_demo.ax", "ui_gallery.ax", "ui_demo.ax"] {
        let entry = repo.join("examples").join(name);
        let text = std::fs::read_to_string(&entry).expect("example must exist");
        assert!(
            text.contains("from \"stdlib"),
            "{name} must import the stdlib by name so a packaged install works"
        );
        // `Aoxn c` needs no clang: it stops after C emission, so this checks
        // import resolution alone on any machine.
        let (code, out) = run_in(&repo, &["c", &entry.display().to_string()], &[]);
        assert_eq!(code, Some(0), "{name} failed to emit C: {out}");
        assert!(out.contains("int main"), "{name} produced no C: {out}");
    }
}

/// `aoxn doctor` reports the layout; on a machine with a working C toolchain
/// its own smoke test must pass too.
#[test]
fn doctor_reports_the_toolchain() {
    if !have_clang() {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    }
    let scratch = Scratch::new("doctor");
    let cache_dir = cache(scratch.path()).display().to_string();
    let (code, out) = run_in(scratch.path(), &["doctor", "--json"], &[("AOXN_CACHE_DIR", &cache_dir)]);
    assert_eq!(code, Some(0), "doctor --json failed: {out}");
    assert!(out.contains("\"ok\":true"), "doctor reported problems: {out}");
    assert!(out.contains("\"name\":\"stdlib\""), "doctor did not report the stdlib: {out}");
    assert!(out.contains("\"smoke\":{\"ok\":true"), "the smoke test did not run: {out}");

    // the human-readable form mentions the same essentials
    let scratch2 = Scratch::new("doctor-text");
    let cache2 = cache(scratch2.path()).display().to_string();
    let (code, text) = run_in(scratch2.path(), &["doctor"], &[("AOXN_CACHE_DIR", &cache2)]);
    assert_eq!(code, Some(0), "doctor failed: {text}");
    for needle in ["install root", "stdlib", "clang", "smoke test", "status: ok"] {
        assert!(text.contains(needle), "doctor output is missing {needle:?}: {text}");
    }
}

/// `--no-smoke` skips the compile step and still reports success.
#[test]
fn doctor_no_smoke_skips_the_compile() {
    let scratch = Scratch::new("doctor-quick");
    let cache_dir = cache(scratch.path()).display().to_string();
    let (code, out) = run_in(
        scratch.path(),
        &["doctor", "--json", "--no-smoke"],
        &[("AOXN_CACHE_DIR", &cache_dir)],
    );
    assert_eq!(code, Some(0), "doctor --no-smoke failed: {out}");
    assert!(out.contains("\"smoke\":null"), "smoke test should be skipped: {out}");
}

/// `aoxn version` / `--version` print the manifest version.
#[test]
fn version_command_prints_the_manifest_version() {
    let scratch = Scratch::new("version");
    for args in [vec!["version"], vec!["--version"]] {
        let (code, out) = run_in(scratch.path(), &args, &[]);
        assert_eq!(code, Some(0), "`aoxn {}` failed: {out}", args[0]);
        assert!(
            out.trim() == format!("aoxn {}", env!("CARGO_PKG_VERSION")),
            "unexpected version output: {out}"
        );
    }
}