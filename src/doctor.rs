//! `aoxn doctor` — verify that a *installed* toolchain actually works.
//!
//! A compiler that "installs" in one click still has three moving parts that
//! can be missing on the target machine: the driver binary, the stdlib
//! sources it resolves `import "stdlib"` against, and the C toolchain
//! (clang) it shells out to. `doctor` reports all three, then proves it by
//! compiling and running a one-line program that imports the stdlib. Exit
//! code is 0 when everything is usable, 1 otherwise, so the installer
//! scripts can gate on it.

use std::path::{Path, PathBuf};
use std::process::Command;

use aoxn::paths;

/// One line of the report: a label, a value, and an optional note.
struct Row {
    label: &'static str,
    value: String,
    ok: Option<bool>, // None = informational, no pass/fail marker
}

pub fn cmd(args: &[String]) -> i32 {
    let json = args.iter().any(|a| a == "--json");
    let smoke_enabled = !args.iter().any(|a| a == "--no-smoke");
    for a in args {
        if a != "--json" && a != "--no-smoke" {
            eprintln!("error: unknown option '{a}' for `Aoxn doctor` (try --json / --no-smoke)");
            return 2;
        }
    }

    let mut rows: Vec<Row> = Vec::new();
    let mut problems: Vec<String> = Vec::new();

    rows.push(row_info("version", env!("CARGO_PKG_VERSION").to_string()));
    rows.push(row_info(
        "platform",
        format!(
            "{} {} (target_os() = {})",
            std::env::consts::OS,
            std::env::consts::ARCH,
            aoxn::platform::target_os_name()
        ),
    ));

    let exe = std::env::current_exe().ok();
    rows.push(row_info(
        "executable",
        exe.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "unknown".into()),
    ));

    // install root + stdlib
    let root = paths::install_root();
    rows.push(row(
        "install root",
        root.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| {
            "not an install directory (running from a source checkout)".into()
        }),
        if root.is_some() { Some(true) } else { None },
    ));

    let stdlib = paths::stdlib_dir();
    let missing: Vec<String> = paths::stdlib_status(stdlib.as_deref())
        .into_iter()
        .filter(|(_, present)| !present)
        .map(|(name, _)| name)
        .collect();
    if stdlib.is_some() && missing.is_empty() {
        rows.push(row("stdlib", stdlib.as_ref().unwrap().display().to_string(), Some(true)));
    } else {
        let value = match &stdlib {
            Some(dir) => format!("{} (missing: {})", dir.display(), missing.join(", ")),
            None => "not found — set AOXN_STDLIB or AOXN_HOME".into(),
        };
        let ok = stdlib.is_some();
        if !ok {
            problems.push("standard library sources not found (AOXN_STDLIB / AOXN_HOME)".into());
        } else {
            problems.push(format!("stdlib incomplete: missing {}", missing.join(", ")));
        }
        rows.push(row("stdlib", value, Some(ok)));
    }

    let examples = paths::examples_dir();
    rows.push(row_info(
        "examples",
        match &examples {
            Some(dir) => format!("{} ({} .ax files)", dir.display(), count_ax(dir)),
            None => "not bundled with this install".into(),
        },
    ));

    // C toolchain
    let clang = aoxn::find_clang();
    match &clang {
        Some(path) => {
            let version = clang_version(path);
            rows.push(row("clang", format!("{}  {}", path.display(), version), Some(true)));
        }
        None => {
            rows.push(row(
                "clang",
                "not found — install a C toolchain (see docs/install.md) or set AOXN_CLANG".into(),
                Some(false),
            ));
            problems.push(
                "clang not found: the C backend needs it to compile and link native code".into(),
            );
        }
    }

    // end-to-end proof: import the stdlib by name, compile, run
    let mut smoke: Option<(bool, String)> = None;
    if smoke_enabled && clang.is_some() {
        smoke = Some(run_smoke());
        match &smoke {
            Some((true, out)) => rows.push(row("smoke test", format!("ok  ({})", out.trim()), Some(true))),
            Some((false, out)) => {
                rows.push(row("smoke test", format!("FAILED  {}", out.trim()), Some(false)));
                problems.push(format!("smoke test failed: {}", out.trim()));
            }
            None => {}
        }
    } else if smoke_enabled {
        rows.push(row_info("smoke test", "skipped (no clang)".into()));
    } else {
        rows.push(row_info("smoke test", "skipped (--no-smoke)".into()));
    }

    let cache = cache_dir();
    rows.push(row_info("cache dir", cache.display().to_string()));

    let ok = problems.is_empty();
    if json {
        println!("{}", to_json(&rows, &problems, &smoke));
    } else {
        println!("Aoxn {} doctor", env!("CARGO_PKG_VERSION"));
        for r in &rows {
            let mark = match r.ok {
                Some(true) => "ok  ",
                Some(false) => "FAIL",
                None => "    ",
            };
            println!("  [{mark}] {:<12} {}", r.label, r.value);
        }
        println!();
        if ok {
            println!("status: ok — `aoxn run <file.ax>` is ready to use.");
        } else {
            println!("status: problems found:");
            for p in &problems {
                println!("  - {p}");
            }
            println!("\nSee docs/install.md, or run this again after fixing the items above.");
        }
    }
    if ok {
        0
    } else {
        1
    }
}

fn row_info(label: &'static str, value: String) -> Row {
    Row { label, value, ok: None }
}

fn row(label: &'static str, value: String, ok: Option<bool>) -> Row {
    Row { label, value, ok }
}

fn count_ax(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().extension().map(|x| x == "ax").unwrap_or(false))
                .count()
        })
        .unwrap_or(0)
}

/// First line of `clang --version`, trimmed; `""` when it cannot be run.
fn clang_version(clang: &Path) -> String {
    Command::new(clang)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or("").trim().to_string())
        .unwrap_or_else(|| "(version query failed)".into())
}

/// Compile and run a one-line program that imports the stdlib by name.
/// Returns `None` only when the scratch file cannot be written.
fn run_smoke() -> (bool, String) {
    let dir = cache_dir().join("doctor");
    if std::fs::create_dir_all(&dir).is_err() {
        return (false, "cannot create the doctor scratch directory".into());
    }
    let src = dir.join("smoke.ax");
    let exe = dir.join(format!("smoke{}", exe_suffix()));
    let source = "# generated by `aoxn doctor`\nimport * from \"stdlib\"\n\ndef main() -> int:\n    print(\"aoxn-doctor-ok\")\n    return 0\n";
    if std::fs::write(&src, source).is_err() {
        return (false, "cannot write the smoke-test source".into());
    }
    let _ = std::fs::remove_file(&exe);
    // Windows AV / Smart App Control routinely holds a freshly written object
    // or executable for a few hundred milliseconds; the backend already
    // retries the LINK step, but the COMPILE step can be killed outright.
    // One retry turns that environment race into a non-event.
    let mut last = String::new();
    for attempt in 0..2 {
        let _ = std::fs::remove_file(&exe);
        match aoxn::build_paths_opts_lvl(
            &[src.display().to_string()],
            &exe,
            1, // -O1: the doctor checks wiring, not performance
            &[],
            &[],
        ) {
            Ok(()) => last.clear(),
            Err(diags) => {
                last = diags.first().map(|d| d.message.clone()).unwrap_or_default();
                if attempt == 0 {
                    std::thread::sleep(std::time::Duration::from_millis(800));
                }
            }
        }
        if last.is_empty() {
            break;
        }
    }
    if !last.is_empty() {
        return (false, last);
    }
    let out = Command::new(&exe).output();
    let _ = std::fs::remove_file(&exe);
    match out {
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if o.status.success() && text.contains("aoxn-doctor-ok") {
                (true, text)
            } else {
                (false, format!("exit {:?}, output {:?}", o.status.code(), text))
            }
        }
        Err(e) => (false, format!("cannot run the built program: {}", aoxn::io_reason(&e))),
    }
}

fn exe_suffix() -> &'static str {
    if cfg!(windows) {
        ".exe"
    } else {
        ""
    }
}

/// Same default as the build cache (`AOXN_CACHE_DIR` > `<cwd>/target/cache`).
fn cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("AOXN_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join("target").join("cache")
}

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

fn to_json(rows: &[Row], problems: &[String], smoke: &Option<(bool, String)>) -> String {
    let mut out = String::from("{\"ok\":");
    out.push_str(if problems.is_empty() { "true" } else { "false" });
    out.push_str(",\"checks\":[");
    for (i, r) in rows.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"name\":\"{}\",\"value\":\"{}\",\"ok\":{}}}",
            esc(r.label),
            esc(&r.value),
            match r.ok {
                Some(b) => b.to_string(),
                None => "null".to_string(),
            }
        ));
    }
    out.push_str("],\"smoke\":");
    match smoke {
        Some((ok, text)) => out.push_str(&format!(
            "{{\"ok\":{},\"output\":\"{}\"}}",
            ok,
            esc(text)
        )),
        None => out.push_str("null"),
    }
    out.push_str(",\"problems\":[");
    for (i, p) in problems.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!("\"{}\"", esc(p)));
    }
    out.push_str("]}");
    out
}