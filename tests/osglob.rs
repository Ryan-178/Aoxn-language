//! stdlib/os.ax + glob.ax tests (v0.44.0) against a sandbox directory
//! under the driver's cwd: attributes, listdir, copy/rename/remove,
//! environment (unset via NULL), chdir, the pure matcher, the directory
//! walk (dotfile rule, subdir, sorted output), and the json.ax file I/O
//! pair on top of it.

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
import * from "stdlib/os.ax"
import * from "stdlib/glob.ax"
import * from "stdlib/json.ax"

# the driver creates a sandbox directory under its cwd and exercises the
# os/glob/json-file layers against it; every step is idempotent

def vec_has(v: Vec, s: string) -> bool:
    i = 0
    while i < v.len:
        if vec_get_str(v, i) == s:
            return True
        i = i + 1
    return False

# does s end with tail? (no substring builtin; the loop is short)
def ends_with(s: string, tail: string) -> bool:
    n = len(s)
    m = len(tail)
    if m > n:
        return False
    off = n - m
    i = 0
    while i < m:
        if str_get(s, off + i) != str_get(tail, i):
            return False
        i = i + 1
    return True

def main() -> int:
    fails = 0
    # ---- fresh sandbox ----
    if os_exists("sandbox"):
        if not os_isdir("sandbox/sub"):
            print("FAIL sandbox-pre")
            return 1
    else:
        if not os_mkdir("sandbox"):
            print("FAIL mkdir-sandbox")
            return 1
    if not os_exists("sandbox/sub"):
        if not os_mkdir("sandbox/sub"):
            print("FAIL mkdir-sub")
            return 1
    write_file("sandbox/a.txt", "alpha")
    write_file("sandbox/b.ax", "def main() -> int:\n    return 0\n")
    write_file("sandbox/sub/c.txt", "gamma")
    write_file("sandbox/.hidden", "h")
    # ---- attributes ----
    if os_exists("sandbox/a.txt") and os_isfile("sandbox/a.txt") and not os_isdir("sandbox/a.txt"):
        print("PASS attr-file")
    else:
        print("FAIL attr-file")
        fails = fails + 1
    if os_isdir("sandbox/sub") and not os_isfile("sandbox/sub") and os_exists("sandbox/missing.zzz") == False:
        print("PASS attr-dir")
    else:
        print("FAIL attr-dir")
        fails = fails + 1
    # ---- listdir ----
    names = os_listdir("sandbox")
    if vec_has(names, "a.txt") and vec_has(names, "b.ax") and vec_has(names, "sub") and vec_has(names, ".hidden") and not vec_has(names, ".") and not vec_has(names, ".."):
        print("PASS listdir")
    else:
        print("FAIL listdir")
        fails = fails + 1
    if os_listdir("sandbox/does-not-exist").len == 0:
        print("PASS listdir-missing")
    else:
        print("FAIL listdir-missing")
        fails = fails + 1
    # ---- copy / rename / remove ----
    if os_copy("sandbox/a.txt", "sandbox/copy.txt", True) and os_isfile("sandbox/copy.txt"):
        print("PASS copy")
    else:
        print("FAIL copy")
        fails = fails + 1
    if os_rename("sandbox/copy.txt", "sandbox/moved.txt") and os_isfile("sandbox/moved.txt") and not os_exists("sandbox/copy.txt"):
        print("PASS rename")
    else:
        print("FAIL rename")
        fails = fails + 1
    if os_remove("sandbox/moved.txt") and not os_exists("sandbox/moved.txt"):
        print("PASS remove")
    else:
        print("FAIL remove")
        fails = fails + 1
    if not os_copy("sandbox/a.txt", "sandbox/b.ax", True):
        print("PASS copy-fail-if-exists")
    else:
        # the failed call must not have clobbered the destination
        os_copy("sandbox/b.ax", "sandbox/b.bak", False)
        print("FAIL copy-fail-if-exists")
        fails = fails + 1
    os_remove("sandbox/b.bak")
    # ---- environment ----
    if os_setenv("AOXN_STDLIB_TEST_VAR", "42") and os_getenv("AOXN_STDLIB_TEST_VAR") == "42" and os_has_env("AOXN_STDLIB_TEST_VAR"):
        print("PASS env-set-get")
    else:
        print("FAIL env-set-get " + os_getenv("AOXN_STDLIB_TEST_VAR"))
        fails = fails + 1
    os_unsetenv("AOXN_STDLIB_TEST_VAR")
    if not os_has_env("AOXN_STDLIB_TEST_VAR"):
        print("PASS env-unset")
    else:
        print("FAIL env-unset")
        fails = fails + 1
    if len(os_getenv("PATH")) > 0:
        print("PASS env-path")
    else:
        print("FAIL env-path")
        fails = fails + 1
    # a value LONGER than the fixed 2048-char buffer getenv used to have must
    # survive the round trip (the CI runners' PATH does; v0.44.0 truncated it
    # silently and the env-path check above failed there — v0.44.1's pin)
    long_val = ""
    i = 0
    while i < 3000:
        long_val = long_val + "x"
        i = i + 1
    os_setenv("AOXN_STDLIB_TEST_LONG", long_val)
    got = os_getenv("AOXN_STDLIB_TEST_LONG")
    if got == long_val:
        print("PASS env-long-value")
    else:
        print("FAIL env-long-value " + str(len(got)))
        fails = fails + 1
    os_unsetenv("AOXN_STDLIB_TEST_LONG")
    # ---- working directory ----
    # Each term is reported on its own. v0.50.3 printed a bare "FAIL chdir"
    # on Linux, which cannot say WHICH of "os_cwd() gave nothing", "the
    # chdir failed", "the cwd did not change" or "the file is not there" —
    # and the failures after it (every glob hit empty, json file I/O dead)
    # were all consistent with the process having STAYED in the sandbox, so
    # the bare marker left the real question open.
    sep = path_sep()
    before = os_cwd()
    if len(before) == 0:
        print("FAIL chdir-cwd-empty")
        fails = fails + 1
    else:
        ok_chdir = os_chdir("sandbox")
        if not ok_chdir:
            print("FAIL chdir-call " + before)
            fails = fails + 1
        else:
            after = os_cwd()
            if after == before:
                print("FAIL chdir-no-move " + after)
                fails = fails + 1
            elif not os_isfile("a.txt"):
                print("FAIL chdir-file-missing " + after)
                fails = fails + 1
            else:
                print("PASS chdir")
        os_chdir(before)
    # a string from os_cwd() must OWN its buffer. v0.50.5 returned one that
    # aliased a block the function had already freed; glibc handed the same
    # 4096-byte chunk to the next os_cwd(), so the two answers compared EQUAL
    # even though the working directory had moved — which is invisible as a
    # crash and silent as a wrong directory. Two calls with a chdir between
    # them is the whole repro.
    c1 = os_cwd()
    os_chdir("sandbox")
    c2 = os_cwd()
    os_chdir(c1)
    if len(c1) > 0 and len(c2) > 0 and c1 != c2 and ends_with(c2, "sandbox"):
        print("PASS cwd-answers-own-their-buffer")
    else:
        print("FAIL cwd-answers-own-their-buffer [" + c1 + "] [" + c2 + "]")
        fails = fails + 1
    # ---- glob matcher (pure) ----
    if glob_match("*.ax", "foo.ax") and not glob_match("*.ax", "foo.c") and glob_match("a?c", "abc") and not glob_match("a?c", "abbc"):
        print("PASS match-star-q")
    else:
        print("FAIL match-star-q")
        fails = fails + 1
    if glob_match("a*b*c", "aXbYc") and glob_match("a*b*c", "abc") and not glob_match("a*b*d", "aXbYc"):
        print("PASS match-multi-star")
    else:
        print("FAIL match-multi-star")
        fails = fails + 1
    if glob_match("[a-c]x", "bx") and not glob_match("[a-c]x", "dx") and glob_match("[!a-c]x", "dx") and not glob_match("[!a-c]x", "ax"):
        print("PASS match-class")
    else:
        print("FAIL match-class")
        fails = fails + 1
    if glob_match("a[.b", "a[.b") and not glob_match("[a-", "x"):
        print("PASS match-unterminated-class")
    else:
        print("FAIL match-unterminated-class")
        fails = fails + 1
    # ---- glob walk ----
    # Results are joined with the PLATFORM's separator (path_sep), so the
    # expected spelling is built rather than written as "sandbox\\a.txt" —
    # v0.50.3 hardcoded the Windows spelling here, which pinned glob to one
    # platform in the test while glob itself only ever produced the other.
    hits = glob("sandbox/*.txt")
    if hits.len == 1 and vec_get_str(hits, 0) == "sandbox" + sep + "a.txt":
        print("PASS glob-star-txt")
    else:
        print("FAIL glob-star-txt " + str(hits.len))
        i = 0
        while i < hits.len:
            print("  hit: " + vec_get_str(hits, i))
            i = i + 1
        fails = fails + 1
    allf = glob("sandbox/*")
    if vec_has(allf, "sandbox" + sep + "a.txt") and vec_has(allf, "sandbox" + sep + "sub") and vec_has(allf, "sandbox" + sep + ".hidden") == False:
        print("PASS glob-dotfile-rule")
    else:
        print("FAIL glob-dotfile-rule " + str(allf.len))
        fails = fails + 1
    dots = glob("sandbox/.*")
    if vec_has(dots, "sandbox" + sep + ".hidden"):
        print("PASS glob-explicit-dot")
    else:
        print("FAIL glob-explicit-dot")
        fails = fails + 1
    rec = glob("sandbox/sub/*.txt")
    if rec.len == 1 and vec_get_str(rec, 0) == "sandbox" + sep + "sub" + sep + "c.txt":
        print("PASS glob-subdir")
    else:
        print("FAIL glob-subdir")
        fails = fails + 1
    exact = glob("sandbox/b.ax")
    if exact.len == 1 and vec_get_str(exact, 0) == "sandbox" + sep + "b.ax":
        print("PASS glob-exact")
    else:
        print("FAIL glob-exact")
        fails = fails + 1
    if glob("sandbox/*.zzz").len == 0 and glob("no-such-dir/*.ax").len == 0:
        print("PASS glob-empty")
    else:
        print("FAIL glob-empty")
        fails = fails + 1
    # determinism: sorted output
    some = glob("sandbox/*")
    sorted_twice = True
    i = 1
    while i < some.len:
        if vec_get_str(some, i - 1) > vec_get_str(some, i):
            sorted_twice = False
        i = i + 1
    if sorted_twice:
        print("PASS glob-sorted")
    else:
        print("FAIL glob-sorted")
        fails = fails + 1
    # ---- json file I/O ----
    # v0.50.3 printed only the err code, which cannot separate "the path did
    # not resolve" (wrong cwd) from "the write failed" or "the file held the
    # wrong bytes". The reachability probe and the dumped length answer both
    # in one run.
    p = j_parse("{\"name\":\"aoxn\",\"n\":7,\"tags\":[\"a\",\"b\"]}")
    text = j_dumps(p.dom, p.dom.last)
    if p.err == 0 and j_write_file("sandbox/doc.json", p.dom, p.dom.last):
        print("PASS json-write")
    else:
        print("FAIL json-write err=" + str(p.err) + " len=" + str(len(text)) + " reachable=" + str(j_readable("sandbox/doc.json")))
        fails = fails + 1
    p2 = j_read_file("sandbox/doc.json")
    if p2.err == 0 and j_get_str(p2.dom, p2.dom.last, "name", "?") == "aoxn" and j_get_int(p2.dom, p2.dom.last, "n", -1) == 7:
        print("PASS json-read")
    else:
        print("FAIL json-read err=" + str(p2.err) + " name=" + j_get_str(p2.dom, p2.dom.last, "name", "?") + " n=" + str(j_get_int(p2.dom, p2.dom.last, "n", -1)))
        fails = fails + 1
    arr = j_obj_get(p2.dom, p2.dom.last, "tags")
    if j_len(p2.dom, arr) == 2 and j_arr_str(p2.dom, arr, 1, "?") == "b":
        print("PASS json-read-arr")
    else:
        print("FAIL json-read-arr")
        fails = fails + 1
    miss = j_read_file("sandbox/definitely-missing.json")
    if miss.err == 2:
        print("PASS json-read-missing")
    else:
        print("FAIL json-read-missing " + str(miss.err))
        fails = fails + 1
    # ---- cleanup ----
    os_remove("sandbox/a.txt")
    os_remove("sandbox/b.ax")
    os_remove("sandbox/sub/c.txt")
    os_remove("sandbox/.hidden")
    os_remove("sandbox/doc.json")
    os_rmdir("sandbox/sub")
    os_rmdir("sandbox")
    if not os_exists("sandbox"):
        print("PASS cleanup")
    else:
        print("FAIL cleanup")
        fails = fails + 1
    print("DONE fails=" + str(fails))
    return fails

"#;

#[test]
fn os_glob_filesystem() {
    if aoxn::find_clang().is_none() {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    }
    // the drivers import stdlib modules BY NAME, which exercises the
    // AOXN_STDLIB resolution path (env -> <root>/lib/stdlib -> checkout)
    std::env::set_var("AOXN_STDLIB", abs("stdlib"));
    let dir = std::env::temp_dir().join(format!("aoxn-os_glob_filesystem-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let _ = std::fs::remove_dir_all(&dir.join("sandbox"));

    let src_path = dir.join("os_glob_filesystem.ax");
    let exe = dir.join(format!("os_glob_filesystem{EXE}"));
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
        "attr-file",
        "attr-dir",
        "listdir",
        "listdir-missing",
        "copy",
        "rename",
        "remove",
        "copy-fail-if-exists",
        "env-set-get",
        "env-unset",
        "env-path",
        "env-long-value",
        "chdir",
        "cwd-answers-own-their-buffer",
        "match-star-q",
        "match-multi-star",
        "match-class",
        "match-unterminated-class",
        "glob-star-txt",
        "glob-dotfile-rule",
        "glob-explicit-dot",
        "glob-subdir",
        "glob-exact",
        "glob-empty",
        "glob-sorted",
    ] {
        assert!(
            text.contains(&format!("PASS {marker}")),
            "missing PASS {marker}
{text}"
        );
    }
}
