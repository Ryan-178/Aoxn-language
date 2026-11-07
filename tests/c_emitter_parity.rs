//! Genuine Rust-emitter vs Aoxn-emitter parity, not the selfhost fixed point.
//! Deliberately excludes features the selfhost checker/emitter cannot support.
use std::{fs, path::PathBuf, process::Command};

const CASES: &[(&str, &str)] = &[
    (
        "fstring_shared_arena_growth",
        r#"def main() -> int:
    x = 1
    a = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
         17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32,
         33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48,
         49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64]
    print(f"sum={x+x+x+x+x+x+x+x+x+x+x+x+x+x+x+x+x+x+x+x+x+x+x+x+x+x+x+x+x+x+x+x+x+x+x+x+x+x+x+x}")
    print(f"next={a[0] + x}; again={x}")
    assert(True, "arena remains aligned")
    return 0
"#,
    ),
    ("assert_failure_plain", "def main():\n    assert(\n        False\n    )\n"),
    ("assert_failure", "def main():\n    assert(False, \"boom\")\n"),
    ("assert_lazy_message", "def message() -> string:\n    print(\"unexpected\")\n    return \"bad\"\ndef main():\n    assert(True, message())\n"),
    ("argv", "def main() -> int:\n    print(argc())\n    print(arg(-1))\n    print(arg(0))\n    print(arg(argc()))\n    return 0\n"),
    ("exit", "def main():\n    exit(7)\n"),
    ("assertions", "def main() -> int:\n    assert(True)\n    assert(\n        argc() >= 0,\n        arg(0) + \" message\"\n    )\n    return 0\n"),
    ("generic_assert", "def checked[T](x: T) -> T:\n    assert(True, \"generic\")\n    return x\ndef main() -> int:\n    return checked(2)\n"),
    ("to_int", "def main() -> int:\n    return to_int(1.25)\n"),
    ("to_float", "def main() -> int:\n    print(to_float(7))\n    return 0\n"),
    (
        "nullable_basics",
        r#"def take(x: int | None) -> int:
    if x is None:
        return 0
    return x + 1

def main() -> int:
    x: int | None = None
    print(x is None)
    x = 7
    print(x is None)
    if x is not None:
        print(x + 1)
    a = [1, None, 3]
    if a[1] is None:
        print(take(None))
    print(take(5))
    return take(x)
"#,
    ),
    (
        "dict_basics",
        r#"def main() -> int:
    d: dict[int] = {}
    d["a"] = 1
    d["b"] = 2
    alias = d
    alias["c"] = 3
    print(len(d))
    print(d["a"] + d["c"])
    if dict_has(d, "b"):
        print("has")
    dict_del(d, "b")
    print(len(d))
    for k in d:
        print(k)
    print(d["missing"])
    return 0
"#,
    ),
    (
        "fnptr_basics",
        r#"def twice(x: int) -> int:
    return x * 2

def add2(x: int) -> int:
    return x + 2

def pick(which: bool) -> fn(int) -> int:
    if which:
        return twice
    return add2

def apply(f: fn(int) -> int, x: int) -> int:
    return f(x)

def main() -> int:
    cb: fn(int) -> int = twice
    print(cb(21))
    p = to_int(cb)
    cb2 = p as fn(int) -> int
    print(cb2(5))
    g: fn(int) -> int = pick(True)
    print(g(3))
    h = pick(False)
    print(h(3))
    print(apply(twice, 7))
    return 0
"#,
    ),
    (
        "raise_try",
        r#"def leaf(n: int) -> int:
    if n < 0:
        raise "negative"
    return n

def caller(n: int) -> int:
    return leaf(n)

def guarded(n: int) -> int:
    x = 0
    try:
        x = leaf(n)
    except as e:
        print("caught " + e)
        x = 7
    return x

def main() -> int:
    try:
        print(caller(1))
        print(caller(-1))
    except as e:
        print("outer " + e)
    try:
        raise "inner"
    except:
        print("bare")
    return guarded(-5)
"#,
    ),
    (
        "field_augassign",
        "struct P:\n    x: int\ndef main() -> int:\n    p = P(x=1)\n    p.x += 1\n    return p.x\n",
    ),
    (
        "reverse_extern",
        r#"extern def zebra(x: int) -> int
extern def alpha(x: float) -> float
def main() -> int:
    return 0
"#,
    ),
    (
        "control_flow",
        r#"def fib(n: int) -> int:
    if n < 2:
        return n
    return fib(n - 1) + fib(n - 2)
def main() -> int:
    n = 0
    for i in range(1, 8, 2):
        if i == 3:
            continue
        n += i
    while n > 0:
        n -= 1
        if n == 2:
            break
    if not n == 0 and n < 3:
        print(fib(6))
    elif n == 0:
        print(False)
    else:
        pass
    return 0
"#,
    ),
    (
        "aggregate_rep",
        r#"struct Pair:
    x: int
    label: string
def bump(p: Pair) -> Pair:
    p.x = p.x + 1
    return p
def row(a: [int; 3]) -> [int; 3]:
    a[1] = a[1] + 2
    return a
def main() -> int:
    p = Pair(x=4, label="p")
    q = bump(p)
    a = [2] * 3
    b = [p] * 2
    c = ["s"] * 2
    d = [1.25] * 2
    m = [[1, 2, 3], [4, 5, 6]]
    a = row(a)
    for r in m:
        print(r[1])
    print(q.x)
    print(b[0].label)
    print(c[1])
    print(d[0])
    return a[0]
"#,
    ),
    (
        "strings_float",
        r#"def main() -> int:
    s = "quote\" slash\\ tab\t line\n cr\r"
    t = "世界"
    print(s + t)
    print(s < t)
    print(s == t)
    print(len(t))
    x = 3.25
    y = -0.125
    print(x + y)
    print(str(x))
    print(str(42))
    print(str(True))
    print(f"x={x}; y={y}; n={42}; b={False}; s={t}")
    return 0
"#,
    ),
];

#[test]
fn c_emitters_match_complete_bytes() {
    run_comparison(false);
}

#[test]
fn known_gap_selfhost_frontend_and_float_serialization() {
    run_comparison(true);
}

// Known gaps are exact expectations, never ignored tests or allowed arbitrary failures.
const KNOWN_GAPS: &[(&str, &str, &str)] = &[
    (
        "index_augassign",
        "def main() -> int:\n    a = [1, 2]\n    a[0] += 1\n    return a[0]\n",
        "missing expression",
    ),
    (
        "nul",
        r#"def main() -> int:
    print("nul\0tail")
    return 0
"#,
        "",
    ),
    (
        "float_spelling",
        "def main() -> int:\n    print(3.2500)\n    return 0\n",
        "",
    ),
];

const BUILTIN_REJECTIONS: &[(&str, &str, &str)] = &[
    (
        "arg_many",
        "def main():\n    arg(0, 1)\n",
        "arg expects exactly 1 positional argument (index: int)",
    ),
    (
        "exit_many",
        "def main():\n    exit(0, 1)\n",
        "exit expects exactly 1 positional argument (code: int)",
    ),
    (
        "assert_many",
        "def main():\n    assert(True, \"x\", \"y\")\n",
        "assert expects (condition: bool[, message: string])",
    ),
    (
        "assert_named_message",
        "def main():\n    assert(True, message=\"x\")\n",
        "assert expects (condition: bool[, message: string])",
    ),
    (
        "argc_arity",
        "def main() -> int:\n    return argc(1)\n",
        "argc expects no arguments",
    ),
    (
        "arg_arity",
        "def main() -> int:\n    arg()\n    return 0\n",
        "arg expects exactly 1 positional argument (index: int)",
    ),
    (
        "arg_named",
        "def main() -> int:\n    arg(index=0)\n    return 0\n",
        "arg expects exactly 1 positional argument (index: int)",
    ),
    (
        "arg_type",
        "def main() -> int:\n    arg(True)\n    return 0\n",
        "arg index must be int, found bool",
    ),
    (
        "exit_arity",
        "def main():\n    exit()\n",
        "exit expects exactly 1 positional argument (code: int)",
    ),
    (
        "exit_named",
        "def main():\n    exit(code=0)\n",
        "exit expects exactly 1 positional argument (code: int)",
    ),
    (
        "exit_type",
        "def main():\n    exit(False)\n",
        "exit code must be int, found bool",
    ),
    (
        "assert_arity",
        "def main():\n    assert()\n",
        "assert expects (condition: bool[, message: string])",
    ),
    (
        "assert_named",
        "def main():\n    assert(condition=True)\n",
        "assert expects (condition: bool[, message: string])",
    ),
    (
        "assert_condition",
        "def main():\n    assert(1)\n",
        "assert condition must be bool, found int",
    ),
    (
        "assert_message",
        "def main():\n    assert(True, 2)\n",
        "assert message must be string, found int",
    ),
    (
        "main_params",
        "def main(n: int) -> int:\n    return n\n",
        "'main' cannot take parameters; read the command line with argc() / arg(i)",
    ),
    (
        "main_return",
        "def main() -> bool:\n    return True\n",
        "'main' must return int (the exit code) or void, found bool",
    ),
    (
        "empty_dict_unannotated",
        "def main():\n    d = {}\n",
        "an empty dict literal needs an annotation: d: dict[int] = {}",
    ),
    (
        "dict_key_type",
        "def main():\n    d: dict[int] = {}\n    d[1] = 2\n",
        "dict key must be a string, found int",
    ),
    (
        "nullable_operator",
        "def main() -> int:\n    x: int | None = None\n    return x + 1\n",
        "the left operand is int | None: a nullable value cannot be used here — test it with `is None` / `is not None` first",
    ),
    (
        "raise_payload_type",
        "def main():\n    raise 1\n",
        "raise requires a string message, found int",
    ),
    (
        "fnptr_arity",
        "def one(x: int) -> int:\n    return x\ndef main():\n    cb: fn(int) -> int = one\n    cb(1, 2)\n",
        "function pointer 'cb' expects 1 argument(s), found 2",
    ),
];

fn run_comparison(known_gaps: bool) {
    let Some(clang) = aoxn::find_clang() else {
        assert!(
            std::env::var_os("AOXN_TEST_REQUIRE_CLANG").is_none(),
            "clang is required for this test run; set AOXN_CLANG"
        );
        eprintln!("skipping: clang unavailable (set AOXN_CLANG)");
        return;
    };
    assert!(Command::new(&clang)
        .arg("--version")
        .output()
        .unwrap()
        .status
        .success());
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // Keep fixtures ASCII and two levels below the checkout for relative imports.
    let dir = root.join("target").join(format!(
        "c-emitter-parity-{}-{}",
        std::process::id(),
        known_gaps
    ));
    fs::create_dir_all(&dir).unwrap();
    let driver = dir.join("driver.ax");
    fs::write(
        &driver,
        r#"import * from "../../selfhost/driver.ax"
def main() -> int:
    assert(not shell_path_safe(""))
    assert(not shell_path_safe("bad\npath"))
    assert(shell_path_safe("safe path.obj"))
    if target_os() == "windows":
        assert(not shell_path_safe("bad%PATH%.obj"))
        assert(not shell_path_safe("bad!.obj"))
        assert(not shell_path_safe("bad\".obj"))
        assert(not shell_path_safe("bad&.obj"))
        assert(shell_quote("safe path.obj") == "\"safe path.obj\"")
        assert(shell_quote("-dash.obj") == "\"./-dash.obj\"")
    else:
        assert(shell_quote("a'b.obj") == "'a'\"'\"'b.obj'")
        assert(shell_quote("-dash.obj") == "'./-dash.obj'")
    d = emit_c("input.ax", "selfhost.c")
    if d.err != 0:
        print(d.errmsg)
        return 1
    return 0
"#,
    )
    .unwrap();
    let exe = dir.join("driver").with_extension(aoxn::platform::exe_ext());
    aoxn::build_paths_exe(&[driver.display().to_string()], &exe, true)
        .expect("build Aoxn emit_c driver");
    let mut failures = Vec::new();
    let cases: Vec<_> = if known_gaps {
        KNOWN_GAPS.to_vec()
    } else {
        CASES
            .iter()
            .copied()
            .chain(std::iter::once(("stdlib_use", STDLIB_USE_PROG)))
            .map(|(name, source)| (name, source, ""))
            .collect()
    };
    for (name, source, rejection) in cases {
        let input = dir.join("input.ax");
        fs::write(&input, source).unwrap();
        let rust = aoxn::compile_paths_to_c(&[input.display().to_string()], true)
            .unwrap_or_else(|e| panic!("{name}: Rust emitter failed: {e:?}"));
        let output = dir.join("selfhost.c");
        let _ = fs::remove_file(&output); // stale output must never satisfy the test
        let out = Command::new(&exe).current_dir(&dir).output().unwrap();
        if !rejection.is_empty() {
            assert_eq!(
                out.status.code(),
                Some(1),
                "{name}: expected selfhost rejection"
            );
            assert_eq!(out.stdout, format!("{rejection}\n").as_bytes(), "{name}");
            assert!(!output.exists(), "{name}: rejected input must not emit C");
            continue;
        }
        assert!(
            out.status.success(),
            "{name}: emit_c failed: {:?}\n{}\n{}; artifacts: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
            dir.display()
        );
        let selfhost = fs::read(&output).expect("emit_c did not write C");
        if known_gaps {
            // Pin the *entire* current difference, not merely inequality. A fix
            // must promote the fixture to the parity corpus, not disappear here.
            let (rust_fragment, selfhost_fragment) = match name {
                "nul" => (r#""nul\000tail""#, r#""nultail""#),
                "float_spelling" => ("3.25", "3.2500"),
                _ => unreachable!(),
            };
            assert_eq!(rust.matches(rust_fragment).count(), 1, "{name}");
            let expected = rust.replacen(rust_fragment, selfhost_fragment, 1);
            assert_ne!(
                rust.as_bytes(),
                selfhost,
                "{name}: gap closed; promote to parity"
            );
            assert_eq!(
                expected.as_bytes(),
                selfhost,
                "{name}: unexpected additional drift"
            );
            continue;
        }
        if matches!(
            name,
            "argv" | "exit" | "assert_failure" | "assert_failure_plain" | "assert_lazy_message"
        ) {
            let native = dir
                .join("builtin")
                .with_extension(aoxn::platform::exe_ext());
            let compiled = Command::new(&clang)
                .arg("-w")
                .arg(&output)
                .arg("-o")
                .arg(&native)
                .output()
                .unwrap();
            assert!(
                compiled.status.success(),
                "{name}: {}",
                String::from_utf8_lossy(&compiled.stderr)
            );
            let run = Command::new(&native).arg("hello world").output().unwrap();
            let (code, stdout) = match name {
                "argv" => (0, "1\n\nhello world\n\n"),
                "exit" => (7, ""),
                "assert_failure" => (1, "assertion failed at line 2: boom\n"),
                "assert_failure_plain" => (1, "assertion failed at line 2\n"),
                _ => (0, ""),
            };
            assert_eq!(run.status.code(), Some(code), "{name}");
            assert_eq!(run.stdout, stdout.as_bytes(), "{name}");
            if name == "argv" {
                let noargs = Command::new(&native).output().unwrap();
                assert_eq!(noargs.status.code(), Some(0));
                assert_eq!(noargs.stdout, b"0\n\n\n\n");
            }
        }
        if rust.as_bytes() != selfhost {
            let offset = rust
                .as_bytes()
                .iter()
                .zip(&selfhost)
                .position(|(a, b)| a != b)
                .unwrap_or(rust.len().min(selfhost.len()));
            fs::write(dir.join(format!("{name}.rust.c")), &rust).unwrap();
            fs::write(dir.join(format!("{name}.selfhost.c")), &selfhost).unwrap();
            failures.push(format!(
                "{name}: first differing byte {offset}; lengths Rust={} selfhost={}",
                rust.len(),
                selfhost.len()
            ));
        }
    }
    if !known_gaps {
        for (name, source, rejection) in BUILTIN_REJECTIONS {
            let input = dir.join("input.ax");
            fs::write(&input, source).unwrap();
            let errors =
                aoxn::compile_paths_to_c(&[input.display().to_string()], true).expect_err(name);
            assert!(
                errors.iter().any(|e| e.message == *rejection),
                "{name}: {errors:?}"
            );
            let output = dir.join("selfhost.c");
            let _ = fs::remove_file(&output);
            let out = Command::new(&exe).current_dir(&dir).output().unwrap();
            assert_eq!(out.status.code(), Some(1), "{name}");
            assert_eq!(out.stdout, format!("{rejection}\n").as_bytes(), "{name}");
            assert!(!output.exists(), "{name}: rejected program emitted C");
        }
    }
    assert!(
        failures.is_empty(),
        "{}\nUnmodified C artifacts: {}",
        failures.join("\n"),
        dir.display()
    );
    fs::remove_dir_all(dir).unwrap();
}

// Private copy of the existing pipeline aggregate/generic fixture.
const STDLIB_USE_PROG: &str = r#"import * from "../../stdlib/stdlib.ax"

struct Grid:
    cells: [[int; 3]; 2]
    name: string

def sum_row(row: [int; 3]) -> int:
    total = 0
    for x in row:
        total = total + x
    return total

def grid_total(grid: Grid) -> int:
    acc = 0
    for row in grid.cells:
        acc = acc + sum_row(row)
    return acc

def main() -> int:
    a = [5, 3, 8, 1]
    s = sort(a)
    print(s[0])
    print(s[3])
    print(binary_search(s, 5))
    print(sum_int(a))
    print(len(a))
    w = ["pear", "apple"]
    sw = sort(w)
    print(sw[0])
    print(sw[1])
    b = [0] * 3
    b[1] = 7
    total = 0
    for x in b:
        total = total + x
    print(total)
    v = vec_new()
    v = vec_push(v, 10)
    v = vec_push(v, 20)
    print(vec_get(v, 0) + vec_get(v, 1))
    print(is_alpha(65))
    print(is_digit(97))
    print(str_get("hello", 1))
    print(hypot(3.0, 4.0))
    m = [[1, 2, 3], [4, 5, 6]]
    print(m[0][0])
    print(m[1][2])
    m[1][0] = 40
    print(m[1][0])
    print(len(m))
    print(len(m[0]))
    for row in m:
        print(sum_row(row))
    print(sum_row([1, 2, 3]))
    g = Grid(cells=[[7, 8, 9], [10, 11, 12]], name="g")
    print(g.name)
    print(g.cells[1][1])
    print(sum_row(g.cells[1]))
    print(grid_total(g))
    h = g
    h.cells[0][0] = 99
    print(g.cells[0][0])
    print(h.cells[0][0])
    print(grid_total(g))
    print(grid_total(h))
    words = ["pear", "apple", "fig"]
    sorted_words = sort(words)
    print(sorted_words[0])
    print(sorted_words[2])
    print(sort([5, 3, 8, 1])[0])
    return 0
"#;
