//! stdlib/bisect.ax + heapq.ax tests (v0.44.0) — insertion positions
//! (left vs right), float-slot variants, and the min-heap lifecycle
//! including heapify-from-vec and a full drain.

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
import * from "stdlib/bisect.ax"
import * from "stdlib/heapq.ax"

def main() -> int:
    fails = 0
    a = vec_new()
    a = vec_push(a, 1)
    a = vec_push(a, 3)
    a = vec_push(a, 5)
    a = vec_push(a, 7)
    if bisect_left(a, 5) == 2 and bisect_right(a, 5) == 3 and bisect_left(a, 4) == 2 and bisect_right(a, 0) == 0 and bisect_left(a, 9) == 4:
        print("PASS bisect-positions")
    else:
        print("FAIL bisect-positions")
        fails = fails + 1
    a = insort_right(a, 4)
    if a.len == 5 and vec_get(a, 2) == 4 and vec_get(a, 3) == 5:
        print("PASS insort-right")
    else:
        print("FAIL insort-right")
        fails = fails + 1
    a = insort_left(a, 4)
    if a.len == 6 and vec_get(a, 2) == 4 and vec_get(a, 3) == 4 and vec_get(a, 4) == 5:
        print("PASS insort-left")
    else:
        print("FAIL insort-left")
        fails = fails + 1
    if bisect_find(a, 5) == 4 and bisect_find(a, 6) == -1:
        print("PASS bisect-find")
    else:
        print("FAIL bisect-find")
        fails = fails + 1
    f = vec_new()
    f = insort_right_f(f, 2.5)
    f = insort_right_f(f, 1.5)
    f = insort_right_f(f, 2.0)
    if f.len == 3 and bisect_left_f(f, 2.0) == 1 and bisect_find_f(f, 2.5) == 2:
        print("PASS bisect-float")
    else:
        print("FAIL bisect-float")
        fails = fails + 1
    h = heap_new()
    h = heap_push(h, 5)
    h = heap_push(h, 3)
    h = heap_push(h, 8)
    h = heap_push(h, 1)
    if heap_peek(h) == 1 and heap_len(h) == 4:
        print("PASS heap-peek")
    else:
        print("FAIL heap-peek " + str(heap_peek(h)))
        fails = fails + 1
    h = heap_pop(h)
    if h.res == 1 and heap_len(h) == 3:
        print("PASS heap-pop-1")
    else:
        print("FAIL heap-pop-1 " + str(h.res))
        fails = fails + 1
    h = heap_pop(h)
    if h.res == 3:
        print("PASS heap-pop-3")
    else:
        print("FAIL heap-pop-3 " + str(h.res))
        fails = fails + 1
    h = heap_push(h, 0)
    h = heap_pop(h)
    if h.res == 0:
        print("PASS heap-push-after-pop")
    else:
        print("FAIL heap-push-after-pop " + str(h.res))
        fails = fails + 1
    v = vec_new()
    v = vec_push(v, 9)
    v = vec_push(v, 4)
    v = vec_push(v, 7)
    v = vec_push(v, 1)
    v = vec_push(v, 8)
    h2 = heap_from_vec(v)
    out = heap_sorted(h2)
    if out.len == 5 and vec_get(out, 0) == 1 and vec_get(out, 1) == 4 and vec_get(out, 2) == 7 and vec_get(out, 3) == 8 and vec_get(out, 4) == 9:
        print("PASS heap-from-vec-sorted")
    else:
        print("FAIL heap-from-vec-sorted")
        fails = fails + 1
    print("DONE fails=" + str(fails))
    return fails

"#;

#[test]
fn bisect_heapq_containers() {
    if aoxn::find_clang().is_none() {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    }
    // the drivers import stdlib modules BY NAME, which exercises the
    // AOXN_STDLIB resolution path (env -> <root>/lib/stdlib -> checkout)
    std::env::set_var("AOXN_STDLIB", abs("stdlib"));
    let dir = std::env::temp_dir().join(format!("aoxn-bisect_heapq_containers-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src_path = dir.join("bisect_heapq_containers.ax");
    let exe = dir.join(format!("bisect_heapq_containers{EXE}"));
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
        "bisect-positions",
        "insort-right",
        "insort-left",
        "bisect-find",
        "bisect-float",
        "heap-peek",
        "heap-pop-1",
        "heap-pop-3",
        "heap-push-after-pop",
        "heap-from-vec-sorted",
    ] {
        assert!(
            text.contains(&format!("PASS {marker}")),
            "missing PASS {marker}
{text}"
        );
    }
}
