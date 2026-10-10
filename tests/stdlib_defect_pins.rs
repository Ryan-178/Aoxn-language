//! Regression pins for the stdlib defects found in the v0.47.0 audit.
//!
//! Each block below corresponds to a defect that was REPRODUCED before the
//! fix and is now measured or asserted to be gone. They are grouped in one
//! file because they share a driver style (a `PASS <name>` marker per check,
//! `DONE fails=0` at the end) and because they are one story: the JSON DOM's
//! builder shrank a parsed node's buffer, and `len()` on a string being
//! `strlen` at every use site made three loops quadratic.
//!
//! What is asserted here is CORRECTNESS (a crash or a wrong answer fails the
//! run). The perf numbers quoted in the source comments were measured on this
//! machine with the probes described in CHANGELOG; a re-introduced quadratic
//! loop would show up as this suite getting slow, not as a hard failure.

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
import * from "stdlib/base64.ax"
import * from "stdlib/os.ax"

# a JSON object with exactly `n` members, as text
def obj_with(n: int) -> string:
    out = "{"
    i = 0
    while i < n:
        if i > 0:
            out = out + ","
        out = out + "\"m" + str(i) + "\":" + str(i)
        i = i + 1
    return out + "}"

# a JSON array with exactly `n` elements, as text
def arr_with(n: int) -> string:
    out = "["
    i = 0
    while i < n:
        if i > 0:
            out = out + ","
        out = out + str(i)
        i = i + 1
    return out + "]"

def main() -> int:
    fails = 0

    # ---- 1. a PARSED object can be extended --------------------------
    # The parser adopts the child Vec's buffer but left the node's stored
    # capacity at 0. `jb_grow_obj` read that as "capacity 8", doubled to 16,
    # and realloc'd a 32-slot (256 byte) buffer DOWN to 128 bytes before
    # writing slot 20 -- 8 bytes past the end. Any parsed object with 16+
    # members died with 0xC0000005 on the first `jb_set`. 20 members crosses
    # that boundary; 8 did not, which is how it stayed latent.
    p = j_parse(obj_with(20))
    dom = p.dom
    root = p.dom.last
    k = 0
    while k < 20:
        w = jb_int(dom, 1000 + k)
        dom = w
        dom = jb_set(dom, root, "x" + str(k), w.last)
        k = k + 1
    if p.err == 0 and j_kind(dom, root) == J_OBJ() and j_ival(dom, root) == 40 and j_get_int(dom, root, "m0", -1) == 0 and j_get_int(dom, root, "m19", -1) == 19 and j_get_int(dom, root, "x0", -1) == 1000 and j_get_int(dom, root, "x19", -1) == 1019:
        print("PASS parsed-object-extend")
    else:
        print("FAIL parsed-object-extend n=" + str(j_ival(dom, root)) + " m0=" + str(j_get_int(dom, root, "m0", -1)) + " x19=" + str(j_get_int(dom, root, "x19", -1)))
        fails = fails + 1

    # ---- 2. same for an ARRAY (the one-buffer arm) -------------------
    q = j_parse(arr_with(20))
    dq = q.dom
    aq = q.dom.last
    k = 0
    while k < 20:
        w = jb_int(dq, 500 + k)
        dq = w
        dq = jb_push(dq, aq, w.last)
        k = k + 1
    if q.err == 0 and j_kind(dq, aq) == J_ARR() and j_ival(dq, aq) == 40 and j_ival(dq, j_arr_get(dq, aq, 0)) == 0 and j_ival(dq, j_arr_get(dq, aq, 19)) == 19 and j_ival(dq, j_arr_get(dq, aq, 20)) == 500 and j_ival(dq, j_arr_get(dq, aq, 39)) == 519:
        print("PASS parsed-array-extend")
    else:
        print("FAIL parsed-array-extend n=" + str(j_ival(dq, aq)) + " first=" + str(j_ival(dq, j_arr_get(dq, aq, 0))) + " last=" + str(j_ival(dq, j_arr_get(dq, aq, 39))))
        fails = fails + 1

    # ---- 3. serializer round trip, byte identical --------------------
    # j_dumps used to build the text by repeated `out = out + ...`, which
    # allocates a fresh buffer per step and abandons the old one: O(n^2)
    # allocations AND O(n^2) leaked bytes (8.6 GB on a 20 000-key response).
    # It is now one pass into a geometrically grown buffer. This document is
    # already in the canonical compact spelling, so dumps(parse(x)) == x --
    # which is what proves the rewritten writer is byte-correct, not merely
    # fast. Covers arrays, ints, escapes, bool, null, float, empty string.
    canon = "{\"a\":[1,2,3],\"b\":\"x\\ny\",\"c\":true,\"d\":null,\"e\":1.5,\"f\":\"q\\\\r\",\"g\":\"\"}"
    r1 = j_roundtrip(canon)
    if r1 == canon:
        print("PASS dumps-roundtrip-canonical")
    else:
        print("FAIL dumps-roundtrip-canonical " + r1)
        fails = fails + 1

    # a wide document: 200 members, one line, no pretty printing
    wide = j_parse(obj_with(200))
    out = j_dumps(wide.dom, wide.dom.last)
    if wide.err == 0 and len(out) == len(obj_with(200)) and out == obj_with(200):
        print("PASS dumps-wide-object")
    else:
        print("FAIL dumps-wide-object len=" + str(len(out)))
        fails = fails + 1

    # nesting survives the rewrite (recursive write-back, not concat)
    deep = j_roundtrip("{\"x\":{\"y\":[{\"z\":[1,[2,[3]]]}]}}")
    if deep == "{\"x\":{\"y\":[{\"z\":[1,[2,[3]]]}]}}":
        print("PASS dumps-nested")
    else:
        print("FAIL dumps-nested " + deep)
        fails = fails + 1

    # ---- 4. the exponent is clamped ---------------------------------
    # The digit run was unbounded, so `1e999999999` spun the scaling loop
    # for minutes -- a DoS on any untrusted JSON (an API response, a user's
    # config file). It must now come back promptly, and ordinary exponents
    # must still be exact.
    huge = j_parse("{\"x\":1e999999999}")
    small = j_parse("{\"x\":1e3}")
    frac = j_parse("{\"x\":1.5e-3}")
    if huge.err == 0 and small.err == 0 and frac.err == 0 and j_get_float(small.dom, small.dom.last, "x", 0.0) == 1000.0 and j_get_float(frac.dom, frac.dom.last, "x", 0.0) == 0.0015:
        print("PASS exponent-clamped")
    else:
        print("FAIL exponent-clamped e3=" + str(j_get_float(small.dom, small.dom.last, "x", 0.0)) + " e-3=" + str(j_get_float(frac.dom, frac.dom.last, "x", 0.0)))
        fails = fails + 1

    # ---- 5. base64 decode -------------------------------------------
    # The loop bound was `len(s)`, i.e. a fresh strlen of the whole input on
    # every iteration and every padding byte: O(n^2) (64 KB took 608 ms and
    # 1.4 MB did not finish in 600 s). Correctness of the rewritten loop,
    # including the padding scan.
    raw = ""
    i = 0
    while i < 3000:
        raw = raw + "A"
        i = i + 1
    enc = b64_encode_str(raw)
    dec = b64_decode(enc)
    same = dec.n == 3000
    if same:
        i = 0
        while i < 3000:
            if load_u8(dec.p, i) != 65:
                same = False
            i = i + 1
    if same:
        print("PASS base64-roundtrip")
    else:
        print("FAIL base64-roundtrip n=" + str(dec.n))
        fails = fails + 1

    # padded input still decodes to the right length
    pad = b64_decode("QUJD")            # "ABC"
    pad2 = b64_decode("QUJDRA==")        # "ABCD"
    if pad.n == 3 and pad2.n == 4:
        print("PASS base64-padding")
    else:
        print("FAIL base64-padding " + str(pad.n) + "/" + str(pad2.n))
        fails = fails + 1

    # invalid input returns n == -1 AND p == 0 -- the buffer used to be
    # abandoned without free(), which leaked on every malformed payload. The
    # length of a base64 body is attacker-chosen (an Authorization header,
    # an SSE line), so this was a real leak on a security boundary.
    bad = b64_decode("!!!!")
    bad2 = b64_decode("QUJD=x")
    badstr = b64_decode_str("!!!!")
    if bad.n == -1 and bad.p == 0 and bad2.n == -1 and bad2.p == 0 and badstr == "":
        print("PASS base64-invalid-frees")
    else:
        print("FAIL base64-invalid-frees p=" + str(bad.p))
        fails = fails + 1

    # ---- 6. os_has_env after a genuine miss --------------------------
    # Win32's last error is thread-sticky and a SUCCESSFUL call does not
    # clear it, so any earlier miss left 203 latched and the next probe of a
    # variable that really exists answered False.
    os_setenv("AOXN_STDLIB_PIN_PRESENT", "1")
    os_setenv("AOXN_STDLIB_PIN_EMPTY", "")
    gone = os_has_env("AOXN_STDLIB_PIN_ABSENT")
    hit = os_has_env("AOXN_STDLIB_PIN_PRESENT")
    empty = os_has_env("AOXN_STDLIB_PIN_EMPTY")
    still_gone = not os_has_env("AOXN_STDLIB_PIN_ABSENT")
    os_unsetenv("AOXN_STDLIB_PIN_PRESENT")
    os_unsetenv("AOXN_STDLIB_PIN_EMPTY")
    if gone == false and hit and empty and still_gone:
        print("PASS has-env-after-miss")
    else:
        print("FAIL has-env-after-miss gone=" + str(gone) + " hit=" + str(hit) + " empty=" + str(empty) + " still_gone=" + str(still_gone))
        fails = fails + 1

    print("DONE fails=" + str(fails))
    return fails

"#;

#[test]
fn stdlib_defect_pins() {
    if aoxn::find_clang().is_none() {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    }
    // the drivers import stdlib modules BY NAME, which exercises the
    // AOXN_STDLIB resolution path (env -> <root>/lib/stdlib -> checkout)
    std::env::set_var("AOXN_STDLIB", abs("stdlib"));
    let dir = std::env::temp_dir().join(format!("aoxn-stdlib_defect_pins-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let src_path = dir.join("stdlib_defect_pins.ax");
    let exe = dir.join(format!("stdlib_defect_pins{EXE}"));
    std::fs::write(&src_path, SRC).unwrap();
    aoxn::build_paths_opts(&[src_path.display().to_string()], &exe, true, &[], &[])
        .unwrap_or_else(|d| panic!("driver failed to compile: {d:?}"));
    let out = Command::new(&exe).current_dir(&dir).output().expect("failed to run driver");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    // A heap-corruption regression shows up here as an abnormal exit (the old
    // code died with 0xC0000005), so the status assert is load-bearing and not
    // just a formality.
    assert!(
        out.status.success(),
        "driver exited {:?}\nstdout:\n{text}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("DONE fails=0"), "driver reported failures:\n{text}");
    assert!(!text.contains("FAIL"), "driver printed FAIL:\n{text}");
    for marker in [
        "parsed-object-extend",
        "parsed-array-extend",
        "dumps-roundtrip-canonical",
        "dumps-wide-object",
        "dumps-nested",
        "exponent-clamped",
        "base64-roundtrip",
        "base64-padding",
        "base64-invalid-frees",
        "has-env-after-miss",
    ] {
        assert!(
            text.contains(&format!("PASS {marker}")),
            "missing PASS {marker}\n{text}"
        );
    }
}
/// A path-BUILDING expression in a cross-platform module may not contain a
/// hardcoded separator.
///
/// v0.50.3, the Linux CI job: `stdlib/glob.ax` and `stdlib/pathlib.ax`
/// spelled every joined path with a literal `"\\"`, so on Linux
/// `glob("sandbox/sub/*.txt")` walked `os_exists("sandbox\\sub")` — false —
/// and returned nothing, while `path_join` handed callers a path that named
/// no file. Four sites: `glob_join`, `path_join`, `path_with_suffix`,
/// `path_with_name`, plus `path_norm`'s join byte.
///
/// What is legal is the module being honest about which platform it serves:
/// a Windows-only module (`ui_win.ax`, `net/http.ax`, `os_listdir`'s own
/// `path + "\\*"` arm) may name `\` freely, and so may a doc comment. This
/// check therefore runs on the modules that are meant to work on BOTH, and
/// it strips `#` comments first. Building uses `path_sep()`, which is the
/// one way to spell a separator here.
#[test]
fn cross_platform_modules_never_hardcode_a_path_separator() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("stdlib");
    // dual-platform modules: these serve Windows AND Linux
    let modules = [
        "stdlib.ax",
        "os.ax",
        "glob.ax",
        "pathlib.ax",
        "math.ax",
        "time.ax",
        "datetime.ax",
        "calendar.ax",
        "base64.ax",
        "hashlib.ax",
        "bisect.ax",
        "heapq.ax",
        "json.ax",
        "ui.ax",
        "ui_draw.ax",
    ];
    // a separator glued onto a path: `... + "\"` or `"\" + ...`
    // (the raw-byte form gets its own rule below, below `for`)
    let needles = [r#"" + "\\""#, r#""\\" +"#];
    for m in modules {
        let path = root.join(m);
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} must exist: {e}", path.display()));
        // strip `#` comments: prose about separators is allowed, code is not
        let code: String = src
            .lines()
            .map(|l| match l.find('#') {
                Some(i) => &l[..i],
                None => l,
            })
            .collect::<Vec<_>>()
            .join("\n");
        for needle in needles {
            assert!(
                !code.contains(needle),
                "{}: hardcoded path separator {needle:?} in cross-platform code.\n\
                 A joined path must be spelled with path_sep() (stdlib/pathlib.ax), \
                 or path_sep_byte() when a raw byte is needed.\n\
                 Found near:\n{}",
                m,
                code.lines()
                    .find(|l| l.contains(needle))
                    .unwrap_or("<unknown>")
                    .trim()
            );
        }

        // the same mistake in raw-byte form, which is what path_norm did
        // (`w = sb_byte(w, 92)`). Scoped to a sink push, because a bare 92
        // is also a perfectly good JSON `\\` escape - json.ax writes one.
        if let Some(bad) = code
            .lines()
            .find(|l| l.contains("sb_byte(") && l.contains(", 92)"))
        {
            panic!(
                "{}: hardcoded separator byte in a byte-sink push — use path_sep_byte():\n{}",
                m,
                bad.trim()
            );
        }
    }
}

/// A dual-platform stdlib module must not reference the OTHER platform's API
/// symbols — and that is checkable from a Windows machine, without a Linux
/// runner, because `target_os()` is a compile-time fold.
///
/// The backend emits `if (__builtin_strcmp("windows", "windows") == 0)`.
/// Rewriting that one string literal to `"linux"` and recompiling with clang
/// performs EXACTLY the fold a Linux build would perform — clang
/// constant-folds `__builtin_strcmp` of two literals in the frontend, so the
/// branch that is not taken never becomes a call. The undefined-symbol table
/// of the resulting object is then the Linux link's problem list, exactly.
///
/// This is how v0.50.7 found the bug: `stdlib/net/http.ax` guards its two
/// public entry points with `if target_os() != "windows": return <error>`,
/// which made THEIR WinHTTP calls dead on Linux — but four helper functions
/// (`net_query_status`, `net_raw_headers`, `net_http_stream_read`,
/// `net_http_stream_close`) called WinHTTP with no guard of their own, and the
/// compiler emits every function body, so `WinHttpQueryHeaders`,
/// `WinHttpQueryDataAvailable`, `WinHttpReadData`, `WinHttpCloseHandle` and
/// `GetLastError` were still referenced. A Linux build of any program
/// importing the module failed to LINK, where the module's own comment and
/// the README both promise a clean "no transport on this platform" error.
///
/// The two probes are separate programs because the resolver correctly
/// refuses to star-import `os.ax` and `net/http.ax` into one namespace (they
/// both declare `GetLastError` and `Sleep`). That refusal is correct
/// behaviour, not a bug — a program picks one.
#[test]
fn dual_platform_modules_reference_no_foreign_platform_symbols() {
    let Some(clang) = aoxn::find_clang() else {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    };
    let dir = std::env::temp_dir().join(format!("aoxn-linuxsim-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let probes: [(&str, &str); 2] = [
        (
            "core",
            r#"import * from "stdlib.ax"
import * from "stdlib/os.ax"
import * from "stdlib/glob.ax"
import * from "stdlib/pathlib.ax"
import * from "stdlib/time.ax"
import * from "stdlib/datetime.ax"
import * from "stdlib/calendar.ax"
import * from "stdlib/json.ax"
import * from "stdlib/base64.ax"
import * from "stdlib/hashlib.ax"
import * from "stdlib/bisect.ax"
import * from "stdlib/heapq.ax"
import * from "stdlib/math.ax"

def main() -> int:
    return 0
"#,
        ),
        (
            "net",
            r#"import * from "stdlib/net/http.ax"
import * from "stdlib/net/codec.ax"
import * from "stdlib/net/sse.ax"

def main() -> int:
    return 0
"#,
        ),
    ];

    // the Win32 surface the dual-platform stdlib is allowed to reach for
    const FOREIGN: &[&str] = &[
        "WinHttp", "GetLocalTime", "QueryPerformance", "GetSystemTimeAsFileTime",
        "GetTickCount64", "Sleep", "GetLastError", "SetLastError",
        "CreateDirectoryW", "RemoveDirectoryW", "DeleteFileW", "CopyFileW",
        "MoveFileW", "GetFileAttributesW", "GetEnvironmentVariableW",
        "SetEnvironmentVariableW", "GetCurrentDirectoryW",
        "SetCurrentDirectoryW", "FindFirstFileW", "FindNextFileW", "FindClose",
        "GetModuleFileName", "GetModuleHandle",
    ];

    for (name, src) in probes {
        let ax = dir.join(format!("{name}.ax"));
        std::fs::write(&ax, src).unwrap();
        let c = aoxn::compile_paths_to_c(&[ax.display().to_string()], true)
            .unwrap_or_else(|d| panic!("{name}: emit failed: {d:?}"));

        // perform the fold a Linux build would
        let windows = r#"__builtin_strcmp("windows", "windows")"#;
        let linux = r#"__builtin_strcmp("windows", "linux")"#;
        assert!(
            c.contains(windows),
            "{name}: the backend no longer emits the target_os fold this test \
             rewrites ({windows:?}); the check is void until it is updated"
        );
        let simulated = c.replace(windows, linux);

        let cpath = dir.join(format!("{name}_linux.c"));
        std::fs::write(&cpath, simulated).unwrap();
        let obj = dir.join(format!("{name}_linux{}", if cfg!(windows) { ".obj" } else { ".o" }));
        let status = std::process::Command::new(&clang)
            .arg("-O1")
            .arg("-w")
            .arg("-c")
            .arg("-o")
            .arg(&obj)
            .arg(&cpath)
            .status()
            .expect("failed to run clang");
        assert!(status.success(), "{name}: clang refused the simulated C");
        let bytes = std::fs::read(&obj).expect("object");
        // symbol names sit in the object's string table in plain bytes on
        // both COFF and ELF, so a substring scan needs no nm/objdump
        let text = String::from_utf8_lossy(&bytes).into_owned();
        for sym in FOREIGN {
            assert!(
                !text.contains(sym),
                "{name}: a simulated-Linux object still references {sym:?}. \
                 Every function that calls a platform API must branch on \
                 target_os() ITSELF — the compiler emits every function body, \
                 so a helper reached from a guarded caller still leaks its \
                 platform's symbols into the link."
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
