//! Regression pins for the v0.50.0 stdlib audit: the second `len()`-is-
//! `strlen` sweep, and the byte-sink rewrites that followed it.
//!
//! What is asserted here is CORRECTNESS (a crash, a wrong answer, or a
//! byte-different string fails the run). The perf numbers quoted in the
//! source comments were measured on this machine with the probes described
//! in CHANGELOG; a re-introduced quadratic loop would show up as this
//! suite getting slow, not as a hard failure.
//!
//! The SDK joiners (`an_msg_text` / `oa_responses_text` and the request
//! bodies) are pinned in anthropic_sdk.rs / openai_sdk.rs, which are
//! Windows-gated because they link winhttp.

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
import * from "stdlib.ax"
import * from "stdlib/os.ax"
import * from "stdlib/pathlib.ax"
import * from "stdlib/glob.ax"
import * from "stdlib/datetime.ax"
import * from "stdlib/json.ax"
import * from "stdlib/net/codec.ax"

def main() -> int:
    fails = 0

    # ---- 1. datetime_format: same bytes as the old per-char concat ----
    dt = DateTime(year=2026, month=10, day=10, hour=12, minute=34, second=56)
    f = datetime_format(dt, "%Y-%m-%d %H:%M:%S")
    if f == "2026-10-10 12:34:56":
        print("PASS datetime-format-bytes")
    else:
        print("FAIL datetime-format-bytes got=" + f)
        fails = fails + 1
    g = datetime_format(dt, "%A %a %B %b %j %%")
    if len(g) > 0 and str_get(g, len(g) - 1) == 37:
        print("PASS datetime-format-literals")
    else:
        print("FAIL datetime-format-literals got=" + g)
        fails = fails + 1
    # an empty format is still an empty string
    if datetime_format(dt, "") == "":
        print("PASS datetime-format-empty")
    else:
        print("FAIL datetime-format-empty got=" + datetime_format(dt, ""))
        fails = fails + 1

    # ---- 2. path_norm: root shapes survive the sink rewrite ----------
    n1 = path_norm("C:\\a\\b\\..\\c")
    n2 = path_norm("\\\\srv\\share\\x")
    n3 = path_norm("a\\.\\b")
    n4 = path_norm("..")
    n5 = path_norm("")
    if n1 == "C:\\a\\c" and n2 == "\\\\srv\\share\\x" and n3 == "a\\b" and n4 == ".." and n5 == ".":
        print("PASS path-norm-shapes")
    else:
        print("FAIL path-norm-shapes n1=" + n1 + " n2=" + n2 + " n3=" + n3 + " n4=" + n4 + " n5=" + n5)
        fails = fails + 1

    # ---- 3. json numbers: the buf_str extraction is byte-correct -----
    numdoc = "{\"a\":123456789,\"b\":-42,\"c\":1.5e3,\"d\":0.125,\"e\":-1e-2}"
    pn = j_parse(numdoc)
    ok = pn.err == 0
    ok = ok and j_get_int(pn.dom, pn.dom.last, "a", 0) == 123456789
    ok = ok and j_get_int(pn.dom, pn.dom.last, "b", 0) == -42
    ok = ok and j_get_float(pn.dom, pn.dom.last, "c", 0.0) == 1500.0
    ok = ok and j_get_float(pn.dom, pn.dom.last, "d", 0.0) == 0.125
    ok = ok and j_get_float(pn.dom, pn.dom.last, "e", 0.0) < 0.0
    if ok:
        print("PASS json-numbers")
    else:
        print("FAIL json-numbers a=" + str(j_get_int(pn.dom, pn.dom.last, "a", 0)) + " c=" + str(j_get_float(pn.dom, pn.dom.last, "c", 0.0)))
        fails = fails + 1
    # strings and keys are unaffected by the extraction change
    sd = "{\"k\":\"v\\n\",\"arr\":[1,\"two\",true,null]}"
    ps = j_parse(sd)
    if ps.err == 0 and j_get_str(ps.dom, ps.dom.last, "k", "?") == "v\n" and j_len(ps.dom, j_obj_get(ps.dom, ps.dom.last, "arr")) == 4:
        print("PASS json-strings-keys")
    else:
        print("FAIL json-strings-keys err=" + str(ps.err))
        fails = fails + 1

    # ---- 4. percent-encoding: same bytes, longer strings ------------
    enc = net_url_encode("a b/c?d=e&f")
    if enc == "a%20b%2Fc%3Fd%3De%26f":
        print("PASS url-encode-bytes")
    else:
        print("FAIL url-encode-bytes got=" + enc)
        fails = fails + 1
    longv = ""
    i = 0
    while i < 500:
        longv = longv + "xy"
        i = i + 1
    e2 = net_url_encode(longv)
    if len(e2) == 1000 and str_get(e2, 0) == 120 and str_get(e2, 999) == 121:
        print("PASS url-encode-long")
    else:
        print("FAIL url-encode-long len=" + str(len(e2)))
        fails = fails + 1
    # pairs join with & and =
    pk = vec_new()
    pv = vec_new()
    pk = vec_push_str(pk, "a")
    pv = vec_push_str(pv, "1 2")
    pk = vec_push_str(pk, "b")
    pv = vec_push_str(pv, "x&y")
    pj = net_url_encode_pairs(pk, pv)
    if pj == "a=1%202&b=x%26y":
        print("PASS url-encode-pairs")
    else:
        print("FAIL url-encode-pairs got=" + pj)
        fails = fails + 1

    # ---- 5. UTF-16 encoders: byte counts, incl. surrogate pairs ------
    # os.utf16_write and net.utf16_write are the same algorithm; the two
    # copies must stay byte-identical (they were both quadratic before
    # the v0.50.0 hoist).
    plain = "hello"
    wide = "é€😀"
    pb = malloc(64)
    wb = malloc(64)
    u1 = os_utf16_write(plain, pb)
    u2 = net_utf16_write(plain, wb)
    same = u1 == u2 and u1 == 10
    i = 0
    while i < u1:
        if load_u8(pb, i) != load_u8(wb, i):
            same = False
        i = i + 1
    u3 = os_utf16_write(wide, pb)
    # "é" 2 bytes UTF-8 -> 2 bytes UTF-16; "€" 3 -> 2; "😀" 4 -> surrogate pair 4
    if same and u3 == 8:
        print("PASS utf16-counts")
    else:
        print("FAIL utf16-counts u1=" + str(u1) + " u2=" + str(u2) + " u3=" + str(u3))
        fails = fails + 1

    # ---- 6. str_sub / buf_str / StrBuf primitives -------------------
    s = "abcdef"
    if str_sub(s, 2, 4) == "cd" and str_sub(s, 4, 99) == "ef" and str_sub(s, 99, 200) == "" and str_sub(s, -5, 2) == "ab":
        print("PASS str-sub-clamps")
    else:
        print("FAIL str-sub-clamps " + str_sub(s, 2, 4) + "|" + str_sub(s, 4, 99) + "|" + str_sub(s, 99, 200) + "|" + str_sub(s, -5, 2))
        fails = fails + 1
    raw = malloc(8)
    store_u8(raw, 0, 104)
    store_u8(raw, 1, 105)
    store_u8(raw, 2, 0)
    store_u8(raw, 3, 88)
    if buf_str(raw, 2) == "hi" and buf_str(raw, 0) == "" and buf_str(raw, 3) == "hi":
        print("PASS buf-str")
    else:
        print("FAIL buf-str [" + buf_str(raw, 2) + "][" + buf_str(raw, 0) + "][" + buf_str(raw, 3) + "]")
        fails = fails + 1
    w = sb_new(4)
    w = sb_str(w, "abc")
    w = sb_byte(w, 33)
    w = sb_str(w, "defghijklmnopqrstuvwxyz")
    t = sb_text(w)
    if t == "abc!defghijklmnopqrstuvwxyz" and len(t) == 27:
        print("PASS strbuf-grow")
    else:
        print("FAIL strbuf-grow [" + t + "] len=" + str(len(t)))
        fails = fails + 1

    # ---- 7. glob: hoisted bounds did not change the matcher --------
    if glob_match("*.ax", "hello.ax") and not glob_match("*.ax", "hello.txt") and glob_match("h?llo*", "hello world") and glob_match("[a-c]at", "bat") and not glob_match("[a-c]at", "dat") and glob_match("*", "anything"):
        print("PASS glob-match-hoisted")
    else:
        print("FAIL glob-match-hoisted")
        fails = fails + 1
    segs = glob_split("a\\b\\c")
    if segs.len == 3 and vec_get_str(segs, 0) == "a" and vec_get_str(segs, 2) == "c":
        print("PASS glob-split")
    else:
        print("FAIL glob-split len=" + str(segs.len))
        fails = fails + 1

    # ---- 8. sse: buf_str extraction keeps the line bytes ------------
    # (a header line through the pull filter; the buffer contains MORE
    # than the line, which is what made str_sub scan it per line)
    buf = malloc(64)
    line = "data: {\"i\":7}"
    i = 0
    while i < len(line):
        store_u8(buf, i, load_u8(line, i))
        i = i + 1
    store_u8(buf, len(line), 10)
    store_u8(buf, len(line) + 1, 88)
    got = buf_str(buf, len(line))
    if got == line:
        print("PASS sse-line-bytes")
    else:
        print("FAIL sse-line-bytes got=[" + got + "]")
        fails = fails + 1

    print("DONE fails=" + str(fails))
    return fails
"#;

#[test]
fn stdlib_perf_pins() {
    if aoxn::find_clang().is_none() {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    }
    // the drivers import stdlib modules BY NAME, which exercises the
    // AOXN_STDLIB resolution path (env -> <root>/lib/stdlib -> checkout)
    std::env::set_var("AOXN_STDLIB", abs("stdlib"));
    let dir = std::env::temp_dir().join(format!("aoxn-stdlib_perf_pins-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let src_path = dir.join("stdlib_perf_pins.ax");
    let exe = dir.join(format!("stdlib_perf_pins{EXE}"));
    std::fs::write(&src_path, SRC).unwrap();
    aoxn::build_paths_opts(&[src_path.display().to_string()], &exe, true, &[], &[])
        .unwrap_or_else(|d| panic!("driver failed to compile: {d:?}"));
    let out = Command::new(&exe).current_dir(&dir).output().expect("failed to run driver");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "driver exited {:?}\nstdout:\n{text}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("DONE fails=0"), "driver reported failures:\n{text}");
    assert!(!text.contains("FAIL"), "driver printed FAIL:\n{text}");
    for marker in [
        "datetime-format-bytes",
        "datetime-format-literals",
        "datetime-format-empty",
        "path-norm-shapes",
        "json-numbers",
        "json-strings-keys",
        "url-encode-bytes",
        "url-encode-long",
        "url-encode-pairs",
        "utf16-counts",
        "str-sub-clamps",
        "buf-str",
        "strbuf-grow",
        "glob-match-hoisted",
        "glob-split",
        "sse-line-bytes",
    ] {
        assert!(
            text.contains(&format!("PASS {marker}")),
            "missing PASS {marker}\n{text}"
        );
    }
}
