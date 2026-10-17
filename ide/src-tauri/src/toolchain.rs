//! Driving the Aoxn toolchain: finding the compiler, and running it.
//!
//! The IDE does not reimplement any part of the compiler. It shells out to
//! exactly the `aoxn` a user would type, captures the output verbatim, and
//! lets the frontend parse diagnostics out of it. That is deliberate: a
//! build that behaves differently inside the IDE than in a terminal is the
//! fastest way to make an IDE untrustworthy, and this way there is nothing
//! to disagree about.
//!
//! Two environment variables matter:
//!
//! * `AOXN_IDE_CC` — the compiler to use. Lets the IDE drive a build of the
//!   compiler's own `target/debug/aoxn` instead of whatever is on PATH.
//! * `AOXN_CLANG` — forwarded from this process when set, because the
//!   compiler shells out to clang and a developer running the IDE from a
//!   shell usually has that variable set to a local LLVM.

use crate::model::{DiagInfo, ExecResult, ToolchainInfo};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

/// The compiler command. `AOXN_IDE_CC` wins; otherwise the name `aoxn`,
/// which a normal install puts on PATH.
pub fn compiler_command() -> String {
    std::env::var("AOXN_IDE_CC").unwrap_or_else(|_| "aoxn".to_string())
}

/// Resolve a bare command name to an executable, the way a shell would.
///
/// On Windows `CreateProcess` only looks at PATH and the current directory,
/// and PATHEXT decides which suffixes count; without this a configured
/// `clang.exe` would report "not found" while working perfectly well from a
/// terminal. On POSIX only an executable `PATH` entry counts.
fn which(name: &str) -> Option<PathBuf> {
    let direct = Path::new(name);
    if name.contains('/') || name.contains('\\') {
        return direct.is_file().then(|| direct.to_path_buf());
    }
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
            .split(';')
            .map(|e| e.to_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&path) {
        for ext in &exts {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Is the Aoxn compiler usable on this machine?
pub fn probe() -> ToolchainInfo {
    let compiler = compiler_command();
    let found = which(&compiler).is_some();
    // clang matters even though the IDE never calls it: the compiler does,
    // and a missing clang is the single most common "why won't this build"
    // question. `AOXN_CLANG` is what the compiler itself honours first.
    let clang = std::env::var("AOXN_CLANG")
        .ok()
        .filter(|c| !c.is_empty())
        .map(|c| PathBuf::from(c))
        .or_else(|| which("clang"))
        .map(|p| p.is_file())
        .unwrap_or(false);

    let version = if found {
        run(&compiler, &["version".to_string()], None)
            .map(|r| r.output.trim().to_string())
            .unwrap_or_default()
    } else {
        String::new()
    };

    ToolchainInfo {
        compiler,
        compiler_found: found,
        clang_found: clang,
        version,
    }
}

/// Run a command, capturing stdout and stderr as one stream.
pub fn run(program: &str, args: &[String], cwd: Option<&str>) -> Result<ExecResult, String> {
    let started = Instant::now();
    let mut cmd = Command::new(program);
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    // Forward the developer's clang override, but only if they set one:
    // inventing a value here would silently mask a missing clang.
    if let Ok(v) = std::env::var("AOXN_CLANG") {
        if !v.is_empty() {
            cmd.env("AOXN_CLANG", v);
        }
    }

    let out = cmd
        .output()
        .map_err(|e| format!("cannot run '{program}': {e}"))?;
    let output = merge(&out.stdout, &out.stderr);
    Ok(ExecResult {
        code: out.status.code().unwrap_or(-1),
        // The raw text is kept verbatim whatever happens next: it is the
        // only place clang's own output and a program's prints survive, and
        // the output panel shows it as-is.
        diags: parse_diags(&output),
        output,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

/// Pull the structured diagnostics out of a `--json` compiler report.
///
/// The report arrives on STDERR and is merged into the same buffer as the
/// program's own stdout, so it is EXTRACTED rather than parsed as the whole
/// log: the leading `{` of the outermost object starts the document, and the
/// scan that follows counts braces while respecting strings and escapes (a
/// `}` inside a diagnostic message must not end the document early — and a
/// message that contains `\"` must not make the scanner think a string
/// closed).
///
/// Returns an empty vector when the text holds no JSON object at all, which
/// is the normal answer for a successful compile with no diagnostics, for a
/// program that printed something brace-shaped, and for a compiler too old
/// to speak `--json`. All three are handled by the caller falling back to
/// the text scanner, so a wrong guess here degrades rather than breaks.
fn parse_diags(output: &str) -> Vec<DiagInfo> {
    let Some(start) = output.find('{') else {
        return Vec::new();
    };
    let Some(doc) = json_object_at(&output[start..]) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(doc) else {
        return Vec::new();
    };
    // `aoxn --json` emits {"ok":…,"errors":[…]}; a document without an
    // `errors` array is not a diagnostics report (it is `doctor`'s, or some
    // program's own output that happened to start with a brace).
    let Some(errors) = v.get("errors").and_then(|e| e.as_array()) else {
        return Vec::new();
    };
    errors
        .iter()
        .map(|e| {
            let s = |k: &str| e.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
            let n = |k: &str| e.get(k).and_then(|x| x.as_u64()).unwrap_or(0) as usize;
            DiagInfo {
                stage: s("stage"),
                file: s("file"),
                line: n("line"),
                col: n("col"),
                message: s("message"),
            }
        })
        .collect()
}

/// The balanced `{…}` or `[…]` starting at `s[0]`, or `None` if it never
/// closes.
///
/// Brace counting with string awareness: an escaped quote does not end the
/// string it is in, and braces inside a string are not structure. Both
/// brackets are accepted because the package manager reports a dependency
/// list as a top-level ARRAY (`aoxn outdated --json`) while the compiler
/// reports diagnostics as an object (`{"errors":[…]}`) — one extractor has
/// to serve both, or every caller grows its own half-correct version.
fn json_object_at(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (i, b) in bytes.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if *b == b'\\' {
                escaped = true;
            } else if *b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(&s[..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Like [`json_object_at`] but scans for the first `{` or `[` anywhere.
///
/// Used where the document's first character is not known in advance — the
/// package manager pretty-prints its report, so a note printed before it
/// would push the opening bracket off position 0.
pub fn json_document(text: &str) -> Option<&str> {
    let at = text.find(['{', '['])?;
    json_object_at(&text[at..])
}

/// Combine the two captured streams into one log.
///
/// The compiler writes diagnostics to stderr and a program's own output to
/// stdout. They are concatenated — stderr last — rather than interleaved,
/// because the standard library cannot recover the true interleaving of two
/// separately piped streams after the fact (and spawning with one shared
/// pipe is not expressible in `std::process`).
///
/// This is stated rather than hidden because it is visible: in a build log
/// every diagnostic is on stderr and comes after any program output, which
/// is the ordering a reader expects anyway. If a future toolchain writes
/// diagnostics to stdout, the order here is simply the order they were
/// written in.
fn merge(stdout: &[u8], stderr: &[u8]) -> String {
    if stderr.is_empty() {
        return String::from_utf8_lossy(stdout).into_owned();
    }
    if stdout.is_empty() {
        return String::from_utf8_lossy(stderr).into_owned();
    }
    let mut s = stdout.to_vec();
    s.extend_from_slice(stderr);
    String::from_utf8_lossy(&s).into_owned()
}

/// Build a program: `aoxn build <path> -o <exe> --json`.
///
/// `--json` is requested on every compile-shaped command so the diagnostics
/// arrive structured. It changes nothing else about the run: the compiler
/// still prints the same human text to stderr, which is what `output`
/// carries to the panel.
pub fn build_args(path: &str, out: &str) -> Vec<String> {
    vec![
        "build".to_string(),
        path.to_string(),
        "-o".to_string(),
        out.to_string(),
        "--json".to_string(),
    ]
}

/// Check a program: `aoxn check <path> --json` runs the full pipeline and
/// prints only the diagnostics. `aoxn c` would be the same compile but it
/// also prints the generated C to stdout, which would flood the output panel
/// with text nobody asked for.
pub fn check_args(path: &str) -> Vec<String> {
    vec!["check".to_string(), path.to_string(), "--json".to_string()]
}

/// The toolchain's own self-check: `aoxn doctor`.
pub fn doctor_args() -> Vec<String> {
    vec!["doctor".to_string()]
}

/// Run a program: `aoxn run <path> --json -l <backend>`.
///
/// `--json` goes BEFORE the link flags and well before any `--` separator,
/// because everything after `--` is handed to the compiled program rather
/// than to the compiler. The link flags are needed because a UI program
/// imports a backend that resolves to external symbols; an Aoxn program
/// with no backend links needs none.
pub fn run_args(path: &str) -> Vec<String> {
    let flags: &[&str] = if cfg!(windows) {
        &["-l", "user32", "-l", "gdi32"]
    } else {
        &["-l", "X11", "-l", "Xft"]
    };
    let mut args = vec!["run".to_string(), path.to_string(), "--json".to_string()];
    args.extend(flags.iter().map(|s| s.to_string()));
    args
}

/// Where a build artifact should land: beside the source, as `name.exe` on
/// Windows and `name` elsewhere.
///
/// This variant is the one commands use, and it CLEANS the destination first.
/// Validating the source is not enough: the compiler publishes the artifact
/// with a write that follows an existing destination, so a pre-existing HARD
/// LINK named `hello.exe` pointing outside the workspace turns a build into
/// an overwrite of that file (reproduced: an outside marker became a 112 KB
/// PE image). Canonicalisation cannot see a hard link — two names for one
/// inode are not a path escape — and asking "is this a link" needs a
/// platform API that is not stable in this toolchain.
///
/// Removing the entry answers it without the question: unlinking a hard link
/// removes THAT NAME and leaves the other name with its data intact, so the
/// compiler then creates a fresh file instead of writing through an existing
/// inode. That closes the class on every platform, for links and symlinks
/// alike.
pub fn artifact_path_prepared(src: &str) -> Result<String, String> {
    let out = artifact_path(src);
    let p = Path::new(&out);
    match std::fs::symlink_metadata(p) {
        Ok(_) => std::fs::remove_file(p).map_err(|e| {
            format!("cannot replace the existing build artifact `{out}`: {e}")
        })?,
        Err(_) => {}
    }
    Ok(out)
}

/// Where a build artifact should land: beside the source, as `name.exe` on
/// Windows and `name` elsewhere.
pub fn artifact_path(src: &str) -> String {
    let p = Path::new(src);
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned());
    let stem = stem.unwrap_or_else(|| "a.out".to_string());
    let parent = p.parent().map(|x| x.to_path_buf()).unwrap_or_default();
    let mut out = parent.join(stem);
    if cfg!(windows) {
        out.set_extension("exe");
    }
    out.to_string_lossy().into_owned()
}

/// Ask the compiler for the top-level declarations of a program and of every
/// module it imports: `aoxn symbols <path> --json`.
///
/// This is what the outline, "go to definition" and the symbol search are
/// built on, and it comes from the compiler's own AST rather than from a
/// regex over the text — so a name that is a parameter is not mistaken for a
/// declaration, and an `extern def` is recognisable as having no body.
///
/// The command does not invoke clang (it stops after the parser), which is
/// what makes it cheap enough to re-run whenever the open file changes.
pub fn symbols_json(path: &str) -> Result<crate::model::SymbolTable, String> {
    let args = vec![
        "symbols".to_string(),
        path.to_string(),
        "--json".to_string(),
    ];
    let result = run(&compiler_command(), &args, None)?;
    // The report goes to stdout (it is the product, not a diagnostic), but it
    // is EXTRACTED rather than parsed as the whole stream: a future compiler
    // that prints a note before the report must not make the outline fail.
    let doc = json_document(&result.output).unwrap_or("");
    match serde_json::from_str::<crate::model::SymbolTable>(doc) {
        Ok(t) => Ok(t),
        Err(e) => {
            // A file that does not parse exits 1 with a diagnostics report on
            // stderr. Hand that to the caller as diagnostics, so the editor
            // can show the error INSTEAD of an empty outline rather than
            // beside it.
            if !result.diags.is_empty() {
                return Err(result
                    .diags
                    .iter()
                    .map(|d| format!("[{}] {}:{}:{}: {}", d.stage, d.file, d.line, d.col, d.message))
                    .collect::<Vec<_>>()
                    .join("\n"));
            }
            Err(format!("cannot read the symbol table: {e}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_sits_beside_the_source_with_the_right_suffix() {
        let p = artifact_path("src/main.ax");
        if cfg!(windows) {
            assert!(p.ends_with("main.exe"), "{p}");
        } else {
            assert_eq!(Path::new(&p).file_name().unwrap(), "main");
        }
    }

    #[test]
    fn check_prints_only_diagnostics_and_run_passes_link_flags() {
        let c = check_args("a.ax");
        assert_eq!(c, vec!["check", "a.ax", "--json"]);
        let r = run_args("a.ax");
        assert_eq!(r[0], "run");
        assert!(r.contains(&"-l".to_string()));
        // `--json` must reach the compiler, never the compiled program.
        assert!(r.contains(&"--json".to_string()));
        assert_eq!(r.iter().position(|a| a == "--json").unwrap(), 2);
    }

    #[test]
    fn a_diagnostics_report_becomes_structured_diags() {
        let log = concat!(
            "hello from the program\n",
            r#"{"ok":false,"errors":[{"stage":"type","file":"D:\\p\\main.ax","line":12,"col":5,"message":"unknown variable 'nope'"}]}"#,
            "\n"
        );
        let d = parse_diags(log);
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].stage, "type");
        assert_eq!(d[0].file, r"D:\p\main.ax");
        assert_eq!(d[0].line, 12);
        assert_eq!(d[0].col, 5);
        assert_eq!(d[0].message, "unknown variable 'nope'");
    }

    #[test]
    fn a_clean_run_yields_no_diags_even_when_the_program_prints_braces() {
        // A program that prints JSON must not be mistaken for a report: the
        // `errors` array is what makes a document a diagnostics report.
        let log = "{\"hello\": \"world\"}\ncount = 3\n";
        assert!(parse_diags(log).is_empty(), "{:?}", parse_diags(log));
    }

    #[test]
    fn braces_and_quotes_inside_a_message_do_not_end_the_document_early() {
        // The scanner has to respect strings: a `}` in a message is not the
        // end of the JSON, and an escaped quote does not close a string.
        let log = concat!(
            r#"{"ok":false,"errors":[{"stage":"parse","file":"a.ax","line":1,"col":1,"message":"expected '}' but found { }"}]}"#,
            "\n"
        );
        let d = parse_diags(log);
        assert_eq!(d.len(), 1, "{d:?}");
        assert!(d[0].message.contains('{'), "{}", d[0].message);
    }

    #[test]
    fn a_quote_escaped_inside_a_message_keeps_the_scan_honest() {
        let log = r#"{"ok":false,"errors":[{"stage":"type","file":"a.ax","line":2,"col":3,"message":"cannot find \"x\" } here"}]}"#;
        let d = parse_diags(log);
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].line, 2);
    }

    #[test]
    fn output_with_no_json_at_all_is_simply_no_diags() {
        // A missing compiler, a crash, a compiler too old for `--json`: all
        // of these must degrade to the frontend's text scanner rather than
        // fail the command.
        for log in ["", "aoxn: command not found\n", "error: '{' unclosed\n"] {
            assert!(parse_diags(log).is_empty(), "{log:?}");
        }
    }

    #[test]
    fn a_top_level_array_is_extracted_too() {
        // `aoxn outdated --json` reports a bare array; an extractor that
        // only understood objects would silently find nothing.
        let doc = json_document("note: checking\n[\n  {\"name\":\"http\"}\n]\n").expect("array");
        assert_eq!(doc, "[\n  {\"name\":\"http\"}\n]");
    }

    #[test]
    fn pretty_printed_json_survives_extraction() {
        let pretty = "{\n  \"packages\": [\n    {\n      \"name\": \"http\",\n      \"version\": \"2.1.0\"\n    }\n  ]\n}";
        let doc = json_document(pretty).expect("doc");
        let v: serde_json::Value = serde_json::from_str(doc).expect("parses");
        assert_eq!(v["packages"][0]["name"], "http");
    }

    #[test]
    fn an_unclosed_document_is_not_extracted() {
        // Truncated output (a killed process) must not be parsed as a
        // half-report that happens to be valid JSON.
        assert!(json_document("{\"errors\":[{\"stage\":\"ty").is_none());
        assert!(json_document("no json here at all").is_none());
        assert!(json_document("").is_none());
    }

    #[test]
    fn several_diagnostics_keep_their_order() {        let log = concat!(
            r#"{"ok":false,"errors":["#,
            r#"{"stage":"lex","file":"a.ax","line":1,"col":1,"message":"first"},"#,
            r#"{"stage":"type","file":"a.ax","line":9,"col":2,"message":"second"},"#,
            r#"{"stage":"io","file":"","line":0,"col":0,"message":"third"}]}"#
        );
        let d = parse_diags(log);
        assert_eq!(d.len(), 3, "{d:?}");
        assert_eq!(d[0].message, "first");
        assert_eq!(d[1].message, "second");
        assert_eq!(d[2].stage, "io");
        assert_eq!(d[2].line, 0);
    }

    #[test]
    fn which_finds_the_shell_and_a_path_miss_does_not() {
        // `sh` is present on every POSIX CI runner; on Windows the test is
        // skipped rather than made conditional, because the whole point is
        // that `which` follows the same rules the OS does.
        if cfg!(windows) {
            assert!(which("definitely-not-a-real-binary-xyz").is_none());
        } else {
            assert!(which("sh").is_some());
        }
    }

    #[test]
    fn run_reports_exit_code_and_captures_output() {
        let (prog, args): (&str, Vec<String>) = if cfg!(windows) {
            ("cmd", vec!["/C".into(), "echo hello".into()])
        } else {
            ("sh", vec!["-c".into(), "echo hello".into()])
        };
        let r = run(prog, &args, None).expect("run");
        assert_eq!(r.code, 0);
        assert!(r.output.contains("hello"), "got {:?}", r.output);
    }

    #[test]
    fn run_reports_a_missing_program_as_an_error() {
        assert!(run("definitely-not-a-real-binary-xyz", &[], None).is_err());
    }
    use super::*;

    /// THE BUILD-OVERWRITE ESCAPE: a pre-existing HARD LINK named after the
    /// artifact turns a build into an overwrite of an unrelated file, because
    /// the compiler publishes through a write that follows an existing
    /// destination (reproduced: an outside marker became a 112 KB PE image).
    ///
    /// The mitigation does not ask whether the destination is a link — that
    /// needs an unstable Windows API — it REMOVES the entry, so the compiler
    /// creates a fresh file. Unlinking a hard link removes that NAME and
    /// leaves the other name's data alone, which is exactly the property this
    /// test asserts.
    #[test]
    fn preparing_an_artifact_leaves_an_outside_hard_link_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside.txt");
        std::fs::write(&outside, "ORIGINAL").unwrap();

        let src = dir.path().join("hello.ax");
        std::fs::write(&src, "def main() -> int:\n    return 0\n").unwrap();
        let out = artifact_path_prepared(&src.to_string_lossy()).unwrap();
        assert!(out.ends_with(if cfg!(windows) { "hello.exe" } else { "hello" }));

        // plant the link the audit used: the artifact name IS the outside file
        let planted = dir.path().join(if cfg!(windows) { "hello.exe" } else { "hello" });
        std::fs::hard_link(&outside, &planted).unwrap();

        let prepared = artifact_path_prepared(&src.to_string_lossy()).unwrap();
        assert!(!Path::new(&prepared).exists(), "the link entry should be gone");
        assert_eq!(
            std::fs::read_to_string(&outside).unwrap(),
            "ORIGINAL",
            "preparing the artifact modified the file the link pointed at"
        );
        // the outside file still has its own name and its data
    }

    /// A missing artifact is the ordinary case and must not error.
    #[test]
    fn preparing_an_absent_artifact_is_a_no_op() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("x.ax");
        std::fs::write(&src, "def main() -> int:\n    return 0\n").unwrap();
        let out = artifact_path_prepared(&src.to_string_lossy()).unwrap();
        assert!(out.contains("x"));
    }
}
