//! stdlib/csv.ax: RFC 4180 parsing/serialization, error positions, and a
//! differential comparison against Python's `csv` module when Python is
//! available (the canonical fixtures below are pinned regardless).

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const EXE: &str = if cfg!(windows) { ".exe" } else { "" };

const DRIVER: &str = r#"
import * from "stdlib/csv.ax"

def str_chr(b: int) -> string:
    p = malloc(2)
    store_u8(p, 0, b)
    store_u8(p, 1, 0)
    return as_string(p)

def hex_str(s: string) -> string:
    d = "0123456789abcdef"
    w = sb_new(len(s) * 2 + 8)
    i = 0
    while i < len(s):
        b = str_get(s, i)
        w = sb_byte(w, str_get(d, b / 16))
        w = sb_byte(w, str_get(d, b % 16))
        i = i + 1
    return sb_text(w)

def main() -> int:
    mode = arg(0)
    path = arg(1)
    if mode == "fields":
        t = csv_read_file(path)
        if t.err != 0:
            print("ERR " + str(t.err) + " " + str(t.line) + ":" + str(t.col) + " " + t.msg)
            return 0
        r = 0
        while r < csv_rows(t):
            print("R" + str(r) + " N" + str(csv_cols(t, r)))
            c = 0
            while c < csv_cols(t, r):
                print("F" + str(c) + "=" + hex_str(csv_get(t, r, c)))
                c = c + 1
            r = r + 1
        print("OK rows=" + str(csv_rows(t)) + " cols=" + str(csv_max_cols(t)))
        return 0
    if mode == "dump":
        out_path = arg(2)
        t = csv_read_file(path)
        if t.err != 0:
            print("ERR " + str(t.err))
            return 1
        if not write_file(out_path, csv_dumps(t)):
            print("WRITE FAIL")
            return 1
        print("OK")
        return 0
    print("unknown mode")
    return 2
"#;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn build_driver(dir: &Path) -> PathBuf {
    let driver = dir.join("csv_driver.ax");
    fs::write(&driver, DRIVER).unwrap();
    let exe = dir.join(format!("csv_driver{EXE}"));
    aoxn::build_paths_opts(&[driver.display().to_string()], &exe, true, &[], &[])
        .unwrap_or_else(|d| panic!("csv driver failed to compile: {d:?}"));
    exe
}

fn run(exe: &Path, args: &[&str]) -> std::process::Output {
    Command::new(exe).args(args).output().unwrap()
}

fn python_available() -> Option<String> {
    for cand in ["python", "python3"] {
        if Command::new(cand).arg("--version").output().map(|o| o.status.success()).unwrap_or(false) {
            return Some(cand.to_string());
        }
    }
    None
}

fn python_fields(python: &str, fixture: &Path, dir: &Path) -> String {
    let script = dir.join("csv_ref.py");
    fs::write(
        &script,
        r#"
import csv, sys
path = sys.argv[1]
with open(path, newline='') as f:
    rows = list(csv.reader(f))
out = []
for r, row in enumerate(rows):
    out.append("R%d N%d" % (r, len(row)))
    for c, field in enumerate(row):
        out.append("F%d=%s" % (c, field.encode('utf-8').hex()))
out.append("OK rows=%d cols=%d" % (len(rows), max([len(r) for r in rows] + [0])))
print("\n".join(out))
"#,
    )
    .unwrap();
    let out = Command::new(python)
        .arg(&script)
        .arg(fixture)
        .output()
        .expect("python reference failed to run");
    assert!(out.status.success(), "python: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n")
}

fn python_dump(python: &str, fixture: &Path, out_path: &Path, dir: &Path) {
    let script = dir.join("csv_write_ref.py");
    fs::write(
        &script,
        r#"
import csv, sys
with open(sys.argv[1], newline='') as f:
    rows = list(csv.reader(f))
with open(sys.argv[2], 'w', newline='') as f:
    w = csv.writer(f, lineterminator='\r\n')
    w.writerows(rows)
"#,
    )
    .unwrap();
    let out = Command::new(python)
        .arg(&script)
        .arg(fixture)
        .arg(out_path)
        .output()
        .expect("python writer failed to run");
    assert!(out.status.success(), "python writer: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn csv_parses_and_serializes_and_matches_python() {
    let dir = root().join("target").join(format!("csv-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let exe = build_driver(&dir);
    let python = python_available();

    let fixtures: &[(&str, &str)] = &[
        ("empty", ""),
        ("basic", "a,b\nc,d\n"),
        ("crlf", "a,b\r\nc,d\r\n"),
        ("quoted", "a,\"b,c\",d\r\n\"e\"\"f\",g\n"),
        ("blank-lines", "\n\n"),
        ("trailing-fields", "a,b,\n,,\n"),
        ("no-final-newline", "x,y"),
        ("multiline", "a,\"multi\nline\",z\r\nlast\n"),
        ("spaces", " a , b \n\" c \",d\n"),
    ];

    for (name, text) in fixtures {
        let fixture = dir.join(format!("{name}.csv"));
        fs::write(&fixture, text).unwrap();
        let out = run(&exe, &["fields", &fixture.display().to_string()]);
        assert!(
            out.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(text.contains("OK rows="), "{name}: {text}");
        if let Some(py) = &python {
            assert_eq!(text, python_fields(py, &fixture, &dir), "{name}: python field mismatch");
            let dump = dir.join(format!("{name}.out.csv"));
            let out = run(
                &exe,
                &["dump", &fixture.display().to_string(), &dump.display().to_string()],
            );
            assert!(out.status.success(), "{name}: dump failed");
            let py_out = dir.join(format!("{name}.py.csv"));
            python_dump(py, &fixture, &py_out, &dir);
            assert_eq!(
                fs::read(&dump).unwrap(),
                fs::read(&py_out).unwrap(),
                "{name}: python serialization mismatch"
            );
        }
    }

    // Pinned values for the core fixtures (independent of Python).
    let basic = dir.join("basic.csv");
    let out = run(&exe, &["fields", &basic.display().to_string()]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("R0 N2"), "{text}");
    assert!(text.contains("F0=61"), "{text}");
    assert!(text.contains("F1=62"), "{text}");
    assert!(text.contains("F1=64"), "{text}");

    let quoted = dir.join("quoted.csv");
    let out = run(&exe, &["fields", &quoted.display().to_string()]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("F1=622c63"), "quoted comma: {text}");
    assert!(text.contains("F0=652266"), "escaped quote: {text}");

    let blank = dir.join("blank-lines.csv");
    let out = run(&exe, &["fields", &blank.display().to_string()]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("R0 N0"), "blank row is empty: {text}");
    assert!(text.contains("R1 N0"), "blank row is empty: {text}");
    assert!(text.contains("OK rows=2 cols=0"), "{text}");

    // Malformed inputs must be value errors with a position.
    for (name, text) in [
        ("unterminated", "a,\"unterminated"),
        ("stray-quote", "a\"b,c\n"),
        ("after-quote", "\"a\"b,c\n"),
    ] {
        let fixture = dir.join(format!("bad-{name}.csv"));
        fs::write(&fixture, text).unwrap();
        let out = run(&exe, &["fields", &fixture.display().to_string()]);
        assert!(out.status.success());
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.starts_with("ERR 1 "), "{name}: {text}");
        assert!(!text.contains("OK"), "{name}: malformed input accepted: {text}");
    }

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn csv_module_is_selfhost_compilable() {
    let Some(clang) = aoxn::find_clang() else {
        assert!(
            std::env::var_os("AOXN_TEST_REQUIRE_CLANG").is_none(),
            "clang required"
        );
        eprintln!("skipping: clang unavailable");
        return;
    };
    let dir = root().join("target").join(format!("csv-selfhost-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = dir.join("csv_selfhost.ax");
    fs::write(
        &source,
        r#"import * from "stdlib/csv.ax"
def main():
    t = csv_parse("a,\"b,c\"\r\n")
    print(csv_rows(t))
    print(csv_get(t, 0, 1))
    print(csv_dumps(t) == "a,\"b,c\"\r\n")
"#,
    )
    .unwrap();
    let cli = dir.join(format!("selfhost-cli{EXE}"));
    aoxn::build_paths_exe(
        &[root().join("selfhost/cli.ax").display().to_string()],
        &cli,
        true,
    )
    .expect("build the selfhost CLI");
    let out = Command::new(&cli)
        .current_dir(&dir)
        .env("AOXN_STDLIB", root().join("stdlib"))
        .env("AOXN_CLANG", &clang)
        .args(["run", &source.display().to_string()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "selfhost csv failed: {} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(text, "1\nb,c\ntrue\n", "{text}");
    fs::remove_dir_all(&dir).unwrap();
}
