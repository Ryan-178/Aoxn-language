//! `aoxn-setup.exe` — the single-file Aoxn installer for Windows.
//!
//! One executable carries the whole toolchain: the compiler, the standard
//! library, the UI toolkit and the examples. `dist/package.ps1` builds this
//! stub and appends the payload to it, so the file the user double-clicks
//! installs everything — no second file, no archive to unpack by hand.
//!
//! # Payload format (v1)
//!
//! ```text
//! [ PE image ][ payload ][ u64 payload_len ][ 8-byte magic "AOXNSETUP" ]
//! ```
//!
//! Little-endian throughout. The payload is *stored*, not deflated, so the
//! stub needs no decompressor and the zero-dependency rule holds:
//!
//! ```text
//! payload := u32 entry_count, entry*
//! entry   := u16 name_len, name bytes (UTF-8, '/' separators),
//!            u64 size, u8 executable, size bytes
//! ```
//!
//! # What it does
//!
//! 1. unpacks into `%LOCALAPPDATA%\aoxn` (or `-Prefix <dir>` / `$AOXN_HOME`),
//! 2. adds `<prefix>\bin` to the user PATH,
//! 3. makes sure a C toolchain exists — the compiler lowers to C and shells
//!    out to clang, so this is a hard requirement, not a nicety,
//! 4. runs `aoxn doctor`, which compiles and executes a test program,
//! 5. exits non-zero unless the toolchain actually works.
//!
//! Flags: `-Prefix <dir>`, `-NoClang`, `-Quiet`, `-Console`, `-Uninstall`, `-?`.

#![cfg_attr(not(windows), allow(unused))]
#![windows_subsystem = "windows"]

use std::fs;
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Sender};

#[cfg(windows)]
mod ui;

/// What the user asked for. The window fills this in from its checkboxes
/// BEFORE the worker starts, which is why the worker is created lazily
/// rather than alongside the window (see `start_install`).
#[derive(Clone, Debug)]
pub struct InstallOptions {
    pub prefix: PathBuf,
    /// put `<prefix>/bin` on the user PATH
    pub add_to_path: bool,
    /// install LLVM via winget when no clang is found
    pub install_clang: bool,
}

// Spelled with an escape rather than a literal NUL: the NUL byte made
// this file read as binary by grep/rg and anything else that sniffs
// encoding.
const MAGIC: &[u8; 8] = b"AOXNSFX\x00";
const TRAILER_LEN: u64 = 16; // u64 length + 8-byte magic
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    let opts = match Options::parse() {
        Ok(o) => o,
        Err(message) => {
            report(&format!("error: {message}"));
            return ExitCode::from(2);
        }
    };
    if opts.help {
        print_help();
        return ExitCode::SUCCESS;
    }
    let prefix = install_prefix(opts.prefix.as_deref());

    if opts.uninstall {
        return match uninstall(&prefix) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                report(&format!("error: {e}"));
                ExitCode::from(2)
            }
        };
    }

    let (tx, rx) = mpsc::channel();
    let cancel: &'static AtomicBool = Box::leak(Box::new(AtomicBool::new(false)));

    let defaults = InstallOptions {
        prefix: prefix.clone(),
        add_to_path: true,
        install_clang: opts.install_clang,
    };

    // Console mode has nobody to click a button, so it starts at once. The
    // window must NOT: before this change the worker was spawned next to the
    // channel, so the toolchain was unpacked into the user's disk while the
    // window was still being created, with nothing on screen to explain it.
    // "Install Now" has to be a decision the user actually makes.
    if opts.console || !cfg!(windows) {
        // Spawn directly rather than through the window's closure. That
        // closure borrows `tx` and outlives this block; while it is alive the
        // channel never closes, and `rx.recv()` below blocks forever after the
        // worker finishes -- the install completes, prints its last line, and
        // the process never exits. Dropping our own sender is what lets
        // recv() observe the close.
        let worker = start_worker(defaults.clone(), &tx, cancel);
        drop(tx);
        let mut ok = true;
        while let Ok(progress) = rx.recv() {
            match progress {
                InstallProgress::Log(line) => println!("  {line}"),
                InstallProgress::Step { text, .. } => println!("==> {text}"),
                InstallProgress::Done { ok: done, message } => {
                    ok = done;
                    if !message.is_empty() {
                        eprintln!("{message}");
                    }
                }
            }
        }
        let _ = worker.join();
        if ok {
            summarize(&prefix);
        }
        return if ok { ExitCode::SUCCESS } else { ExitCode::from(1) };
    }

    // The window calls this with the options its checkboxes produced, not
    // with the defaults: that is the whole point of asking the user. The
    // wizard lives in `mod ui`, which is Windows-only — the cfg blocks keep
    // it out of the non-Windows build entirely (the console branch above
    // catches every other platform, since `!cfg!(windows)` is true there).
    #[cfg(windows)]
    {
        let start_install = move |opts: InstallOptions| start_worker(opts, &tx, cancel);
        let code = ui::wizard(VERSION, &defaults, rx, cancel, Box::new(start_install));
        return ExitCode::from(code as u8);
    }
    #[cfg(not(windows))]
    {
        // exists so the `if` above keeps a statement after it on Linux,
        // where the wizard block is compiled out; the console branch
        // already returned
        unreachable!("non-Windows builds take the console path")
    }
}

/// Spawn the installation on a worker thread and report the outcome. Shared by
/// the console path and the window's Install button, so both drive exactly the
/// same sequence.
fn start_worker(
    options: InstallOptions,
    tx: &Sender<InstallProgress>,
    cancel: &'static AtomicBool,
) -> std::thread::JoinHandle<()> {
    let tx = tx.clone();
    std::thread::spawn(move || {
        let ok = install(&options, &tx, cancel);
        let _ = tx.send(InstallProgress::Done {
            ok,
            message: if ok {
                String::new()
            } else {
                String::from("installation failed - see the messages above")
            },
        });
    })
}

// ---- progress reporting -----------------------------------------------------
/// The wizard's progress bar has one tick per step.
#[derive(Clone, Copy)]
pub enum InstallStep {
    Prepare,
    Unpack,
    Path,
    Clang,
    Doctor,
    Finish,
}

impl InstallStep {
    pub const COUNT: u32 = 6;
}

pub enum InstallProgress {
    Log(String),
    Step { done: u32, total: u32, text: String },
    Done { ok: bool, message: String },
}

/// The installation itself. Runs on a worker thread; `send` may fail only
/// after the window closed, which is not an error.
fn install(opts: &InstallOptions, tx: &Sender<InstallProgress>, cancel: &AtomicBool) -> bool {
    let prefix = &opts.prefix;
    let send = |progress: InstallProgress| {
        let _ = tx.send(progress);
    };
    let step = |n: u32, text: String| InstallProgress::Step { done: n, total: InstallStep::COUNT, text };

    send(step(1, "Preparing".into()));
    if let Err(e) = read_payload() {
        send(InstallProgress::Log(format!("error: {e}")));
        return false;
    }

    send(step(2, format!("Unpacking the toolchain into {}", prefix.display())));
    if cancel.load(std::sync::atomic::Ordering::SeqCst) {
        return false;
    }
    let count = match unpack(prefix, tx) {
        Ok(n) => n,
        Err(e) => {
            send(InstallProgress::Log(format!("error: {e}")));
            return false;
        }
    };
    send(InstallProgress::Log(format!("{count} files installed")));

    let aoxn = prefix.join("bin").join("aoxn.exe");
    if !aoxn.is_file() {
        send(InstallProgress::Log(format!(
            "error: the payload did not contain bin/aoxn.exe ({})",
            aoxn.display()
        )));
        return false;
    }

    // PATH is a choice, not a step: the window may have unticked it, and
    // editing the user's environment is not ours to decide silently.
    if opts.add_to_path {
        send(step(3, "Adding aoxn to your PATH".into()));
        match add_to_path(&prefix.join("bin"), tx) {
            Ok(()) => {}
            Err(e) => {
                send(InstallProgress::Log(format!("warning: {e}")));
            }
        }
    } else {
        send(step(3, String::from("Leaving your PATH unchanged")));
    }

    send(step(4, "Checking the C toolchain".into()));
    let mut clang = find_clang(prefix);
    // Opt-in only. Downloading LLVM means a winget fetch that can take many
    // minutes and pop its own window; doing that unasked, behind a progress bar
    // that says nothing about it, is how an install comes to look hung.
    // `-InstallClang` still does it deliberately.
    if clang.is_none() && opts.install_clang {
        send(InstallProgress::Log(
            "clang not found — installing LLVM (a window may pop up; let it finish)".into(),
        ));
        install_llvm();
        clang = find_clang(prefix);
        if clang.is_some() {
            // the installer put it in the machine PATH, which this process
            // does not see yet
            prepend_machine_path(r"C:\Program Files\LLVM\bin");
        }
    }
    match &clang {
        Some(path) => send(InstallProgress::Log(format!("clang: {}", path.display()))),
        None => send(InstallProgress::Log(String::from(
            "clang not found — install LLVM (https://releases.llvm.org) or the MSVC Build Tools,\n           then run 'aoxn doctor' again",
        ))),
    }

    if cancel.load(std::sync::atomic::Ordering::SeqCst) {
        return false;
    }

    send(step(5, "Checking the installation with 'aoxn doctor'".into()));
    let mut ok = run_doctor(&aoxn, tx);
    // A SUCCESSFUL doctor is not proof the binary survived. Windows Smart
    // App Control and Defender routinely quarantine a freshly written,
    // unsigned executable AFTER its first run, and the payload unpacked a
    // moment ago is exactly that. Without this check the installer reports
    // Done and every later invocation (the CI smoke test, the user's first
    // command) fails with "not recognized as a name of a cmdlet", which
    // points at the shell rather than at what actually happened.
    if ok {
        ok = confirm_present(&aoxn, tx);
    }
    if ok {
        send(step(6, "Done".into()));
    }
    ok
}

/// Wait briefly for a just-run binary to still be there.
///
/// Antivirus holds and removes files asynchronously, so a deletion can land
/// a moment after the process exits. A short retry turns a lost race into a
/// slow install instead of a broken one; a genuinely quarantined binary is
/// still reported, with the reason named and the remedy given.
fn confirm_present(aoxn: &Path, tx: &Sender<InstallProgress>) -> bool {
    confirm_present_with(aoxn, tx, 8, std::time::Duration::from_millis(750))
}

/// The body of confirm_present, with the retry budget as parameters.
///
/// Split out so the tests can prove both directions of the race in
/// milliseconds; production passes the real budget.
fn confirm_present_with(
    aoxn: &Path,
    tx: &Sender<InstallProgress>,
    attempts: u32,
    delay: std::time::Duration,
) -> bool {
    for attempt in 0..attempts {
        if aoxn.is_file() {
            return true;
        }
        // Warn once, not on every poll: the log is a transcript the user
        // reads, not a progress meter.
        if attempt == attempts / 4 {
            let _ = tx.send(InstallProgress::Log(String::from(
                "warning: bin/aoxn.exe is gone after its first run, waiting for antivirus to settle",
            )));
        }
        std::thread::sleep(delay);
    }
    let _ = tx.send(InstallProgress::Log(String::from(
        concat!(
            "error: bin/aoxn.exe was removed right after it ran. This is antivirus or ",
            "Smart App Control quarantining a newly installed, unsigned binary.\n",
            "The install is NOT usable yet: exclude the install folder (default ",
            "%LOCALAPPDATA%\\aoxn) from real-time scanning, or restore the file, then ",
            "run 'aoxn doctor' again.",
        ),
    )));
    false
}

fn run_doctor(aoxn: &Path, tx: &Sender<InstallProgress>) -> bool {
    let mut child = match Command::new(aoxn).arg("doctor").stdout(std::process::Stdio::piped()).spawn() {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.send(InstallProgress::Log(format!("error: cannot run aoxn doctor: {e}")));
            return false;
        }
    };
    // forward the report into the log pane as it arrives
    if let Some(out) = child.stdout.take() {
        let reader = std::io::BufReader::new(out);
        for line in reader.lines().map_while(Result::ok) {
            let _ = tx.send(InstallProgress::Log(line));
        }
    }
    matches!(child.wait(), Ok(status) if status.success())
}

// ---- payload ----------------------------------------------------------------
fn read_payload() -> Result<Vec<u8>, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot locate the setup executable: {e}"))?;
    let file = fs::File::open(&exe).map_err(|e| format!("cannot open {}: {e}", exe.display()))?;
    let len = file
        .metadata()
        .map_err(|e| format!("cannot stat {}: {e}", exe.display()))?
        .len();
    if len < TRAILER_LEN {
        return Err("this executable carries no Aoxn payload".into());
    }
    let mut trailer = [0u8; TRAILER_LEN as usize];
    read_at(&file, len - TRAILER_LEN, &mut trailer)?;
    if &trailer[8..16] != MAGIC {
        return Err("this executable carries no Aoxn payload — build it with dist\\package.ps1".into());
    }
    let payload_len = u64::from_le_bytes(trailer[0..8].try_into().unwrap());
    if payload_len + TRAILER_LEN > len {
        return Err("the payload trailer does not match the file size (corrupt setup?)".into());
    }
    let mut payload = vec![0u8; payload_len as usize];
    read_at(&file, len - TRAILER_LEN - payload_len, &mut payload)?;
    Ok(payload)
}

fn read_at(file: &fs::File, offset: u64, buf: &mut [u8]) -> Result<(), String> {
    use std::io::Seek;
    let mut f = file
        .try_clone()
        .map_err(|e| format!("cannot re-open the setup executable: {e}"))?;
    f.seek(std::io::SeekFrom::Start(offset))
        .map_err(|e| format!("cannot seek in the setup executable: {e}"))?;
    f.read_exact(buf).map_err(|e| format!("cannot read the setup payload: {e}"))
}

/// Unpack the stored archive into `prefix`; returns the number of files.
fn unpack(prefix: &Path, tx: &Sender<InstallProgress>) -> Result<usize, String> {
    let payload = read_payload()?;
    if payload.len() < 4 {
        return Err("payload is truncated".into());
    }
    let count = u32::from_le_bytes(payload[0..4].try_into().unwrap()) as usize;
    let mut at = 4usize;
    let mut written = 0usize;
    for index in 0..count {
        let need = |at: usize, n: usize| -> Result<(), String> {
            if at + n > payload.len() {
                Err(format!("payload is truncated at entry {index}"))
            } else {
                Ok(())
            }
        };
        need(at, 2)?;
        let name_len = u16::from_le_bytes(payload[at..at + 2].try_into().unwrap()) as usize;
        at += 2;
        need(at, name_len)?;
        let name = String::from_utf8_lossy(&payload[at..at + name_len]).to_string();
        at += name_len;
        need(at, 9)?;
        let size = u64::from_le_bytes(payload[at..at + 8].try_into().unwrap()) as usize;
        // the executable flag is part of the format; Windows has no bit to set
        let _executable = payload[at + 8] != 0;
        at += 9;
        need(at, size)?;
        // a hand-edited payload must not be able to write outside the target
        if name.is_empty() || name.starts_with('/') || name.contains("..") || name.contains('\\') || name.contains(':') {
            return Err(format!("payload entry {index} has an unsafe path '{name}'"));
        }
        let target = prefix.join(&name);
        if let Some(dir) = target.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        }
        fs::write(&target, &payload[at..at + size])
            .map_err(|e| format!("cannot write {}: {e}", target.display()))?;
        at += size;
        written += 1;
        if written % 8 == 0 {
            let _ = tx.send(InstallProgress::Log(format!("  … {written}/{count} files")));
        }
    }
    Ok(written)
}

// ---- PATH, clang, uninstall -------------------------------------------------
fn add_to_path(bin_dir: &Path, tx: &Sender<InstallProgress>) -> Result<(), String> {
    let bin = bin_dir.display().to_string();
    let current = read_user_path();
    if current.split(';').any(|p| p.trim().trim_end_matches('\\').eq_ignore_ascii_case(&bin)) {
        let _ = tx.send(InstallProgress::Log(format!("already on PATH: {bin}")));
        return Ok(());
    }
    let updated = if current.trim().is_empty() {
        bin.clone()
    } else {
        format!("{current};{bin}")
    };
    write_user_path(&updated)?;
    let _ = tx.send(InstallProgress::Log(format!("added to PATH: {bin}")));
    let _ = tx.send(InstallProgress::Log(String::from(
        "open a new terminal so it picks up the updated PATH",
    )));
    Ok(())
}

/// The user PATH from the registry — the same store
/// `[Environment]::SetEnvironmentVariable(..., "User")` writes. `reg.exe`
/// keeps this free of dependencies and of `advapi32` bindings.
fn read_user_path() -> String {
    reg_query("Path").unwrap_or_default()
}

fn write_user_path(value: &str) -> Result<(), String> {
    reg_set("Path", value)
}

#[cfg(windows)]
fn reg_query(name: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = Command::new("reg")
        .args(["query", r"HKCU\Environment", "/v", name])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(key), Some(_kind), Some(value)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        if key.eq_ignore_ascii_case(name) {
            return Some(value.to_string());
        }
    }
    None
}

#[cfg(windows)]
fn reg_set(name: &str, value: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let status = Command::new("reg")
        .args([
            "add",
            r"HKCU\Environment",
            "/v",
            name,
            "/t",
            "REG_EXPAND_SZ",
            "/d",
            value,
            "/f",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(std::process::Stdio::null())
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("reg add exited with {status}"))
    }
}

#[cfg(not(windows))]
fn reg_query(_name: &str) -> Option<String> {
    None
}

#[cfg(not(windows))]
fn reg_set(_name: &str, _value: &str) -> Result<(), String> {
    Err("PATH editing is implemented for Windows only".into())
}

/// clang, in the same order as `aoxn::find_clang`.
fn find_clang(prefix: &Path) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("AOXN_CLANG").map(PathBuf::from) {
        if p.is_file() {
            return Some(p);
        }
    }
    let mut candidates: Vec<PathBuf> = vec![prefix.join("toolchain").join("bin").join("clang.exe")];
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            candidates.push(dir.join("clang.exe"));
        }
    }
    for dir in [r"C:\Program Files\LLVM\bin", r"C:\Program Files (x86)\LLVM\bin"] {
        candidates.push(Path::new(dir).join("clang.exe"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

#[cfg(windows)]
fn install_llvm() {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = Command::new("winget")
        .args([
            "install",
            "--id",
            "LLVM.LLVM",
            "--exact",
            "--accept-source-agreements",
            "--accept-package-agreements",
            "--silent",
            "--disable-interactivity",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

#[cfg(not(windows))]
fn install_llvm() {}

/// LLVM installs into the machine PATH, which the running setup process does
/// not see; add it so the self-check can find clang.
#[cfg(windows)]
fn prepend_machine_path(dir: &str) {
    if let Ok(existing) = std::env::var("PATH") {
        if existing.split(';').any(|p| p.eq_ignore_ascii_case(dir)) {
            return;
        }
    }
    let mut path = std::ffi::OsString::from(dir);
    path.push(";");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    std::env::set_var("PATH", path);
}

#[cfg(not(windows))]
fn prepend_machine_path(_dir: &str) {}

fn uninstall(prefix: &Path) -> Result<(), String> {
    let bin = prefix.join("bin").display().to_string();
    let current = read_user_path();
    let parts: Vec<&str> = current.split(';').filter(|p| !p.trim().is_empty()).collect();
    let kept: Vec<&str> = parts
        .iter()
        .copied()
        .filter(|p| !p.trim().trim_end_matches('\\').eq_ignore_ascii_case(&bin))
        .collect();
    if kept.len() != parts.len() {
        let _ = write_user_path(&kept.join(";"));
        report(&format!("removed {bin} from your PATH"));
    }
    if prefix.exists() {
        fs::remove_dir_all(prefix).map_err(|e| format!("cannot remove {}: {e}", prefix.display()))?;
    }
    report(&format!("removed {}", prefix.display()));
    Ok(())
}

// ---- command line -----------------------------------------------------------
struct Options {
    prefix: Option<String>,
    install_clang: bool,
    console: bool,
    uninstall: bool,
    help: bool,
}

impl Options {
    fn parse() -> Result<Options, String> {
        let mut opts = Options {
            prefix: None,
            install_clang: false,
            console: false,
            uninstall: false,
            help: false,
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-Prefix" | "--prefix" => {
                    opts.prefix = Some(args.next().ok_or("-Prefix needs a directory")?)
                }
                "-NoClang" | "--no-clang" => opts.install_clang = false,
                "-InstallClang" | "--install-clang" => opts.install_clang = true,
                "-Quiet" | "-q" => opts.console = true,
                "-Console" | "-c" => opts.console = true,
                "-Uninstall" | "--uninstall" => opts.uninstall = true,
                "-?" | "-h" | "--help" => opts.help = true,
                other => return Err(format!("unknown option '{other}' (try -?)")),
            }
        }
        Ok(opts)
    }
}

fn install_prefix(explicit: Option<&str>) -> PathBuf {
    if let Some(p) = explicit {
        return PathBuf::from(p);
    }
    if let Some(home) = std::env::var_os("AOXN_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(home);
    }
    let local = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    local.join("aoxn")
}

fn summarize(prefix: &Path) {
    let aoxn = prefix.join("bin").join("aoxn.exe");
    let _ = Command::new(&aoxn).arg("version").status();
    println!();
    println!("Aoxn is installed in {}", prefix.display());
    println!("try it:");
    println!("    aoxn run {}\\examples\\hello.ax", prefix.display());
    println!("    aoxn doctor");
    println!("(open a NEW terminal so it picks up the updated PATH)");
}

fn print_help() {
    println!(
        "Aoxn Setup {VERSION}\n\n\
         Installs the Aoxn toolchain: compiler, standard library, UI toolkit, examples.\n\n\
         USAGE:\n  \
         Aoxn-Setup.exe                  install into %LOCALAPPDATA%\\aoxn (with a window)\n  \
         Aoxn-Setup.exe -Console         same, but print to the console (scripts, CI)\n  \
         Aoxn-Setup.exe -Uninstall       remove the install and the PATH entry\n\n\
         OPTIONS:\n  \
         -Prefix <dir>    install root (default: $AOXN_HOME, else %LOCALAPPDATA%\\aoxn)\n  \
         -InstallClang    download LLVM via winget when no clang is found\n  \
         -NoClang         never download anything (the default)\n  \
         -Console, -Quiet no window; write progress to stdout\n  \
         -?               this help\n\n\
         This installer downloads NOTHING by default: it unpacks the toolchain, adds\n\
         it to your PATH, and runs 'aoxn doctor'. Aoxn needs two things already on\n\
         the machine - the MSVC Build Tools (for linking) and clang (to compile the C\n\
         the backend emits) - and doctor reports both, with the remedy."
    );
}

/// A GUI-subsystem process has no console of its own; when output is piped
/// (CI, `-Console`) this still writes there.
fn report(message: &str) {
    let _ = writeln!(std::io::stdout(), "{message}");
    let _ = std::io::stdout().flush();
}
#[cfg(test)]
mod tests {
    use super::*;

    /// The check must PASS when the binary is still there, or every install
    /// on a healthy machine would be reported as broken.
    #[test]
    fn confirm_present_passes_for_a_file_that_is_there() {
        let dir = std::env::temp_dir().join(format!("aoxn-setup-cp-ok-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let exe = dir.join("aoxn.exe");
        std::fs::write(&exe, b"MZ").expect("write stub");
        let (tx, _rx) = mpsc::channel();
        assert!(confirm_present(&exe, &tx));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The whole point of the function: a binary that is NOT there must be
    /// reported, never waved through. The path is one that cannot exist, so
    /// this does not depend on reproducing the antivirus race itself.
    #[test]
    fn confirm_present_reports_a_binary_that_is_not_there() {
        let dir = std::env::temp_dir().join(format!("aoxn-setup-cp-gone-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let exe = dir.join("aoxn.exe"); // never created
        let (tx, rx) = mpsc::channel();
        assert!(!confirm_present_with(&exe, &tx, 4, std::time::Duration::from_millis(1)));
        // The failure must NAME the cause and the remedy; a bare `false`
        // sends the reader looking for a packaging bug instead.
        let logged: Vec<String> = rx
            .try_iter()
            .filter_map(|e| match e {
                InstallProgress::Log(m) => Some(m),
                _ => None,
            })
            .collect();
        assert!(
            logged.iter().any(|l| l.contains("antivirus") && l.contains("NOT usable")),
            "the message must name the cause and the remedy, got {logged:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A binary that APPEARS during the wait must be accepted: antivirus
    /// holding a file for a few hundred milliseconds is the same race in the
    /// other direction, and treating it as a failure would be just as wrong.
    #[test]
    fn confirm_present_accepts_a_binary_that_appears_during_the_wait() {
        let dir = std::env::temp_dir().join(format!("aoxn-setup-cp-late-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let exe = dir.join("aoxn.exe");
        // Stand in for the antivirus hold: the file lands while the check is
        // already retrying.
        let writer = {
            let path = exe.clone();
            let dir2 = dir.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(300));
                let _ = std::fs::write(&path, b"MZ");
                let _ = std::fs::create_dir_all(&dir2);
            })
        };
        let (tx, _rx) = mpsc::channel();
        assert!(confirm_present(&exe, &tx));
        writer.join().expect("writer thread");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
