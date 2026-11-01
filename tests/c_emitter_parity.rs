//! Genuine Rust-emitter vs Aoxn-emitter parity, not the selfhost fixed point.
//! Deliberately excludes features the selfhost checker/emitter cannot support.
use std::{fs, path::PathBuf, process::Command};

const CASES: &[(&str, &str)] = &[
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
        "to_int",
        "def main() -> int:\n    return to_int(1.25)\n",
        "call to undefined function or struct 'to_int'",
    ),
    (
        "to_float",
        "def main() -> int:\n    print(to_float(7))\n    return 0\n",
        "call to undefined function or struct 'to_float'",
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

fn run_comparison(known_gaps: bool) {
    let Some(clang) = aoxn::find_clang() else {
        eprintln!("skipping: clang unavailable (set AOXN_CLANG)");
        return;
    };
    assert!(Command::new(clang)
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
