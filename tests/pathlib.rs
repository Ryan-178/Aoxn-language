//! stdlib/pathlib.ax tests (v0.44.0) — pure path operations:
//! name/stem/suffix/parent, join precedence, absolute detection, lexical
//! norm, the with_* rebuilders, and the split struct.

use std::process::Command;

const EXE: &str = if cfg!(windows) { ".exe" } else { "" };

fn abs(rel: &str) -> String {
    // forward slashes: backslashes would read as escape sequences inside
    // the Aoxn import string literal
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(rel)
        .display()
        .to_string()
        .replace('\\', "/")
}

const SRC: &str = r#"
import * from "stdlib/pathlib.ax"

def main() -> int:
    fails = 0
    if path_name("a/b/c.txt") == "c.txt" and path_name("C:\\x\\y") == "y" and path_name("plain") == "plain":
        print("PASS path-name")
    else:
        print("FAIL path-name")
        fails = fails + 1
    if path_parent("a/b/c") == "a/b" and path_parent("solo") == "" and path_parent("a/b/") == "a/b":
        print("PASS path-parent")
    else:
        print("FAIL path-parent")
        fails = fails + 1
    if path_suffix("a.tar.gz") == ".gz" and path_suffix(".gitignore") == "" and path_suffix("noext") == "":
        print("PASS path-suffix")
    else:
        print("FAIL path-suffix")
        fails = fails + 1
    if path_stem("a.tar.gz") == "a.tar" and path_stem("main.ax") == "main" and path_stem(".hidden") == ".hidden":
        print("PASS path-stem")
    else:
        print("FAIL path-stem")
        fails = fails + 1
    if path_join("a", "b") == "a\\b" and path_join("a\\", "b") == "a\\b" and path_join("a", "C:\\b") == "C:\\b":
        print("PASS path-join")
    else:
        print("FAIL path-join")
        fails = fails + 1
    if path_is_abs("C:\\x") and path_is_abs("\\root") and not path_is_abs("x/y") and not path_is_abs(""):
        print("PASS path-is-abs")
    else:
        print("FAIL path-is-abs")
        fails = fails + 1
    if path_norm("a//b\\./c\\..\\d") == "a\\b\\d" and path_norm("..\\keep") == "..\\keep" and path_norm(".") == ".":
        print("PASS path-norm")
    else:
        print("FAIL path-norm " + path_norm("a//b\\./c\\..\\d"))
        fails = fails + 1
    if path_with_suffix("a/b.txt", ".md") == "a\\b.md" and path_with_name("a/b.txt", "c.txt") == "a\\c.txt":
        print("PASS path-with")
    else:
        print("FAIL path-with")
        fails = fails + 1
    sp = path_split("dir/file.ax")
    if sp.dir == "dir" and sp.name == "file.ax":
        print("PASS path-split")
    else:
        print("FAIL path-split")
        fails = fails + 1
    print("DONE fails=" + str(fails))
    return fails

"#;

#[test]
fn pathlib_module_strings() {
    if aoxn::find_clang().is_none() {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    }
    // the drivers import stdlib modules BY NAME, which exercises the
    // AOXN_STDLIB resolution path (env -> <root>/lib/stdlib -> checkout)
    std::env::set_var("AOXN_STDLIB", abs("stdlib"));
    let dir = std::env::temp_dir().join(format!("aoxn-pathlib_module_strings-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src_path = dir.join("pathlib_module_strings.ax");
    let exe = dir.join(format!("pathlib_module_strings{EXE}"));
    std::fs::write(&src_path, SRC).unwrap();
    aoxn::build_paths_opts(&[src_path.display().to_string()], &exe, true, &[], &[])
        .unwrap_or_else(|d| panic!("driver failed to compile: {d:?}"));
    let out =
    Command::new(&exe).output().expect("failed to run driver");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "driver exited {:?}
stdout:
{text}
stderr:
{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("DONE fails=0"), "driver reported failures:
{text}");
    assert!(!text.contains("FAIL"), "driver printed FAIL:
{text}");
    for marker in [
        "path-name",
        "path-parent",
        "path-suffix",
        "path-stem",
        "path-join",
        "path-is-abs",
        "path-norm",
        "path-with",
        "path-split",
    ] {
        assert!(
            text.contains(&format!("PASS {marker}")),
            "missing PASS {marker}
{text}"
        );
    }
}
