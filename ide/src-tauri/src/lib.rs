//! The Aoxn IDE — Tauri backend.
//!
//! The command surface is deliberately small and deliberately narrow. Every
//! filesystem path that arrives from the webview goes through
//! [`fsops::Workspace::resolve`] first, so the frontend cannot reach outside
//! the folder the user opened; there is no general "read any path" command,
//! and no shell plugin (see `capabilities/default.json`).
//!
//! The commands fall into five groups:
//!
//! * **workspace** — `ide_open_folder`, `ide_scan`, `ide_root`
//! * **files**     — `ide_read`, `ide_save`, `ide_new_file`, `ide_new_dir`
//! * **toolchain** — `ide_toolchain`, `ide_doctor`
//! * **run**       — `ide_check`, `ide_build`, `ide_run`, `ide_build_and_run`
//! * **language**  — `ide_symbols` (the outline / go-to-definition source)
//! * **packages**  — `ide_pkg_manifest`, `ide_pkg_report`, `ide_pkg_run`
//!   (whitelisted `aoxn pkg` subcommands only; see `pkg.rs`)
//!
//! The log comes back verbatim and the diagnostics come back STRUCTURED, and
//! both are kept: `output` is what the panel shows (including clang's own
//! words and a program's prints), while `diags` is what the editor's markers
//! and the problem list are built from. The frontend only falls back to
//! scanning text when `diags` is empty, which is why the text scanner still
//! exists but is no longer the primary path.

mod fsops;
mod model;
mod pkg;
mod toolchain;

use fsops::Workspace;
use model::{
    ExecResult, FileContents, PkgManifestInfo, PkgReport, SymbolTable, ToolchainInfo, TreeNode,
};
use std::sync::Mutex;
use tauri::{Manager, State};

/// The open folder, if any. `None` until the user picks one.
#[derive(Default)]
struct AppState(Mutex<Option<Workspace>>);

fn workspace(state: &State<AppState>) -> Result<Workspace, String> {
    state
        .0
        .lock()
        .map_err(|_| "workspace lock poisoned".to_string())?
        .clone()
        .ok_or_else(|| "no folder is open".to_string())
}

fn set_workspace(state: &State<AppState>, ws: Workspace) -> Result<(), String> {
    let mut guard = state
        .0
        .lock()
        .map_err(|_| "workspace lock poisoned".to_string())?;
    *guard = Some(ws);
    Ok(())
}

// ---- workspace ----

/// Open a folder and make it the boundary for every later command.
///
/// The folder is chosen by a NATIVE dialog the backend opens itself. The
/// command used to take a `root` string from the frontend, which meant any
/// JavaScript already running in the trusted webview could name a directory
/// the user never picked and then read and write it through `ide_read` /
/// `ide_save` — defeating the documented property that a rendering compromise
/// cannot reach outside the folder the user chose. The webview now supplies no
/// path at all: it asks for a folder, the user picks one in a native dialog,
/// and only then does the boundary move. A webview that can run code can still
/// *ask* for a folder; it cannot answer.
#[tauri::command]
async fn ide_open_folder(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<TreeNode>, String> {
    use tauri_plugin_dialog::DialogExt;
    let picked = app
        .dialog()
        .file()
        .set_title("Open project folder")
        .blocking_pick_folder();
    let Some(picked) = picked else {
        // the user cancelled: keep whatever workspace is open
        let ws = workspace(&state)?;
        return Ok(fsops::scan(&ws, 8));
    };
    let path = picked.into_path().map_err(|e| e.to_string())?;
    let ws = Workspace::open(&path.to_string_lossy())?;
    let tree = fsops::scan(&ws, 8);
    set_workspace(&state, ws)?;
    Ok(tree)
}

/// Re-walk the open folder (after a build wrote `target/`, or the user
/// created a file outside the IDE).
#[tauri::command]
fn ide_scan(state: State<AppState>) -> Result<Vec<TreeNode>, String> {
    let ws = workspace(&state)?;
    Ok(fsops::scan(&ws, 8))
}

/// The folder currently open, if any — what the explorer roots its tree at
/// when the boot scan comes back empty (an empty folder is a legal project).
#[tauri::command]
fn ide_root(state: State<AppState>) -> Result<String, String> {
    let ws = workspace(&state)?;
    Ok(ws.root().to_string_lossy().into_owned())
}

// ---- files ----

#[tauri::command]
fn ide_read(path: String, state: State<AppState>) -> Result<FileContents, String> {
    fsops::read(&workspace(&state)?, &path)
}

#[tauri::command]
fn ide_save(path: String, text: String, state: State<AppState>) -> Result<(), String> {
    fsops::write(&workspace(&state)?, &path, &text)
}

/// Create a file and hand back the refreshed tree, so the explorer is never
/// one round trip behind the filesystem.
#[tauri::command]
fn ide_new_file(path: String, state: State<AppState>) -> Result<Vec<TreeNode>, String> {
    let ws = workspace(&state)?;
    fsops::create_file(&ws, &path, "")?;
    Ok(fsops::scan(&ws, 8))
}

/// Create a folder; returns the refreshed tree.
#[tauri::command]
fn ide_new_dir(path: String, state: State<AppState>) -> Result<Vec<TreeNode>, String> {
    let ws = workspace(&state)?;
    fsops::create_dir(&ws, &path)?;
    Ok(fsops::scan(&ws, 8))
}

// ---- toolchain ----

#[tauri::command]
fn ide_toolchain() -> ToolchainInfo {
    toolchain::probe()
}

/// `aoxn doctor` — the toolchain's own self-check, driven the same way a
/// user would run it. The status bar's "compiler not found" button and the
/// command palette both land here.
#[tauri::command]
fn ide_doctor(state: State<AppState>) -> Result<ExecResult, String> {
    let ws = workspace(&state)?;
    toolchain::run(
        &toolchain::compiler_command(),
        &toolchain::doctor_args(),
        Some(&ws.root().to_string_lossy()),
    )
}

// ---- packages ----

/// The workspace's `aoxn.json`, read tolerantly (missing is a normal
/// state, broken JSON is reported, never fatal).
#[tauri::command]
fn ide_pkg_manifest(state: State<AppState>) -> Result<PkgManifestInfo, String> {
    let ws = workspace(&state)?;
    Ok(pkg::read_manifest(&ws))
}

/// What the package manager says is installed and what is behind.
///
/// The manifest says what the project ASKS for (`^2`); this says what the
/// resolver DELIVERED (`2.1.0`) and what could be newer. Both halves are the
/// package manager's own `--json` output, read through the same subcommand
/// whitelist as every other package command — `list` and `outdated` are
/// read-only, so this widens what the panel can SHOW without widening what
/// it can DO.
#[tauri::command]
fn ide_pkg_report(state: State<AppState>) -> PkgReport {
    // Not a `Result`: a report that cannot be read is a normal state (no
    // lockfile yet, a registry that did not answer), and the panel shows
    // which half is missing and why.
    match workspace(&state) {
        Ok(ws) => pkg::read_report(&ws),
        Err(e) => PkgReport {
            error: Some(e),
            ..Default::default()
        },
    }
}

/// Run a whitelisted `aoxn pkg` subcommand from the workspace root.
#[tauri::command]
fn ide_pkg_run(
    subcommand: String,
    arg: Option<String>,
    state: State<AppState>,
) -> Result<ExecResult, String> {
    let ws = workspace(&state)?;
    let args: Vec<String> = arg
        .filter(|a| !a.trim().is_empty())
        .map(|a| vec![a])
        .unwrap_or_default();
    pkg::pkg_command(&ws, &subcommand, &args)
}

// ---- run ----

/// Build, check and run all reduce to "run the compiler with these
/// arguments"; keeping them separate gives the UI honest button labels and
/// a place to put a future per-kind flag set.
#[tauri::command]
fn ide_check(path: String, state: State<AppState>) -> Result<ExecResult, String> {
    let ws = workspace(&state)?;
    let path = ws.resolve(&path)?.to_string_lossy().into_owned();
    toolchain::run(&toolchain::compiler_command(), &toolchain::check_args(&path), None)
}

#[tauri::command]
fn ide_build(path: String, state: State<AppState>) -> Result<ExecResult, String> {
    let ws = workspace(&state)?;
    let path = ws.resolve(&path)?.to_string_lossy().into_owned();
    let out = toolchain::artifact_path_prepared(&path)?;
    toolchain::run(
        &toolchain::compiler_command(),
        &toolchain::build_args(&path, &out),
        None,
    )
}

#[tauri::command]
fn ide_run(path: String, state: State<AppState>) -> Result<ExecResult, String> {
    let ws = workspace(&state)?;
    let path = ws.resolve(&path)?.to_string_lossy().into_owned();
    toolchain::run(
        &toolchain::compiler_command(),
        &toolchain::run_args(&path),
        None,
    )
}

/// Build and then execute the artifact. Separate from `ide_run` because
/// `aoxn run` recompiles through the cache, while "run what I just built"
/// must not silently recompile a source the user has since changed.
#[tauri::command]
fn ide_build_and_run(path: String, state: State<AppState>) -> Result<ExecResult, String> {
    let ws = workspace(&state)?;
    let path = ws.resolve(&path)?.to_string_lossy().into_owned();
    let out = toolchain::artifact_path_prepared(&path)?;
    let built = toolchain::run(
        &toolchain::compiler_command(),
        &toolchain::build_args(&path, &out),
        None,
    )?;
    if built.code != 0 {
        return Ok(built);
    }
    let exe = toolchain::run(&out, &[], None)?;
    Ok(ExecResult {
        code: exe.code,
        output: format!("{}{}", built.output, exe.output),
        // The BUILD's diagnostics, not the artifact's: the artifact is a
        // compiled program, and anything it prints belongs in `output`, not
        // in the editor's markers.
        diags: built.diags,
        duration_ms: built.duration_ms + exe.duration_ms,
    })
}

// ---- language service ----

/// The top-level declarations of a file and of everything it imports.
///
/// This is the outline, "go to definition" and the symbol search's one data
/// source, and it is the COMPILER's answer: `aoxn symbols --json` walks the
/// AST the compiler just built. Nothing here parses source text, so a
/// parameter is never mistaken for a declaration and an `extern def` is
/// recognisable as having no body to jump to.
///
/// A file that does not parse comes back as an `Err` carrying the
/// compiler's own diagnostic — the editor shows that in place of an outline,
/// rather than an empty panel that reads like "no declarations".
#[tauri::command]
fn ide_symbols(path: String, state: State<AppState>) -> Result<SymbolTable, String> {
    let ws = workspace(&state)?;
    let path = ws.resolve(&path)?.to_string_lossy().into_owned();
    toolchain::symbols_json(&path)
}

// ---- app ----

/// Best guess at the folder to open on launch.
///
/// Explicit beats implicit: `AOXN_IDE_ROOT`, then a folder named on the
/// command line, then the current directory. The IDE opens with a tree, not
/// with an empty pane, because an empty pane with nothing to click is the
/// worst first impression an editor can make.
fn initial_root() -> String {
    if let Ok(v) = std::env::var("AOXN_IDE_ROOT") {
        if !v.is_empty() {
            return v;
        }
    }
    // A folder named on the command line. Only an argument that actually
    // IS an existing directory is taken, so a stray flag cannot be mistaken
    // for a path.
    for arg in std::env::args().skip(1) {
        if arg.starts_with('-') {
            continue;
        }
        if std::path::Path::new(&arg).is_dir() {
            return arg;
        }
    }
    std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| ".".to_string())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            ide_open_folder,
            ide_scan,
            ide_root,
            ide_read,
            ide_save,
            ide_new_file,
            ide_new_dir,
            ide_toolchain,
            ide_doctor,
            ide_pkg_manifest,
            ide_pkg_report,
            ide_pkg_run,
            ide_check,
            ide_build,
            ide_run,
            ide_build_and_run,
            ide_symbols,
        ])
        .setup(|app| {
            let root = initial_root();
            let state = app.state::<AppState>();
            match Workspace::open(&root) {
                Ok(ws) => {
                    let _ = set_workspace(&state, ws);
                }
                Err(e) => {
                    // Not fatal: the workbench opens with no folder and the
                    // user picks one. A wrong working directory should not
                    // stop the IDE from starting.
                    eprintln!("aoxn-ide: {e}");
                }
            }
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_title("Aoxn IDE");
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the Aoxn IDE");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_root_prefers_the_env_var() {
        let dir = tempfile::tempdir().unwrap();
        let want = dir.path().to_string_lossy().into_owned();
        // SAFETY: no other thread exists while this test body runs.
        unsafe { std::env::set_var("AOXN_IDE_ROOT", &want) };
        assert_eq!(initial_root(), want);
        unsafe { std::env::remove_var("AOXN_IDE_ROOT") };
    }

    #[test]
    fn initial_root_falls_back_to_the_working_directory() {
        unsafe { std::env::remove_var("AOXN_IDE_ROOT") };
        let root = initial_root();
        assert!(
            std::path::Path::new(&root).is_dir(),
            "fell back to '{root}', which is not a directory"
        );
    }
}
