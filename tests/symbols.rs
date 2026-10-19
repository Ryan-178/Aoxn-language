//! `Aoxn symbols` — the language-service export the IDE builds its outline,
//! its "go to definition" and its symbol search on.
//!
//! These drive the real binary (`CARGO_BIN_EXE_aoxn`) rather than the
//! library, because the contract that matters is the one an editor consumes:
//! the command line, the exit code, and the SHAPE of the `--json` document.
//! A unit test on `symbols::collect` would pass while the command printed
//! something the IDE cannot parse.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

const AOXN: &str = env!("CARGO_BIN_EXE_aoxn");
static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Scratch directory outside the repo, removed when the test ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("aoxn-sym-test-{}-{tag}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        Scratch(dir)
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

fn aoxn(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(AOXN).args(args).output().expect("run aoxn");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The crude JSON reader these tests use: the document is emitted by our own
/// code with a fixed shape, so a full parser would be a second thing to keep
/// in sync. It is deliberately strict — if the shape changes, these fail.
fn field<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\":\"");
    let start = json.find(&needle)? + needle.len();
    let rest = &json[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

/// How many times a JSON key appears — `count(json, "extern")` counts
/// `"extern":`, so a caller can assert on a key that legitimately repeats
/// (several symbols each carry one) without counting anything else.
fn count(json: &str, key: &str) -> usize {
    json.matches(&format!("\"{key}\":")).count()
}

#[test]
fn json_lists_top_level_declarations_with_positions() {
    let s = Scratch::new("outline");
    let f = s.write(
        "main.ax",
        "struct Point:\n    x: int\n\ndef main() -> int:\n    p = Point(x=1)\n    return p.x\n",
    );
    let (code, out, err) = aoxn(&["symbols", &f.to_string_lossy(), "--json"]);
    assert_eq!(code, 0, "stderr: {err}");
    // Structs are emitted before functions (that is the AST order), so the
    // first declaration in the document is the struct.
    assert_eq!(field(&out, "kind"), Some("struct"), "first symbol: {out}");
    assert!(out.contains("\"name\":\"main\""), "{out}");
    assert!(out.contains("\"name\":\"Point\""), "{out}");
    assert!(out.contains("\"signature\":\"def main() -> int\""), "{out}");
    assert!(out.contains("\"line\":4"), "main is on line 4: {out}");
    // A struct's fields travel with it, so a hover can show them.
    assert!(out.contains("\"fields\":[{\"name\":\"x\",\"type\":\"int\"}]"), "{out}");
}

#[test]
fn an_imported_file_contributes_its_symbols() {
    // The cross-file case is the whole reason the IDE needs this: a symbol
    // search over one file would miss everything the file imports.
    let s = Scratch::new("imports");
    s.write("util.ax", "def clamp_i(v: int, lo: int) -> int:\n    return lo\n");
    let f = s.write(
        "main.ax",
        "import * from \"./util.ax\"\n\ndef main() -> int:\n    return 0\n",
    );
    let (code, out, err) = aoxn(&["symbols", &f.to_string_lossy(), "--json"]);
    assert_eq!(code, 0, "stderr: {err}");
    assert!(out.contains("\"name\":\"clamp_i\""), "imported fn missing: {out}");
    assert!(out.contains("\"name\":\"main\""), "{out}");
    // The imported symbol must name the file it actually lives in.
    assert!(out.contains("util.ax"), "{out}");
}

#[test]
fn the_human_form_is_readable_and_the_json_form_is_single_line() {
    let s = Scratch::new("forms");
    let f = s.write("main.ax", "def main() -> int:\n    return 0\n");
    let (code, text, err) = aoxn(&["symbols", &f.to_string_lossy()]);
    assert_eq!(code, 0, "stderr: {err}");
    assert!(text.contains("function"), "{text}");
    assert!(text.contains("def main() -> int"), "{text}");

    let (_, json, _) = aoxn(&["symbols", &f.to_string_lossy(), "--json"]);
    assert_eq!(json.lines().count(), 1, "json must be one line: {json}");
}

#[test]
fn a_file_with_no_declarations_says_so() {
    let s = Scratch::new("empty");
    let f = s.write("notes.ax", "# just a comment\n");
    let (code, text, _) = aoxn(&["symbols", &f.to_string_lossy()]);
    assert_eq!(code, 0);
    assert!(text.contains("no top-level declarations"), "{text}");
    let (_, json, _) = aoxn(&["symbols", &f.to_string_lossy(), "--json"]);
    assert_eq!(json.trim(), "{\"symbols\":[]}");
}

#[test]
fn a_broken_file_exits_nonzero_with_a_diagnostic() {
    // An outline for a file that does not parse would send the user to a
    // declaration that is not there, so this must fail loudly instead.
    let s = Scratch::new("broken");
    let f = s.write("bad.ax", "def ok() -> int:\n    return 0\n\nclass Nope:\n    pass\n");
    let (code, _out, err) = aoxn(&["symbols", &f.to_string_lossy()]);
    assert_eq!(code, 1);
    assert!(err.contains("[parse]"), "stderr: {err}");

    // --json must produce a parseable document, not prose.
    let (code, _out, err) = aoxn(&["symbols", &f.to_string_lossy(), "--json"]);
    assert_eq!(code, 1);
    assert!(err.trim_start().starts_with('{'), "stderr: {err}");
    assert!(err.contains("\"stage\":\"parse\""), "stderr: {err}");
}

#[test]
fn a_missing_file_is_an_io_diagnostic() {
    let (code, _out, err) = aoxn(&["symbols", "definitely-not-here.ax", "--json"]);
    assert_eq!(code, 1);
    assert!(err.contains("\"stage\":\"io\""), "stderr: {err}");
}

#[test]
fn no_input_file_is_a_usage_error() {
    let (code, _out, err) = aoxn(&["symbols"]);
    assert_eq!(code, 2);
    assert!(err.contains("needs an input file"), "stderr: {err}");
}

#[test]
fn extern_declarations_are_distinguishable_from_definitions() {
    // A caller that cannot tell them apart sends "go to definition" into a
    // body that does not exist.
    let s = Scratch::new("extern");
    let f = s.write(
        "main.ax",
        "extern def GetTickCount() -> int\n\ndef main() -> int:\n    return 0\n",
    );
    let (code, out, err) = aoxn(&["symbols", &f.to_string_lossy(), "--json"]);
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(count(&out, "extern"), 2, "two functions: {out}");
    assert!(out.contains("\"extern\":true"), "{out}");
    assert!(out.contains("extern def GetTickCount() -> int"), "{out}");
}
