//! Installation-layout discovery (v0.30.0).
//!
//! `aoxn` is distributed as a *portable* toolchain: the single-file installer
//! (`Aoxn-<version>-Setup.exe`) unpacks to
//!
//! ```text
//! <root>/
//!   bin/aoxn[.exe]         the compiler driver (this program)
//!   lib/stdlib/*.ax        the standard library + the UI toolkit
//!   examples/*.ax          runnable samples (optional, `aoxn doctor` lists them)
//!   toolchain/bin/clang    OPTIONAL bundled C toolchain (drop LLVM in here)
//!   docs/install.md
//! ```
//!
//! Everything here is *discovery*, never installation: the driver looks for
//! the layout relative to its own executable, so the same binary works when
//! run from a release archive, from `cargo run` in a source checkout, or from
//! a package manager that dropped it anywhere on `PATH`. Resolution order for
//! each piece (first hit wins):
//!
//! - install root: `$AOXN_HOME` -> the parent of the executable's `bin/` dir
//!   -> nothing (source checkout)
//! - stdlib: `$AOXN_STDLIB` -> `<root>/lib/stdlib` -> `<root>/stdlib` ->
//!   the checkout this binary was built from (developer builds)
//! - bundled clang: `<root>/toolchain/bin/clang` (a portable LLVM dropped in
//!   by the installer), searched *after* `$AOXN_CLANG` and `PATH` so an
//!   explicit choice always wins.

use std::path::{Path, PathBuf};

/// Platform-specific executable name (`aoxn.exe` / `aoxn`).
pub const EXE_NAME: &str = if cfg!(windows) { "aoxn.exe" } else { "aoxn" };

/// clang driver file names, most specific first (Windows accepts both).
pub fn clang_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["clang.exe", "clang"]
    } else {
        &["clang"]
    }
}

fn env_dir(key: &str) -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os(key)?);
    if p.is_dir() {
        Some(p)
    } else {
        None
    }
}

/// Directory containing the running executable (symlinks resolved).
pub fn exe_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.to_path_buf();
    // Resolve the common symlink case: ~/.local/bin/aoxn -> <root>/bin/aoxn.
    // `canonicalize` needs the file to exist, which it does here.
    match std::fs::canonicalize(&dir) {
        Ok(real) => Some(strip_verbatim(real)),
        Err(_) => Some(dir),
    }
}

/// Windows `canonicalize` returns `\\?\C:\...` verbatim paths. They work for
/// every syscall, but they are unreadable in diagnostics and break naive
/// string comparisons, so strip the prefix when it is there.
///
/// Public because the spelling has to be consistent EVERYWHERE a path is
/// shown or compared: the loader registers canonicalized import paths under
/// this name, so a diagnostic that names an imported file and a symbol
/// exported from it have to agree on the spelling, or an editor cannot match
/// one against the other.
pub fn strip_verbatim(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        // UNC verbatim forms (\\?\UNC\server\share) keep the \\server part
        if let Some(unc) = rest.strip_prefix(r"UNC\") {
            return PathBuf::from(format!(r"\\{unc}"));
        }
        return PathBuf::from(rest);
    }
    path
}

/// The toolchain root this executable belongs to, if it is laid out like a
/// release install. `None` in a source checkout (where `stdlib_dir` still
/// resolves through `CARGO_MANIFEST_DIR`).
pub fn install_root() -> Option<PathBuf> {
    if let Some(root) = env_dir("AOXN_HOME") {
        return Some(root);
    }
    let dir = exe_dir()?;
    let parent = dir.parent()?.to_path_buf();
    let named_bin = dir.file_name().map(|n| n == "bin").unwrap_or(false);
    let has_lib = parent.join("lib").join("stdlib").is_dir();
    let has_flat = parent.join("stdlib").is_dir();
    if named_bin || has_lib || has_flat {
        Some(parent)
    } else {
        None
    }
}

// The stdlib directory is asked for on the import path: `resolve_import_spelled`
// reaches for it every time a package spelling misses, so a program importing
// N things did up to N rounds of `getenv` + `is_dir` probing against the
// filesystem. Memoizing is safe because the answer cannot change under a
// running process: the environment is fixed at exec and the install layout
// does not move while the compiler walks it. `doctor` deliberately wants the
// LIVE answer (it is diagnosing this very directory), so it goes through
// `stdlib_dir_uncached`.
thread_local! {
    static STDLIB_DIR: std::cell::RefCell<Option<Option<PathBuf>>> =
        const { std::cell::RefCell::new(None) };
}

/// Directory holding `stdlib.ax` and the UI toolkit sources, answered once per
/// process. See [`stdlib_dir_uncached`] for the probe itself and for why the
/// answer may be cached.
pub fn stdlib_dir() -> Option<PathBuf> {
    STDLIB_DIR.with(|c| {
        if let Some(hit) = c.borrow().as_ref() {
            return hit.clone();
        }
        let fresh = stdlib_dir_uncached();
        *c.borrow_mut() = Some(fresh.clone());
        fresh
    })
}

/// The uncached probe behind [`stdlib_dir`].
pub fn stdlib_dir_uncached() -> Option<PathBuf> {
    if let Some(dir) = env_dir("AOXN_STDLIB") {
        return Some(dir);
    }
    if let Some(root) = install_root() {
        for candidate in [root.join("lib").join("stdlib"), root.join("stdlib")] {
            if candidate.is_dir() {
                return Some(candidate);
            }
        }
    }
    // developer build: the checkout this binary was compiled from
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("stdlib");
    if dev.is_dir() {
        Some(dev)
    } else {
        None
    }
}

/// `<root>/examples` when the archive shipped samples.
pub fn examples_dir() -> Option<PathBuf> {
    let root = install_root()?;
    for candidate in [root.join("examples"), root.join("lib").join("examples")] {
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

/// A clang driver shipped *inside* the toolchain root (`toolchain/bin/clang`),
/// i.e. the portable-LLVM drop-in the installer can provision.
pub fn bundled_clang() -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(root) = install_root() {
        roots.push(root.join("toolchain").join("bin"));
        // a plain LLVM zip extracted next to the root (LLVM/bin/clang)
        roots.push(root.join("LLVM").join("bin"));
    }
    if let Some(dir) = exe_dir() {
        roots.push(dir.join("toolchain").join("bin"));
    }
    first_existing(&roots)
}

/// First existing file named `clang[.exe]` under any of `dirs`.
pub fn first_existing(dirs: &[PathBuf]) -> Option<PathBuf> {
    for dir in dirs {
        for name in clang_names() {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Files every install is expected to provide; `doctor` lists them. The UI
/// toolkit is Win32/GDI only since v0.30.0 dropped the X11 backend.
pub const STDLIB_FILES: &[&str] = &["stdlib.ax", "ui.ax", "ui_draw.ax", "ui_win.ax"];

/// A stdlib file present at `dir`, as (name, present).
pub fn stdlib_status(dir: Option<&Path>) -> Vec<(String, bool)> {
    STDLIB_FILES
        .iter()
        .map(|name| {
            let present = dir.map(|d| d.join(name).is_file()).unwrap_or(false);
            ((*name).to_string(), present)
        })
        .collect()
}
