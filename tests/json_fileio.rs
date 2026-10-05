//! stdlib/json.ax file-I/O pair (the v0.44.0 promotion's addition):
//! write a DOM to disk, read it back through the getters, err == 2 for a
//! missing file vs err == 1 for a readable-but-empty one.

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
import * from "stdlib/json.ax"
import * from "stdlib/os.ax"

def main() -> int:
    fails = 0
    p = j_parse("{\"name\":\"aoxn\",\"n\":7,\"tags\":[\"a\",\"b\"]}")
    if p.err == 0 and j_write_file("doc.json", p.dom, p.dom.last):
        print("PASS json-write")
    else:
        print("FAIL json-write " + str(p.err))
        fails = fails + 1
    p2 = j_read_file("doc.json")
    if p2.err == 0 and j_get_str(p2.dom, p2.dom.last, "name", "?") == "aoxn" and j_get_int(p2.dom, p2.dom.last, "n", -1) == 7:
        print("PASS json-read")
    else:
        print("FAIL json-read " + str(p2.err))
        fails = fails + 1
    arr = j_obj_get(p2.dom, p2.dom.last, "tags")
    if j_len(p2.dom, arr) == 2 and j_arr_str(p2.dom, arr, 1, "?") == "b":
        print("PASS json-read-arr")
    else:
        print("FAIL json-read-arr")
        fails = fails + 1
    miss = j_read_file("definitely-missing.json")
    if miss.err == 2:
        print("PASS json-read-missing")
    else:
        print("FAIL json-read-missing " + str(miss.err))
        fails = fails + 1
    # a readable but empty file is a PARSE error (1), not an I/O error
    write_file("empty.json", "")
    emp = j_read_file("empty.json")
    if emp.err == 1:
        print("PASS json-read-empty")
    else:
        print("FAIL json-read-empty " + str(emp.err))
        fails = fails + 1
    os_remove("doc.json")
    os_remove("empty.json")
    print("DONE fails=" + str(fails))
    return fails

"#;

#[test]
fn json_file_io() {
    if aoxn::find_clang().is_none() {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    }
    // the drivers import stdlib modules BY NAME, which exercises the
    // AOXN_STDLIB resolution path (env -> <root>/lib/stdlib -> checkout)
    std::env::set_var("AOXN_STDLIB", abs("stdlib"));
    let dir = std::env::temp_dir().join(format!("aoxn-json_file_io-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let _ = std::fs::remove_dir_all(&dir.join("sandbox"));

    let src_path = dir.join("json_file_io.ax");
    let exe = dir.join(format!("json_file_io{EXE}"));
    std::fs::write(&src_path, SRC).unwrap();
    aoxn::build_paths_opts(&[src_path.display().to_string()], &exe, true, &[], &[])
        .unwrap_or_else(|d| panic!("driver failed to compile: {d:?}"));
    let out =
    Command::new(&exe).current_dir(&dir).output().expect("failed to run driver");
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
        "json-write",
        "json-read",
        "json-read-arr",
        "json-read-missing",
    ] {
        assert!(
            text.contains(&format!("PASS {marker}")),
            "missing PASS {marker}
{text}"
        );
    }
}
