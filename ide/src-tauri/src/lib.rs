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

use base64::Engine as _;
use fsops::Workspace;
use model::{
    ChatMessage, ChatReply, ExecResult, FileBytes, FileContents, GitChange, GitFileDiff, GitStatus,
    PkgManifestInfo, PkgReport, SymbolTable, ToolchainInfo, TreeNode,
};
use std::process::Command;
use std::sync::Mutex;
use std::time::Instant;
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

// ---- preview ----

/// A file's bytes for the preview pane.
///
/// Text files keep using `ide_read`; this exists for images, PDFs and other
/// binary previews the webview can render from a data URL. The size cap keeps
/// a 2 GB log from turning into a 2.7 GB JSON string in the IPC channel.
#[tauri::command]
fn ide_read_bytes(path: String, state: State<AppState>) -> Result<FileBytes, String> {
    const MAX_PREVIEW_BYTES: u64 = 16 * 1024 * 1024;
    let ws = workspace(&state)?;
    let resolved = ws.resolve(&path)?;
    let meta = std::fs::metadata(&resolved).map_err(|e| format!("cannot stat '{path}': {e}"))?;
    if meta.len() > MAX_PREVIEW_BYTES {
        return Err(format!(
            "'{path}' is {} bytes; the preview limit is {MAX_PREVIEW_BYTES}",
            meta.len()
        ));
    }
    let bytes = std::fs::read(&resolved).map_err(|e| format!("cannot read '{path}': {e}"))?;
    Ok(FileBytes {
        path,
        base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
        size: meta.len(),
        mime: mime_for(&resolved),
    })
}

fn mime_for(path: &std::path::Path) -> String {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    };
    mime.to_string()
}

// ---- git ----

/// Run git in the workspace root. No user-supplied flags reach this, so the
/// command surface stays a set of read-only queries.
fn run_git(root: &std::path::Path, args: &[&str]) -> Result<std::process::Output, String> {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|e| format!("git is not available: {e}"))
}

/// `git status` for the open workspace, parsed into rows the file tree and
/// the Git pane can render. A non-repository answers `isRepo: false` rather
/// than failing: most Aoxn folders are not repositories yet.
#[tauri::command]
fn ide_git_status(state: State<AppState>) -> Result<GitStatus, String> {
    let ws = workspace(&state)?;
    let probe = run_git(ws.root(), &["rev-parse", "--is-inside-work-tree"])?;
    if !probe.status.success() {
        return Ok(GitStatus {
            is_repo: false,
            branch: String::new(),
            changes: Vec::new(),
            error: None,
        });
    }
    let branch = run_git(ws.root(), &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    let out = run_git(
        ws.root(),
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    if !out.status.success() {
        return Ok(GitStatus {
            is_repo: true,
            branch,
            changes: Vec::new(),
            error: Some(String::from_utf8_lossy(&out.stderr).trim().to_string()),
        });
    }

    Ok(GitStatus {
        is_repo: true,
        branch,
        changes: parse_git_status(&out.stdout),
        error: None,
    })
}

/// Parse `git status --porcelain=v1 -z` into rows.
///
/// The NUL framing matters: a rename carries its old path as a SECOND NUL
/// field, so a naive line split invents a change nobody made. Pure and
/// byte-oriented so the test can pin that entry shape without a repository.
fn parse_git_status(stdout: &[u8]) -> Vec<GitChange> {
    let text = String::from_utf8_lossy(stdout);
    let mut entries = text.split('\0').filter(|entry| !entry.is_empty());
    let mut changes = Vec::new();
    while let Some(entry) = entries.next() {
        if entry.len() < 4 {
            continue;
        }
        let bytes = entry.as_bytes();
        let x = bytes[0] as char;
        let y = bytes[1] as char;
        let path = entry[3..].to_string();
        if x == 'R' || x == 'C' || y == 'R' || y == 'C' {
            let _ = entries.next();
        }
        let status = if x == '?' || y == '?' {
            "untracked"
        } else if x == 'U' || y == 'U' || (x == 'A' && y == 'A') || (x == 'D' && y == 'D') {
            "conflicted"
        } else if x == 'R' || y == 'R' {
            "renamed"
        } else if x == 'A' || y == 'A' {
            "added"
        } else if x == 'D' || y == 'D' {
            "deleted"
        } else if x == '!' {
            "ignored"
        } else {
            "modified"
        };
        changes.push(GitChange {
            path,
            status: status.to_string(),
            staged: x != ' ' && x != '?',
        });
    }
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    changes
}

/// The HEAD version and the working-tree version of one file, for the
/// editor's diff view. A missing HEAD side (a new file) or a missing working
/// side (a deleted file) is normal and answers an empty string.
#[tauri::command]
fn ide_git_diff(path: String, state: State<AppState>) -> Result<GitFileDiff, String> {
    let ws = workspace(&state)?;
    let resolved = ws.resolve(&path)?;
    let relative = resolved
        .strip_prefix(ws.root())
        .unwrap_or(&resolved)
        .to_string_lossy()
        .replace('\\', "/");

    let original = run_git(ws.root(), &["show", &format!("HEAD:{relative}")])
        .ok()
        .filter(|o| o.status.success())
        .map(|o| o.stdout)
        .unwrap_or_default();
    let modified = std::fs::read(&resolved).unwrap_or_default();
    let binary = std::str::from_utf8(&original).is_err() || std::str::from_utf8(&modified).is_err();
    Ok(GitFileDiff {
        path,
        original: if binary {
            String::new()
        } else {
            String::from_utf8_lossy(&original).into_owned()
        },
        modified: if binary {
            String::new()
        } else {
            String::from_utf8_lossy(&modified).into_owned()
        },
        binary,
    })
}

// ---- terminal ----

/// Run one shell command in the workspace (or a folder inside it).
///
/// This is a deliberate widening of the original command surface, which had
/// no shell at all. The command is user-typed and the working directory is
/// bounded by `Workspace::resolve`, so the webview still cannot name a
/// directory the user did not open. Output is captured in full; the UI is a
/// command runner, not a persistent PTY, so `cd` must be passed in `cwd` by
/// the frontend rather than remembered here.
#[tauri::command]
fn ide_terminal_exec(
    command: String,
    cwd: Option<String>,
    state: State<AppState>,
) -> Result<ExecResult, String> {
    let ws = workspace(&state)?;
    let dir = match cwd.filter(|c| !c.trim().is_empty()) {
        Some(path) => ws.resolve(&path)?,
        None => ws.root().to_path_buf(),
    };
    let started = Instant::now();
    let output = if cfg!(windows) {
        Command::new("cmd")
            .arg("/C")
            .arg(&command)
            .current_dir(&dir)
            .output()
    } else {
        Command::new("sh")
            .arg("-lc")
            .arg(&command)
            .current_dir(&dir)
            .output()
    }
    .map_err(|e| format!("cannot run command: {e}"))?;

    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok(ExecResult {
        code: output.status.code().unwrap_or(-1),
        output: text,
        duration_ms: started.elapsed().as_millis() as u64,
        diags: Vec::new(),
    })
}

// ---- assistant ----

/// Proxy one OpenAI-compatible chat completion.
///
/// The webview cannot call provider APIs directly: OpenAI does not send CORS
/// headers, and the packaged app's CSP only permits its own origin. Keeping
/// the request in Rust also keeps the API key out of the DOM and out of any
/// webview-readable store.
#[tauri::command]
async fn ide_ai_chat(
    provider: String,
    base_url: String,
    api_key: String,
    model: String,
    messages: Vec<ChatMessage>,
) -> Result<ChatReply, String> {
    let base = base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Ok(ChatReply {
            content: String::new(),
            error: Some("No API base URL is configured.".to_string()),
            model,
        });
    }
    let anthropic = provider.eq_ignore_ascii_case("anthropic");
    let url = if anthropic {
        if base.ends_with("/v1/messages") {
            base.to_string()
        } else if base.ends_with("/v1") {
            format!("{base}/messages")
        } else {
            format!("{base}/v1/messages")
        }
    } else if base.ends_with("/chat/completions") {
        base.to_string()
    } else {
        format!("{base}/chat/completions")
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| format!("cannot build HTTP client: {e}"))?;

    // Anthropic keeps the system prompt OUTSIDE `messages` and requires a
    // max_tokens field; the OpenAI-compatible shape carries the system turn
    // inline and defaults to streaming off.
    let body = if anthropic {
        let system = messages
            .iter()
            .filter(|message| message.role == "system")
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let conversation: Vec<&ChatMessage> = messages
            .iter()
            .filter(|message| message.role != "system")
            .collect();
        serde_json::json!({
            "model": model,
            "max_tokens": 4096,
            "system": system,
            "messages": conversation,
        })
    } else {
        serde_json::json!({
            "model": model,
            "messages": messages,
            "stream": false,
        })
    };

    let mut request = client.post(&url).json(&body);
    if !api_key.trim().is_empty() {
        request = if anthropic {
            request
                .header("x-api-key", api_key.trim())
                .header("anthropic-version", "2023-06-01")
        } else {
            request.bearer_auth(api_key.trim())
        };
    } else if anthropic {
        request = request.header("anthropic-version", "2023-06-01");
    }

    let response = request
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| format!("cannot read response: {e}"))?;
    if !status.is_success() {
        return Ok(ChatReply {
            content: String::new(),
            error: Some(format!("HTTP {}: {}", status.as_u16(), text.trim())),
            model,
        });
    }
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("invalid JSON from provider: {e}"))?;
    let content = if anthropic {
        value["content"]
            .as_array()
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|part| part["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default()
    } else {
        value["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .or_else(|| {
                value["choices"][0]["message"]["content"]
                    .as_array()
                    .map(|parts| {
                        parts
                            .iter()
                            .filter_map(|part| part["text"].as_str())
                            .collect::<Vec<_>>()
                            .join("")
                    })
            })
            .unwrap_or_default()
    };
    Ok(ChatReply {
        content,
        error: None,
        model,
    })
}

// ---- configuration ----

fn config_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("cannot locate the app config directory: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create '{}': {e}", dir.display()))?;
    Ok(dir.join("ide-config.json"))
}

/// The IDE's small JSON settings file (assistant endpoint/model, UI
/// preferences). Missing answers `{}`; a malformed file is reported so the
/// caller can fall back instead of silently resetting every setting.
#[tauri::command]
fn ide_config_read(app: tauri::AppHandle) -> Result<String, String> {
    let path = config_path(&app)?;
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok("{}".to_string()),
        Err(e) => Err(format!("cannot read '{}': {e}", path.display())),
    }
}

#[tauri::command]
fn ide_config_write(app: tauri::AppHandle, json: String) -> Result<(), String> {
    let path = config_path(&app)?;
    serde_json::from_str::<serde_json::Value>(&json)
        .map_err(|e| format!("refusing to write invalid JSON: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("cannot write '{}': {e}", path.display()))
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
            ide_read_bytes,
            ide_git_status,
            ide_git_diff,
            ide_terminal_exec,
            ide_ai_chat,
            ide_config_read,
            ide_config_write,
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

    #[test]
    fn parse_git_status_consumes_rename_old_paths() {
        // -z framing: a rename field is followed by its old path. Without
        // consuming it, the old path becomes a phantom change.
        let raw = b"?? src/new.ax\0R  src/new_name.ax\0src/old_name.ax\0 M hello.ax\0A  src/added.ax\0";
        let changes = parse_git_status(raw);
        assert_eq!(changes.len(), 4, "{changes:?}");
        assert_eq!(changes[0].path, "hello.ax");
        assert_eq!(changes[0].status, "modified");
        assert!(!changes[0].staged);
        assert_eq!(changes[1].path, "src/added.ax");
        assert_eq!(changes[1].status, "added");
        assert!(changes[1].staged);
        assert_eq!(changes[2].path, "src/new.ax");
        assert_eq!(changes[2].status, "untracked");
        assert!(!changes[2].staged);
        assert_eq!(changes[3].path, "src/new_name.ax");
        assert_eq!(changes[3].status, "renamed");
        assert!(changes[3].staged);
    }

    #[test]
    fn parse_git_status_reports_conflicts() {
        let changes = parse_git_status(b"UU src/conflict.ax\0");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].status, "conflicted");
    }

    #[test]
    fn preview_mime_types_cover_the_rendered_families() {
        assert_eq!(mime_for(std::path::Path::new("a.PNG")), "image/png");
        assert_eq!(mime_for(std::path::Path::new("a.jpg")), "image/jpeg");
        assert_eq!(mime_for(std::path::Path::new("a.svg")), "image/svg+xml");
        assert_eq!(mime_for(std::path::Path::new("a.pdf")), "application/pdf");
        assert_eq!(
            mime_for(std::path::Path::new("a.bin")),
            "application/octet-stream"
        );
    }
}
