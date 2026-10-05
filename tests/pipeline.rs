//! End-to-end pipeline tests: compile Aoxn source -> native exe -> run -> check output.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use aoxn::build_exe;

/// platform executable suffix (`.exe` on Windows, empty elsewhere) 閳?keeps the
/// suite portable without hardcoding the extension at every site
const EXE: &str = if cfg!(windows) { ".exe" } else { "" };

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// the in-Aoxn standard library (compiled together with stdlib tests)
fn stdlib_src() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("stdlib").join("stdlib.ax");
    std::fs::read_to_string(path).expect("stdlib/stdlib.ax not found")
}

fn build_and_run_with_stdlib(src: &str) -> String {
    let full = format!("{}\n{}", stdlib_src(), dedent(src));
    build_and_run(&full)
}

/// Strip the common leading indentation (and leading blank lines) from an
/// embedded source string, so tests can stay indented inside Rust code.
fn dedent(src: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let body: Vec<&&str> = lines
        .iter()
        .skip_while(|l| l.trim().is_empty())
        .collect();
    let min_indent = body
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    let mut out: Vec<String> = body
        .iter()
        .map(|l| if l.len() >= min_indent { l[min_indent..].to_string() } else { l.to_string() })
        .collect();
    // drop trailing blank lines, keep exactly one trailing newline
    while out.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        out.pop();
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

fn build_and_run(src: &str) -> String {
    let src = &dedent(src);
    let id = COUNTER.fetch_add(1, Ordering::SeqCst) + std::process::id() as usize;
    let dir = std::env::temp_dir().join("Aoxn-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let exe: PathBuf = dir.join(format!("t{id}{EXE}"));

    match build_exe(src, &exe, true) {
        Ok(()) => {}
        Err(diags) => panic!("compilation failed: {:?}", diags),
    }

    let out = Command::new(&exe).output().expect("failed to run compiled program");
    let _ = std::fs::remove_file(&exe);
    let _ = std::fs::remove_file(exe.with_extension("obj"));
    assert!(
        out.status.success(),
        "program exited with {:?}, stderr: {:?}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn expect_compile_error(src: &str) -> String {
    let src = &dedent(src);
    let id = COUNTER.fetch_add(1, Ordering::SeqCst) + std::process::id() as usize;
    let dir = std::env::temp_dir().join("Aoxn-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let exe: PathBuf = dir.join(format!("t{id}{EXE}"));
    match build_exe(src, &exe, true) {
        Ok(()) => panic!("expected compilation to fail, but it succeeded"),
        Err(diags) => diags[0].message.clone(),
    }
}

#[test]
fn hello_world() {
    let out = build_and_run(
        r#"
        # the classic
        def main() -> int:
            print("hello, Aoxn")
            return 0
        "#,
    );
    assert_eq!(out, "hello, Aoxn\n");
}

#[test]
fn arithmetic_and_precedence() {
    let out = build_and_run(
        r#"
        def main() -> int:
            a = 2 + 3 * 4        # 14
            b = (2 + 3) * 4      # 20
            c = 17 / 5           # 3
            d = 17 % 5           # 2
            print(a)
            print(b)
            print(c)
            print(d)
            return 0
        "#,
    );
    assert_eq!(out, "14\n20\n3\n2\n");
}

/// v0.37.0: `& | ^ ~ << >>` are int-only and bind exactly as in C —
/// looser than the comparisons, tighter than `&&`/`||`. The priority is the
/// whole point of this test: `1 + 2 << 2 == 12` is only true if `<<` sits
/// between `+` and `==`.
#[test]
fn bitwise_and_shift_operators() {
    let out = build_and_run(
        r#"
        def popcount(n: int) -> int:
            c = 0
            x = n
            while x != 0:
                c = c + (x & 1)
                x = x >> 1
            return c

        def rotl32(x: int, n: int) -> int:
            m = 4294967295
            x = x & m
            return ((x << n) | (x >> (32 - n))) & m

        def main() -> int:
            print(12 & 10)        # 8
            print(12 | 10)        # 14
            print(12 ^ 10)        # 6
            print(~0)             # -1
            print(1 << 10)        # 1024
            print(1024 >> 3)      # 128
            print(popcount(255))  # 8
            print(rotl32(1, 1))          # 2
            print(rotl32(2147483648, 1))  # 1
            print(1 + 2 << 2 == 12)      # precedence: + tighter than <<, << tighter than ==
            print((12 & 3) == 0)         # parens are required here, see below
            return 0
        "#,
    );
    assert_eq!(out, "8\n14\n6\n-1\n1024\n128\n8\n2\n1\ntrue\ntrue\n");
}

/// C's quirk, and Aoxn inherits it on purpose: in C `==` binds TIGHTER than
/// `&`, so `a & b == c` groups as `a & (b == c)` — which is why every real
/// codebase parenthesizes `(a & b) == c`. Pinning it here stops a future
/// "fix" that silently reorders the whole comparison/bitwise chain.
#[test]
fn bitwise_precedence_matches_c() {
    let msg = expect_compile_error(
        r#"
        def main() -> int:
            if 12 & 3 == 0:
                return 1
            return 0
        "#,
    );
    assert!(msg.contains("'&' requires two int operands"), "unexpected diagnostic: {msg}");

    let out = build_and_run(
        r#"
        def main() -> int:
            print((12 & 3) == 0)   # true
            print((1 | 2) == 3)    # true
            return 0
        "#,
    );
    assert_eq!(out, "true\ntrue\n");
}

/// A single `&`/`|` used to be a lex error with a "did you mean `&&`?" hint.
/// It is now the bitwise operator; the two-character forms must keep working.
#[test]
fn bitwise_does_not_disturb_logical_operators() {
    let out = build_and_run(
        r#"
        def main() -> int:
            print(1 & 3)              # 1
            print(1 | 2)              # 3
            print(True and False)    # false
            print(True or False)     # true
            print(not True)           # false
            a = 6
            b = 3
            print(a > b and a != 0)   # true
            return 0
        "#,
    );
    assert_eq!(out, "1\n3\nfalse\ntrue\nfalse\ntrue\n");
}

/// The strictness rule that matters for the crypto/HTTP work this unblocks:
/// there is no implicit int/float conversion, so a bitwise operator on a
/// float must be a type error rather than a silent promotion.
#[test]
fn bitwise_is_int_only() {
    let msg = expect_compile_error(
        r#"
        def main() -> int:
            x: float = 2.0
            return x | 1
        "#,
    );
    assert!(msg.contains("requires two int operands"), "unexpected diagnostic: {msg}");

    let msg = expect_compile_error(
        r#"
        def main() -> int:
            return ~"abc"
        "#,
    );
    assert!(msg.contains("unary '~' requires int"), "unexpected diagnostic: {msg}");
}

#[test]
fn floats_and_negative() {
    let out = build_and_run(
        r#"
        def main() -> int:
            x: float = 1.5 * 2.0
            y = -x
            print(x)
            print(y)
            return 0
        "#,
    );
    assert_eq!(out, "3.000000\n-3.000000\n");
}

#[test]
fn recursion_fib() {
    let out = build_and_run(
        r#"
        def fib(n: int) -> int:
            if n < 2:
                return n
            return fib(n - 1) + fib(n - 2)

        def main() -> int:
            print(fib(20))
            return 0
        "#,
    );
    assert_eq!(out, "6765\n");
}

#[test]
fn while_loop_and_mutation() {
    let out = build_and_run(
        r#"
        def main() -> int:
            total = 0
            i = 1
            while i <= 100:
                total = total + i
                i = i + 1
            print(total)
            return 0
        "#,
    );
    assert_eq!(out, "5050\n");
}

#[test]
fn elif_else_chains() {
    let out = build_and_run(
        r#"
        def classify(n: int) -> string:
            if n < 0:
                return "negative"
            elif n == 0:
                return "zero"
            else:
                return "positive"

        def main() -> int:
            print(classify(-5))
            print(classify(0))
            print(classify(7))
            return 0
        "#,
    );
    assert_eq!(out, "negative\nzero\npositive\n");
}

#[test]
fn bool_logic_and_short_circuit() {
    let out = build_and_run(
        r#"
        def boom() -> bool:
            print("BOOM")
            return True

        def main() -> int:
            a = True and True
            b = False or False
            print(a)
            print(b)
            print(not a)
            if False and boom():
                print("unreachable")
            if True or boom():
                print("ok")
            return 0
        "#,
    );
    assert_eq!(out, "true\nfalse\nfalse\nok\n");
}

#[test]
fn mutual_recursion() {
    let out = build_and_run(
        r#"
        def is_even(n: int) -> bool:
            if n == 0:
                return True
            return is_odd(n - 1)

        def is_odd(n: int) -> bool:
            if n == 0:
                return False
            return is_even(n - 1)

        def main() -> int:
            print(is_even(10))
            print(is_even(7))
            return 0
        "#,
    );
    assert_eq!(out, "true\nfalse\n");
}

#[test]
fn exit_code_propagates() {
    let dir = std::env::temp_dir().join("Aoxn-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join(format!("exit-{}{EXE}", std::process::id()));
    build_exe("def main() -> int: return 42", &exe, true).unwrap();
    let status = Command::new(&exe).status().unwrap();
    let _ = std::fs::remove_file(&exe);
    assert_eq!(status.code(), Some(42));
}

#[test]
fn comparison_ops() {
    let out = build_and_run(
        r#"
        def main() -> int:
            print(1 < 2)
            print(2 <= 2)
            print(3 > 4)
            print(4 >= 5)
            print(1 == 1)
            print(1 != 1)
            print(1.5 < 2.5)
            return 0
        "#,
    );
    assert_eq!(out, "true\ntrue\nfalse\nfalse\ntrue\nfalse\ntrue\n");
}

#[test]
fn comparison_boundary_cases() {
    // regression: '>=' was once lexed as '>'
    let out = build_and_run(
        r#"
        def ge(c: int) -> bool:
            return c >= 65

        def le(c: int) -> bool:
            return c <= 65

        def main() -> int:
            print(5 >= 5)         # boundary: equal
            print(5 >= 6)
            print(5 > 5)          # strict: false
            print(5 <= 5)         # boundary: equal
            print(4 <= 5)
            print(5 < 5)
            print(ge(65))
            print(ge(64))
            print(ge(66))
            print(le(65))
            print(le(66))
            print(le(64))
            return 0
        "#,
    );
    assert_eq!(out, "true\nfalse\nfalse\ntrue\ntrue\nfalse\ntrue\nfalse\ntrue\ntrue\nfalse\ntrue\n");
}

#[test]
fn pass_statement() {
    let out = build_and_run(
        r#"
        def main() -> int:
            # empty branches need `pass`
            if False:
                pass
            else:
                print("else ran")
            return 0
        "#,
    );
    assert_eq!(out, "else ran\n");
}

#[test]
fn multiline_call_arguments() {
    let out = build_and_run(
        r#"
        def add(a: int, b: int) -> int:
            return a + b

        def main() -> int:
            print(
                add(
                    20,
                    22,
                )
            )
            return 0
        "#,
    );
    assert_eq!(out, "42\n");
}

#[test]
fn annotated_binding_reassign() {
    let out = build_and_run(
        r#"
        def main() -> int:
            x: int = 10
            x = x + 5          # re-assignment keeps the type
            print(x)
            return 0
        "#,
    );
    assert_eq!(out, "15\n");
}

// ---- compile error tests (strict type system) ----

#[test]
fn rejects_int_float_mixing() {
    let msg = expect_compile_error("def main() -> int:\n    x: int = 1 + 1.5\n    return 0");
    assert!(msg.contains("requires two int or two float"), "{msg}");
}

#[test]
fn rejects_non_bool_condition() {
    let msg = expect_compile_error("def main() -> int:\n    if 1:\n        print(1)\n    return 0");
    assert!(msg.contains("'if' condition must be bool"), "{msg}");
}

#[test]
fn rejects_undefined_variable() {
    let msg = expect_compile_error("def main() -> int:\n    print(x)\n    return 0");
    assert!(msg.contains("unknown variable 'x'"), "{msg}");
}

#[test]
fn rejects_wrong_arg_type() {
    let msg = expect_compile_error(
        "def f(x: int) -> int:\n    return x\n\ndef main() -> int:\n    print(f(1.5))\n    return 0",
    );
    assert!(msg.contains("argument 1 of 'f' must be int, found float"), "{msg}");
}

#[test]
fn rejects_missing_return() {
    let msg = expect_compile_error("def f() -> int:\n    x = 1\n\ndef main() -> int:\n    return 0");
    assert!(msg.contains("does not return a value on all paths"), "{msg}");
}

#[test]
fn rejects_unreachable_code() {
    let msg = expect_compile_error("def main() -> int:\n    return 0\n    print(1)");
    assert!(msg.contains("unreachable"), "{msg}");
}

#[test]
fn rejects_type_change_on_rebind() {
    let msg = expect_compile_error(
        "def main() -> int:\n    x = 1\n    x: float = 2.0\n    return 0",
    );
    assert!(msg.contains("cannot re-declare 'x' as float: it is already int"), "{msg}");
}

#[test]
fn rejects_assign_wrong_type() {
    let msg = expect_compile_error(
        "def main() -> int:\n    x = 1\n    x = \"hello\"\n    return 0",
    );
    assert!(msg.contains("cannot assign a value of type string to 'x: int'"), "{msg}");
}

#[test]
fn rejects_missing_main() {
    let msg = expect_compile_error("def helper() -> int:\n    return 1");
    assert!(msg.contains("no 'main' function"), "{msg}");
}

#[test]
fn rejects_bad_indentation() {
    let msg = expect_compile_error(
        "def main() -> int:\n    x = 1\n   y = 2\n    return 0",
    );
    assert!(msg.contains("unindent does not match"), "{msg}");
}

#[test]
fn rejects_top_level_statement() {
    let msg = expect_compile_error("print(1)");
    assert!(msg.contains("expected 'import', 'def' or 'struct'"), "{msg}");
}

// ---- arrays (v0.3) ----

#[test]
fn array_literal_index_len() {
    let out = build_and_run(
        r#"
        def main() -> int:
            a = [10, 20, 30]
            print(a[0])
            print(a[2])
            print(len(a))
            a[1] = 99
            print(a[1])
            return 0
        "#,
    );
    assert_eq!(out, "10\n30\n3\n99\n");
}

#[test]
fn array_types_and_params() {
    let out = build_and_run(
        r#"
        def total(xs: [int; 4]) -> int:
            t = 0
            i = 0
            while i < len(xs):
                t = t + xs[i]
                i = i + 1
            return t

        def make() -> [int; 4]:
            return [5, 10, 15, 20]

        def main() -> int:
            xs: [int; 4] = [1, 2, 3, 4]
            print(total(xs))
            print(total(make()))
            ys = [1.5, 2.5]
            print(ys[0] + ys[1])
            return 0
        "#,
    );
    assert_eq!(out, "10\n50\n4.000000\n");
}

#[test]
fn array_value_semantics() {
    let out = build_and_run(
        r#"
        def main() -> int:
            a = [1, 2, 3]
            b = a          # copy, not reference
            b[0] = 99
            print(a[0])
            print(b[0])
            return 0
        "#,
    );
    assert_eq!(out, "1\n99\n");
}

#[test]
fn nested_arrays() {
    let out = build_and_run(
        r#"
        def main() -> int:
            m: [[int; 2]; 3] = [[1, 2], [3, 4], [5, 6]]
            m[1][0] = 30
            print(m[1][0] + m[0][1] + m[2][1])
            return 0
        "#,
    );
    assert_eq!(out, "38\n");
}

// ---- structs (v0.3) ----

#[test]
fn struct_basic() {
    let out = build_and_run(
        r#"
        struct Point:
            x: int
            y: int

        def main() -> int:
            p = Point(x=1, y=2)
            print(p.x + p.y)
            p.x = 10
            print(p.x)
            return 0
        "#,
    );
    assert_eq!(out, "3\n10\n");
}

#[test]
fn struct_value_semantics() {
    let out = build_and_run(
        r#"
        struct Point:
            x: int
            y: int

        def main() -> int:
            p = Point(x=1, y=2)
            q = p           # copy
            q.x = 99
            print(p.x)
            print(q.x)
            return 0
        "#,
    );
    assert_eq!(out, "1\n99\n");
}

#[test]
fn struct_params_and_return() {
    let out = build_and_run(
        r#"
        struct Point:
            x: int
            y: int

        def add(a: Point, b: Point) -> Point:
            return Point(x=a.x + b.x, y=a.y + b.y)

        def main() -> int:
            r = add(Point(x=1, y=2), Point(x=10, y=20))
            print(r.x)
            print(r.y)
            return 0
        "#,
    );
    assert_eq!(out, "11\n22\n");
}

#[test]
fn struct_nested_and_forward_reference() {
    let out = build_and_run(
        r#"
        # Outer references Inner before it is defined
        struct Outer:
            inner: Inner
            flag: bool

        struct Inner:
            v: int

        def main() -> int:
            o = Outer(inner=Inner(v=41), flag=True)
            o.inner.v = o.inner.v + 1
            print(o.inner.v)
            print(o.flag)
            return 0
        "#,
    );
    assert_eq!(out, "42\ntrue\n");
}

#[test]
fn struct_with_array_field() {
    let out = build_and_run(
        r#"
        struct Vec3:
            data: [float; 3]
            tag: int

        def dot(a: Vec3, b: Vec3) -> float:
            return a.data[0] * b.data[0] + a.data[1] * b.data[1] + a.data[2] * b.data[2]

        def main() -> int:
            u = Vec3(data=[1.0, 2.0, 3.0], tag=1)
            v = Vec3(data=[4.0, 5.0, 6.0], tag=2)
            print(dot(u, v))
            print(u.data[2])
            return 0
        "#,
    );
    assert_eq!(out, "32.000000\n3.000000\n");
}

#[test]
fn array_of_structs() {
    let out = build_and_run(
        r#"
        struct Point:
            x: int
            y: int

        def main() -> int:
            pts: [Point; 2] = [Point(x=1, y=2), Point(x=3, y=4)]
            pts[1].x = 30
            print(pts[0].x + pts[1].x + pts[1].y)
            return 0
        "#,
    );
    assert_eq!(out, "35\n");
}

#[test]
fn struct_used_in_conditional() {
    let out = build_and_run(
        r#"
        struct Box:
            w: int
            h: int

        def area(b: Box) -> int:
            return b.w * b.h

        def main() -> int:
            b = Box(w=3, h=4)
            if area(b) > 10:
                print("big")
            else:
                print("small")
            return 0
        "#,
    );
    assert_eq!(out, "big\n");
}

// ---- array/struct compile error tests ----

#[test]
fn rejects_print_struct() {
    let msg = expect_compile_error(
        "struct P:\n    x: int\n\ndef main() -> int:\n    print(P(x=1))\n    return 0",
    );
    assert!(msg.contains("print requires int, float, bool, or string"), "{msg}");
}

#[test]
fn rejects_compare_compound() {
    let msg = expect_compile_error(
        "def main() -> int:\n    a = [1, 2]\n    b = [1, 2]\n    if a == b:\n        print(1)\n    return 0",
    );
    assert!(msg.contains("cannot compare compound type"), "{msg}");
}

#[test]
fn rejects_unknown_struct_field() {
    let msg = expect_compile_error(
        "struct P:\n    x: int\n\ndef main() -> int:\n    p = P(x=1)\n    print(p.z)\n    return 0",
    );
    assert!(msg.contains("has no field 'z'"), "{msg}");
}

#[test]
fn rejects_missing_struct_field() {
    let msg = expect_compile_error(
        "struct P:\n    x: int\n    y: int\n\ndef main() -> int:\n    p = P(x=1)\n    return 0",
    );
    assert!(msg.contains("missing field(s): y"), "{msg}");
}

#[test]
fn rejects_recursive_struct() {
    let msg = expect_compile_error(
        "struct Node:\n    next: Node\n\ndef main() -> int:\n    return 0",
    );
    assert!(msg.contains("recursive struct"), "{msg}");
}

#[test]
fn rejects_index_non_array() {
    let msg = expect_compile_error(
        "def main() -> int:\n    x = 5\n    print(x[0])\n    return 0",
    );
    assert!(msg.contains("cannot index a value of type int"), "{msg}");
}

#[test]
fn rejects_zero_len_array() {
    let msg = expect_compile_error(
        "def main() -> int:\n    a: [int; 0] = [1]\n    return 0",
    );
    assert!(msg.contains("array length must be a positive integer"), "{msg}");
}

#[test]
fn rejects_struct_kwarg_on_fn() {
    let msg = expect_compile_error(
        "def f(x: int) -> int:\n    return x\n\ndef main() -> int:\n    print(f(x=1))\n    return 0",
    );
    assert!(msg.contains("takes positional arguments only"), "{msg}");
}

// ---- strings (v0.4) ----

#[test]
fn string_concat() {
    let out = build_and_run(
        r#"
        def greet(name: string) -> string:
            return "hi, " + name + "!"

        def main() -> int:
            s = "hello" + ", " + "world"
            print(s)
            print(greet("Aoxn"))
            t = ""
            t = t + "x" + "y" + "z"
            print(t)
            print(len(s))
            print(len(t))
            return 0
        "#,
    );
    assert_eq!(out, "hello, world\nhi, Aoxn!\nxyz\n12\n3\n");
}

#[test]
fn string_equality() {
    let out = build_and_run(
        r#"
        def same(a: string, b: string) -> bool:
            return a == b

        def main() -> int:
            print("abc" == "abc")
            print("abc" == "abd")
            print("abc" != "abd")
            print(same("hello", "hello"))
            print(same("hello", "hell"))
            print("" == "")
            return 0
        "#,
    );
    assert_eq!(out, "true\nfalse\ntrue\ntrue\nfalse\ntrue\n");
}

#[test]
fn string_ordering() {
    let out = build_and_run(
        r#"
        def main() -> int:
            print("apple" < "banana")
            print("banana" < "apple")
            print("abc" <= "abc")
            print("abd" > "abc")
            print("abc" >= "abd")
            print("" < "a")
            return 0
        "#,
    );
    assert_eq!(out, "true\nfalse\ntrue\ntrue\nfalse\ntrue\n");
}

#[test]
fn strings_in_structs_and_arrays() {
    let out = build_and_run(
        r#"
        struct User:
            name: string
            id: int

        def main() -> int:
            u = User(name="li lei", id=7)
            print(u.name)
            print(u.id)
            tag = "user#" + u.name
            print(tag)
            names: [string; 3] = ["al", "bo", "cy"]
            print(names[1])
            print(names[0] < names[1])
            print(len(names))
            reps: [string; 2] = ["hi"] * 2
            print(reps[0] + reps[1])
            return 0
        "#,
    );
    assert_eq!(out, "li lei\n7\nuser#li lei\nbo\ntrue\n3\nhihi\n");
}

#[test]
fn string_concat_in_loop() {
    let out = build_and_run(
        r#"
        def join3(a: string, b: string, c: string) -> string:
            return a + b + c

        def main() -> int:
            acc = ""
            i = 0
            while i < 3:
                acc = acc + "ab"
                i = i + 1
            print(acc)
            print(len(acc))
            print(join3("x", "y", "z"))
            return 0
        "#,
    );
    assert_eq!(out, "ababab\n6\nxyz\n");
}

#[test]
fn rejects_concat_string_int() {
    let msg = expect_compile_error(
        "def main() -> int:\n    s = \"a\" + 1\n    return 0",
    );
    assert!(msg.contains("cannot concatenate string with int"), "{msg}");
}

#[test]
fn rejects_compare_string_int() {
    let msg = expect_compile_error(
        "def main() -> int:\n    if \"a\" < 1:\n        print(1)\n    return 0",
    );
    assert!(msg.contains("two int, two float, or two string"), "{msg}");
}

// ---- for loops / break / continue / f-strings (v0.5) ----

#[test]
fn for_range_forms() {
    let out = build_and_run(
        r#"
        def main() -> int:
            for i in range(4):
                print(i)
            for i in range(2, 5):
                print(i)
            for i in range(0, 10, 3):
                print(i)
            for i in range(5, 0, -1):
                print(i)
            return 0
        "#,
    );
    assert_eq!(out, "0\n1\n2\n3\n2\n3\n4\n0\n3\n6\n9\n5\n4\n3\n2\n1\n");
}

#[test]
fn for_array_iteration() {
    let out = build_and_run(
        r#"
        struct P:
            v: int

        def main() -> int:
            total = 0
            for x in [10, 20, 30]:
                total = total + x
            print(total)
            pts: [P; 2] = [P(v=5), P(v=7)]
            s = 0
            for p in pts:
                s = s + p.v
            print(s)
            names = ["ab", "cde"]
            lens = 0
            for n in names:
                lens = lens + len(n)
            print(lens)
            return 0
        "#,
    );
    assert_eq!(out, "60\n12\n5\n");
}

#[test]
fn for_nested_and_reuse() {
    let out = build_and_run(
        r#"
        def main() -> int:
            count = 0
            for i in range(3):
                for j in range(3):
                    count = count + 1
            print(count)
            for i in range(2):
                print(i)
            for i in range(2, 4):
                print(i)
            return 0
        "#,
    );
    assert_eq!(out, "9\n0\n1\n2\n3\n");
}

#[test]
fn break_and_continue() {
    let out = build_and_run(
        r#"
        def main() -> int:
            i = 0
            while True:
                i = i + 1
                if i == 3:
                    break
            print(i)
            total = 0
            for i in range(10):
                if i % 2 == 0:
                    continue
                total = total + i
            print(total)
            found = -1
            for j in range(20):
                if j * j > 50:
                    found = j
                    break
            print(found)
            return 0
        "#,
    );
    assert_eq!(out, "3\n25\n8\n");
}

#[test]
fn f_string_basics() {
    let out = build_and_run(
        r#"
        def main() -> int:
            name = "Aoxn"
            version = 5
            print(f"hello {name}!")
            print(f"v{version}, {version * 2}")
            print(f"{True}/{False}")
            print(f"braces: {{literal}}")
            print(f"expr: {[1, 2, 3][1] + len(name)}")
            print(f"")
            print(f"just text")
            return 0
        "#,
    );
    assert_eq!(out, "hello Aoxn!\nv5, 10\ntrue/false\nbraces: {literal}\nexpr: 6\n\njust text\n");
}

#[test]
fn f_string_with_function_calls() {
    let out = build_and_run(
        r#"
        def double(n: int) -> int:
            return n * 2

        def main() -> int:
            for i in range(1, 4):
                print(f"{i} -> {double(i)}")
            s = f"{double(21)}"
            print(s)
            print(len(s))
            return 0
        "#,
    );
    assert_eq!(out, "1 -> 2\n2 -> 4\n3 -> 6\n42\n2\n");
}

#[test]
fn str_builtin() {
    let out = build_and_run(
        r#"
        def main() -> int:
            print(str(42))
            print(str(-7))
            print(str(True))
            print(str(False))
            print(str(1.5))
            print(str("already"))
            print(len(str(12345)))
            print(str(2 + 3) == "5")
            return 0
        "#,
    );
    assert_eq!(out, "42\n-7\ntrue\nfalse\n1.500000\nalready\n5\ntrue\n");
}

#[test]
fn rejects_break_outside_loop() {
    let msg = expect_compile_error(
        "def main() -> int:\n    break\n    return 0",
    );
    assert!(msg.contains("'break' outside of a loop"), "{msg}");
}

#[test]
fn rejects_continue_outside_loop() {
    let msg = expect_compile_error(
        "def main() -> int:\n    continue\n    return 0",
    );
    assert!(msg.contains("'continue' outside of a loop"), "{msg}");
}

#[test]
fn rejects_range_float_arg() {
    let msg = expect_compile_error(
        "def main() -> int:\n    for i in range(1.5):\n        print(i)\n    return 0",
    );
    assert!(msg.contains("range arguments must be int"), "{msg}");
}

#[test]
fn rejects_for_over_int() {
    let msg = expect_compile_error(
        "def main() -> int:\n    for x in 5:\n        print(x)\n    return 0",
    );
    assert!(msg.contains("'for' can only iterate over arrays"), "{msg}");
}

#[test]
fn rejects_fstring_of_array() {
    let msg = expect_compile_error(
        "def main() -> int:\n    print(f\"{[1, 2]}\")\n    return 0",
    );
    assert!(msg.contains("cannot convert [int; 2] to string"), "{msg}");
}

// ---- standard library (written in Aoxn itself, v0.7 generics) ----

#[test]
fn stdlib_math() {
    let out = build_and_run_with_stdlib(
        r#"
        def main() -> int:
            print(sqrt(4.0))
            print(sqrt(2.0))
            print(isqrt(99))
            print(isqrt(100))
            print(pow_i(3, 4))
            print(pow_i(2, 10))
            print(gcd(48, 18))
            print(gcd(-48, 18))
            print(lcm(4, 6))
            print(lcm(0, 5))
            print(is_prime(97))
            print(is_prime(98))
            print(is_prime(2))
            print(abs_i(-5))
            print(hypot(3.0, 4.0))
            print(clamp_i(15, 0, 10))
            print(clamp_f(0.5, 1.0, 2.0))
            return 0
        "#,
    );
    assert_eq!(
        out,
        "2.000000\n1.414214\n9\n10\n81\n1024\n6\n6\n12\n0\ntrue\nfalse\ntrue\n5\n5.000000\n10\n1.000000\n"
    );
}

#[test]
fn stdlib_generic_sort_search() {
    let out = build_and_run_with_stdlib(
        r#"
        def main() -> int:
            arr = [5, 3, 8, 1, 9, 2, 7, 4]
            sorted_arr = sort(arr)
            print(arr[0])                    # input untouched: value semantics
            print(sum_int(arr))
            print(max_of(arr))
            print(min_of(arr))
            print(linear_search(arr, 8))
            print(linear_search(arr, 42))
            print(binary_search(sorted_arr, 8))
            print(binary_search(sorted_arr, 6))
            print(reverse(arr)[0])
            # same functions, other types and lengths
            print(sort([3, 1])[0])
            print(sort([2.5, 1.5, 0.5])[0])
            print(sort(["pear", "apple", "fig"])[0])
            print(binary_search([10, 20, 30], 20))
            print(sum_float([1.5, 2.5]))
            print(max_of(["pear", "apple"]))
            return 0
        "#,
    );
    assert_eq!(out, "5\n39\n9\n1\n2\n-1\n6\n-1\n4\n1\n0.500000\napple\n1\n4.000000\npear\n");
}

#[test]
fn stdlib_sort_does_not_mutate() {
    let out = build_and_run_with_stdlib(
        r#"
        def main() -> int:
            a = [9, 8, 7]
            b = sort(a)
            print(a[0])
            print(b[0])
            print(b[2])
            return 0
        "#,
    );
    assert_eq!(out, "9\n7\n9\n");
}

#[test]
fn nested_generic_calls() {
    // a generic instance body calling another generic: call_map routing must
    // survive monomorphization (instance node addresses, not throwaway clones)
    let out = build_and_run(
        r#"
        def first_val[T, N](arr: [T; N]) -> T:
            return arr[0]

        def pick[T, N](arr: [T; N]) -> T:
            return first_val(arr)

        def main() -> int:
            print(pick([7, 8, 9]))
            print(pick(["x", "y"]))
            print(pick([1.5, 2.5]))
            print(first_val([42]))
            return 0
        "#,
    );
    assert_eq!(out, "7\nx\n1.500000\n42\n");
}

#[test]
fn rejects_generic_struct_sort() {
    let msg = expect_compile_error(
        "def sort[T, N](arr: [T; N]) -> [T; N]:\n    result = arr\n    for i in range(N):\n        for j in range(N - 1 - i):\n            if result[j] > result[j + 1]:\n                t = result[j]\n                result[j] = result[j + 1]\n                result[j + 1] = t\n    return result\n\nstruct P:\n    v: int\n\ndef main() -> int:\n    r = sort([P(v=1)])\n    return 0",
    );
    assert!(msg.contains("requires two int, two float, or two string operands, found (P, P)"), "{msg}");
}

#[test]
fn rejects_extern_main() {
    let msg = expect_compile_error(
        "extern def main() -> int",
    );
    assert!(msg.contains("'main' cannot be declared extern"), "{msg}");
}

#[test]
fn rejects_generic_main() {
    let msg = expect_compile_error(
        "def main[T](x: T) -> T:\n    return x",
    );
    assert!(msg.contains("'main' cannot be generic"), "{msg}");
}

#[test]
fn rejects_len_param_outside_generic() {
    let msg = expect_compile_error(
        "def f(arr: [int; N]) -> int:\n    return 0\n\ndef main() -> int:\n    return 0",
    );
    assert!(msg.contains("unknown array length 'N'"), "{msg}");
}

#[test]
fn rejects_uninferable_len() {
    let msg = expect_compile_error(
        "def make[T, N]() -> [T; N]:\n    return [0]\n\ndef main() -> int:\n    print(make())\n    return 0",
    );
    assert!(msg.contains("cannot infer array length N"), "{msg}");
}

// ---- import / module system (v0.8) ----

fn tmp_dir(tag: &str) -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst) + std::process::id() as usize;
    let dir = std::env::temp_dir().join(format!("Aoxn-import-{tag}-{id}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn import_transitive_and_include_once() {
    let dir = tmp_dir("transitive");
    std::fs::write(
        dir.join("main.ax"),
        "import * from \"./lib.ax\"\nimport * from \"./lib.ax\"\nimport * from \"./sub/deep.ax\"\n\ndef main() -> int:\n    print(helper() + deep())\n    return 0\n",
    )
    .unwrap();
    std::fs::write(dir.join("lib.ax"), "def helper() -> int:\n    return 40\n").unwrap();
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub").join("deep.ax"), "def deep() -> int:\n    return 2\n").unwrap();

    let exe = dir.join(format!("out{EXE}"));
    aoxn::build_paths_exe(
        &[dir.join("main.ax").display().to_string()],
        &exe,
        true,
    )
    .expect("compilation failed");
    let out = Command::new(&exe).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
}

// regression: per-file literal ids restart at 0; codegen literal temps must
// be keyed per AST site, not per id (aggregate literals in two files collided
// and referenced an alloca from the other function)
#[test]
fn import_aggregate_literals_across_files() {
    let dir = tmp_dir("litids");
    std::fs::write(dir.join("a.ax"), "def f() -> [int; 2]:\n    return [1, 2]\n").unwrap();
    std::fs::write(
        dir.join("main.ax"),
        "import * from \"./a.ax\"\n\ndef h() -> [int; 2]:\n    return [3, 4]\n\ndef main() -> int:\n    x = [5, 6]\n    return f()[0] + h()[0] + x[0]\n",
    )
    .unwrap();

    let exe = dir.join(format!("out{EXE}"));
    aoxn::build_paths_exe(&[dir.join("main.ax").display().to_string()], &exe, true)
        .expect("compilation failed");
    let out = Command::new(&exe).output().unwrap();
    assert_eq!(out.status.code(), Some(9), "1 + 3 + 5");
}

#[test]
fn import_cycle_detected() {
    let dir = tmp_dir("cycle");
    std::fs::write(dir.join("a.ax"), "import * from \"./b.ax\"\n\ndef fa() -> int:\n    return fb() + 1\n").unwrap();
    std::fs::write(dir.join("b.ax"), "import * from \"./a.ax\"\n\ndef fb() -> int:\n    return 1\n").unwrap();

    let exe = dir.join(format!("out{EXE}"));
    match aoxn::build_paths_exe(&[dir.join("a.ax").display().to_string()], &exe, true) {
        Ok(()) => panic!("expected circular import error"),
        Err(diags) => {
            assert!(diags[0].message.contains("circular import"), "{:?}", diags[0]);
            assert!(diags[0].message.contains("a.ax"), "{:?}", diags[0]);
        }
    }
}

#[test]
fn import_missing_file() {
    let dir = tmp_dir("missing");
    std::fs::write(dir.join("main.ax"), "import * from \"./nope.ax\"\n\ndef main() -> int:\n    return 0\n").unwrap();
    let exe = dir.join(format!("out{EXE}"));
    match aoxn::build_paths_exe(&[dir.join("main.ax").display().to_string()], &exe, true) {
        Ok(()) => panic!("expected import error"),
        Err(diags) => assert!(diags[0].message.contains("cannot open"), "{:?}", diags[0]),
    }
}

#[test]
fn import_error_reports_importing_file() {
    let dir = tmp_dir("errfile");
    std::fs::write(dir.join("main.ax"), "import * from \"./lib.ax\"\n\ndef main() -> int:\n    print(helper())\n    return 0\n").unwrap();
    std::fs::write(dir.join("lib.ax"), "def helper() -> int:\n    print(unknown_var)\n    return 0\n").unwrap();
    let exe = dir.join(format!("out{EXE}"));
    match aoxn::build_paths_exe(&[dir.join("main.ax").display().to_string()], &exe, true) {
        Ok(()) => panic!("expected compile error"),
        Err(diags) => {
            let file = aoxn::files::name(diags[0].file);
            assert!(file.contains("lib.ax"), "{:?} / {file}", diags[0]);
            assert_eq!(diags[0].line, 2, "{:?}", diags[0]);
            assert!(diags[0].message.contains("unknown variable 'unknown_var'"), "{:?}", diags[0]);
        }
    }
}

#[test]
fn string_sources_reject_imports() {
    let msg = expect_compile_error("import * from \"./somewhere.ax\"\n\ndef main() -> int:\n    return 0");
    assert!(msg.contains("requires compiling from files"), "{msg}");
}

// ---- module namespaces + the Python import spellings (v0.43.0) ----

/// Write `files` into a fresh directory and build+run `main.ax` there,
/// returning its stdout.
fn build_and_run_modules(tag: &str, files: &[(&str, &str)]) -> String {
    let dir = tmp_dir(tag);
    for (name, body) in files {
        let p = dir.join(name);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(p, body).unwrap();
    }
    let exe = dir.join(format!("out{EXE}"));
    aoxn::build_paths_exe(&[dir.join("main.ax").display().to_string()], &exe, true)
        .expect("compilation failed");
    let out = Command::new(&exe).output().unwrap();
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// The headline capability: two modules that both define `helper` coexist
/// once they are imported as modules instead of merged.
#[test]
fn namespace_qualified_access_disambiguates_same_named_functions() {
    let out = build_and_run_modules(
        "qualified",
        &[
            ("a.ax", "def helper() -> int:\n    return 1\n"),
            ("b.ax", "def helper() -> int:\n    return 2\n"),
            ("main.ax", "import a\nimport b\n\ndef main() -> int:\n    print(a.helper() + b.helper())\n    return 0\n"),
        ],
    );
    assert_eq!(out, "3\n");
}

#[test]
fn namespace_qualified_construction_and_type_annotation() {
    let out = build_and_run_modules(
        "qualified-struct",
        &[
            ("geo.ax", "struct Point:\n    x: int\n    y: int\n"),
            ("main.ax", "import geo\n\ndef main() -> int:\n    p: geo.Point = geo.Point(x=3, y=4)\n    print(p.x + p.y)\n    return 0\n"),
        ],
    );
    assert_eq!(out, "7\n");
}

#[test]
fn namespace_alias_renames_the_module_not_its_members() {
    let out = build_and_run_modules(
        "alias",
        &[
            ("geo.ax", "def area() -> int:\n    return 12\n"),
            ("main.ax", "import geo as g\n\ndef main() -> int:\n    print(g.area())\n    return 0\n"),
        ],
    );
    assert_eq!(out, "12\n");
}

#[test]
fn namespace_bare_specifier_finds_the_sibling_file() {
    let out = build_and_run_modules(
        "sibling",
        &[
            ("util.ax", "def twice(n: int) -> int:\n    return n * 2\n"),
            ("main.ax", "import util\n\ndef main() -> int:\n    print(util.twice(21))\n    return 0\n"),
        ],
    );
    assert_eq!(out, "42\n");
}

#[test]
fn python_star_import_merges_the_whole_module() {
    let out = build_and_run_modules(
        "star",
        &[
            ("util.ax", "def a() -> int:\n    return 1\n\ndef b() -> int:\n    return 2\n"),
            ("main.ax", "from util import *\n\ndef main() -> int:\n    print(a() + b())\n    return 0\n"),
        ],
    );
    assert_eq!(out, "3\n");
}

#[test]
fn python_named_import_renames_with_as() {
    let out = build_and_run_modules(
        "named-as",
        &[
            ("util.ax", "def a() -> int:\n    return 6\n\ndef b() -> int:\n    return 7\n"),
            ("main.ax", "from util import a as alpha, b as beta\n\ndef main() -> int:\n    print(alpha() * beta())\n    return 0\n"),
        ],
    );
    assert_eq!(out, "42\n");
}

// The bare `import "p"` spelling W1-S3 removed is back (v0.43.0) and means
// the same as `import * from "p"`.
#[test]
fn bare_import_path_is_revived() {
    let out = build_and_run_modules(
        "bare",
        &[
            ("util.ax", "def v() -> int:\n    return 9\n"),
            ("main.ax", "import \"./util.ax\"\n\ndef main() -> int:\n    print(v())\n    return 0\n"),
        ],
    );
    assert_eq!(out, "9\n");
}

/// Selective import really filters: the unlisted name is not in scope. This
/// is the behaviour change — before v0.43.0 the list was parsed and ignored.
#[test]
fn named_import_excludes_what_it_does_not_list() {
    let dir = tmp_dir("filtered");
    std::fs::write(
        dir.join("util.ax"),
        "def a() -> int:\n    return 1\n\ndef b() -> int:\n    return 2\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.ax"),
        "from util import a\n\ndef main() -> int:\n    print(a())\n    return b()\n",
    )
    .unwrap();
    let exe = dir.join(format!("out{EXE}"));
    let diags = aoxn::build_paths_exe(&[dir.join("main.ax").display().to_string()], &exe, true)
        .expect_err("`b` was not imported, so it must not resolve");
    let msg = &diags[0].message;
    assert!(msg.contains("undefined") && msg.contains("'b'"), "{msg}");
}

/// A name two modules provide cannot be star-imported unqualified — the
/// diagnostic has to say what to do instead.
#[test]
fn two_star_imports_of_one_name_is_a_named_error() {
    let dir = tmp_dir("conflict");
    std::fs::write(dir.join("a.ax"), "def helper() -> int:\n    return 1\n").unwrap();
    std::fs::write(dir.join("b.ax"), "def helper() -> int:\n    return 2\n").unwrap();
    std::fs::write(
        dir.join("main.ax"),
        "import * from \"./a.ax\"\nimport * from \"./b.ax\"\n\ndef main() -> int:\n    return 0\n",
    )
    .unwrap();
    let exe = dir.join(format!("out{EXE}"));
    let diags = aoxn::build_paths_exe(&[dir.join("main.ax").display().to_string()], &exe, true)
        .expect_err("two modules cannot both provide `helper` unqualified");
    assert!(diags[0].message.contains("comes from two modules"), "{:?}", diags);
}

#[test]
fn importing_a_name_the_module_does_not_have_is_an_error() {
    let dir = tmp_dir("missing-name");
    std::fs::write(dir.join("util.ax"), "def a() -> int:\n    return 1\n").unwrap();
    std::fs::write(
        dir.join("main.ax"),
        "from util import nosuch\n\ndef main() -> int:\n    return 0\n",
    )
    .unwrap();
    let exe = dir.join(format!("out{EXE}"));
    let diags = aoxn::build_paths_exe(&[dir.join("main.ax").display().to_string()], &exe, true)
        .expect_err("the module has no such name");
    assert!(diags[0].message.contains("no top-level name 'nosuch'"), "{:?}", diags);
}

/// A module used as a namespace is not a value — `x = util` has to fail
/// rather than silently do something.
#[test]
fn module_binding_is_not_a_value() {
    let dir = tmp_dir("ns-value");
    std::fs::write(dir.join("util.ax"), "def a() -> int:\n    return 1\n").unwrap();
    std::fs::write(
        dir.join("main.ax"),
        "import util\n\ndef main() -> int:\n    x = util\n    return 0\n",
    )
    .unwrap();
    let exe = dir.join(format!("out{EXE}"));
    let diags = aoxn::build_paths_exe(&[dir.join("main.ax").display().to_string()], &exe, true)
        .expect_err("a module namespace has no runtime value");
    assert!(diags[0].message.contains("unknown variable 'util'"), "{:?}", diags);
}

/// A diamond still resolves: `d`'s names reach the program through both `a`
/// and `b`, because a star import re-exports.
#[test]
fn diamond_star_imports_still_reach_the_program() {
    let out = build_and_run_modules(
        "diamond",
        &[
            ("d.ax", "def shared() -> int:\n    return 1\n"),
            ("a.ax", "import * from \"./d.ax\"\n\ndef av() -> int:\n    return shared() + 1\n"),
            ("b.ax", "import * from \"./d.ax\"\n\ndef bv() -> int:\n    return shared() + 2\n"),
            ("main.ax", "import * from \"./a.ax\"\nimport * from \"./b.ax\"\n\ndef main() -> int:\n    print(shared() + av() + bv())\n    return 0\n"),
        ],
    );
    assert_eq!(out, "6\n");
}

/// The load-bearing invariant behind the whole design: a program that compiled
/// before v0.43.0 emits the SAME C. Mangling is reserved for names two modules
/// provide, and such a program has none.
#[test]
fn whole_module_merge_emits_unmangled_names() {
    let dir = tmp_dir("stable-c");
    std::fs::write(
        dir.join("util.ax"),
        "def helper() -> int:\n    return 40\n\nstruct Point:\n    x: int\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.ax"),
        "import * from \"./util.ax\"\n\ndef main() -> int:\n    p = Point(x=2)\n    return helper() + p.x\n",
    )
    .unwrap();
    let entry = dir.join("main.ax").display().to_string();
    let c = aoxn::compile_paths_to_c(&[entry], true).expect("compilation failed");
    assert!(c.contains("long long helper(void)"), "helper must keep its bare name");
    assert!(!c.contains("util_helper"), "nothing may be prefixed when no name collides: {c}");
}

// ---- stdlib v0.9: raw memory, Vec, buffers, file IO ----

#[test]
fn stdlib_vec_grow_and_slots() {
    let out = build_and_run_with_stdlib(
        r#"
        def main() -> int:
            v = vec_new()
            for i in range(100):
                v = vec_push(v, i * i)
            print(v.len)
            print(vec_get(v, 0))
            print(vec_get(v, 10))
            print(vec_get(v, 99))
            vec_set(v, 50, 7)
            print(vec_get(v, 50))
            vec_free(v)
            # strings via as_ptr / as_string
            sv = vec_new()
            sv = vec_push(sv, as_ptr("alpha"))
            sv = vec_push(sv, as_ptr("beta"))
            print(as_string(vec_get(sv, 0)))
            print(as_string(vec_get(sv, 1)))
            vec_free(sv)
            return 0
        "#,
    );
    assert_eq!(out, "100\n0\n100\n9801\n7\nalpha\nbeta\n");
}

/// v0.39.0 Tier 0.2 — the error channel. A failing call returns an `Err`
/// value; the payload comes back through a heap out-slot the caller owns.
/// The port parser is the shape every fallible Aoxn function should have:
/// validate first, write the out-slot only on the success path.
#[test]
fn stdlib_error_channel_and_out_slots() {
    let out = build_and_run_with_stdlib(
        r#"
        def parse_port(s: string, out: int) -> Err:
            if len(s) == 0:
                return err_new(ERR_INVALID(), "empty port")
            i = 0
            while i < len(s):
                if not is_digit(str_get(s, i)):
                    return err_new(ERR_PARSE(), "port must be digits")
                i = i + 1
            v = 0
            i = 0
            while i < len(s):
                v = v * 10 + (str_get(s, i) - 48)
                i = i + 1
            if v > 65535:
                return err_new(ERR_RANGE(), "port out of range")
            out_set_i(out, v)
            return err_ok()

        def main() -> int:
            p = out_new()

            e = parse_port("8080", p)
            print(err_is_ok(e))
            print(out_get_i(p))

            e = parse_port("80x0", p)
            print(err_is_ok(e))
            print(err_code_name(e.code))
            print(e.message)

            e = parse_port("99999", p)
            print(err_code_name(e.code))
            # err_or_int is the short-circuiting `or` a language with no
            # error propagation does not have
            print(err_or_int(e, 1, -1))

            # a failure that carries no payload needs no out-slot at all
            print(err_is_err(err_new(ERR_IO(), "disk on fire")))
            print(err_is_ok(err_ok()))
            out_free(p)
            return 0
        "#,
    );
    assert_eq!(
        out,
        "true\n8080\nfalse\nparse\nport must be digits\nrange\n-1\ntrue\ntrue\n"
    );
}

/// v0.39.0 Tier 0.3 — variable-length containers. `VecVec` is the case that
/// motivated them: a vector whose elements are themselves vectors, which is
/// what a document format needs and what a fixed `[T; N]` cannot express.
#[test]
fn stdlib_variable_length_containers() {
    let out = build_and_run_with_stdlib(
        r#"
        def make_row(a: int, b: int, c: int) -> Vec:
            v = vec_new()
            v = vec_push(v, a)
            v = vec_push(v, b)
            v = vec_push(v, c)
            return v

        def main() -> int:
            v = vec_new()
            for i in range(20):
                v = vec_push(v, i * i)
            print(vec_len(v))
            print(vec_cap(v) >= 20)
            print(vec_get(v, 7))
            print(vec_last(v))
            print(vec_index_of(v, 49))
            print(vec_index_of(v, 50))       # absent -> -1

            # truncate returns a copy; the original is untouched
            t = vec_truncate(v, 3)
            print(vec_len(t))
            print(vec_len(v))

            # vec_clear keeps the allocation for reuse. It is the SAME buffer
            # `v` points at, so exactly one of the two may be freed — freeing
            # both is a double free, and the test suite hits heap corruption
            # if you try it.
            c = vec_clear(v)
            print(vec_len(c))
            c = vec_push(c, 42)          # reuses the kept allocation
            print(vec_len(c))
            print(vec_get(c, 0))
            vec_free(c)

            # strings as elements need no manual as_ptr dance
            s = vec_new()
            s = vec_push_str(s, "alpha")
            s = vec_push_str(s, "beta")
            s = vec_push_str(s, "gamma")
            print(vec_len(s))
            print(vec_get_str(s, 1))
            print(vec_index_of_str(s, "gamma"))
            print(vec_contains_str(s, "delta"))
            vec_set_str(s, 0, "ALPHA")
            print(vec_get_str(s, 0))
            vec_free(s)

            # two levels: the outer element is a Vec, stored in three slots
            grid = vecvec_new()
            grid = vecvec_push(grid, make_row(1, 2, 3))
            grid = vecvec_push(grid, make_row(4, 5, 6))
            grid = vecvec_push(grid, make_row(7, 8, 9))
            print(vecvec_len(grid))
            print(vec_get(vecvec_get(grid, 0), 2))
            print(vec_get(vecvec_get(grid, 2), 1))
            print(vec_len(vecvec_get(grid, 2)))
            vecvec_free_deep(grid)
            return 0
        "#,
    );
    assert_eq!(
        out,
        "20\ntrue\n49\n361\n7\n-1\n3\n20\n0\n1\n42\n3\nbeta\n2\nfalse\nALPHA\n3\n3\n8\n3\n"
    );
}

#[test]
fn infer_binding_from_as_string_and_as_ptr() {
    // regression: unannotated bindings need codegen type hints for
    // as_string (-> string) and as_ptr (-> int)
    let out = build_and_run_with_stdlib(
        r#"
        def main() -> int:
            s = "roundtrip"
            p = as_ptr(s)
            t = as_string(p)
            print(t)
            print(len(t))
            return 0
        "#,
    );
    assert_eq!(out, "roundtrip\n9\n");
}

#[test]
fn stdlib_str_bytes_and_classes() {
    let out = build_and_run_with_stdlib(
        r#"
        def main() -> int:
            s = "Aoxn9!"
            print(str_get(s, 0))          # 'A' = 65
            print(str_get(s, 4))          # '9' = 57
            print(is_digit(str_get(s, 4)))
            print(is_digit(str_get(s, 0)))
            print(is_alpha(str_get(s, 0)))
            print(is_space(str_get(s, 5)))
            print(len(s))
            return 0
        "#,
    );
    assert_eq!(out, "65\n57\ntrue\nfalse\ntrue\nfalse\n6\n");
}

#[test]
fn stdlib_file_io_roundtrip() {
    let out = build_and_run_with_stdlib(
        r#"
        def main() -> int:
            path = "AOXN_stdlib_test.txt"
            ok = write_file(path, "hello from Aoxn")
            print(ok)
            content = read_file(path)
            print(content)
            print(len(content))
            print(read_file("definitely_missing_file_xyz.txt") == "")
            return 0
        "#,
    );
    assert!(out.starts_with("true\nhello from Aoxn\n15\ntrue\n"), "{out}");
}

#[test]
fn stdlib_system_spawn() {
    // `system_exit_code` normalizes POSIX wait statuses and the Windows exit
    // code, so the command and the expectation are identical everywhere
    let out = build_and_run_with_stdlib(
        r#"
        def main() -> int:
            code = system_exit_code("exit 7")
            print(code)
            return 0
        "#,
    );
    assert_eq!(out, "7\n");
}

// ---- self-hosting stage 1: the Aoxn lexer written in Aoxn (v0.10) ----

#[test]
fn selfhost_lexer_token_stream() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let demo = manifest.join("selfhost").join("lex_demo.ax");
    let exe = std::env::temp_dir()
        .join("axon-tests")
        .join(format!("selfhost-lex-{}{EXE}", std::process::id()));
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();

    aoxn::build_paths_exe(&[demo.display().to_string()], &exe, true)
        .expect("self-host lexer demo failed to compile");
    let out = Command::new(&exe).output().expect("failed to run");
    let _ = std::fs::remove_file(&exe);
    assert!(out.status.success(), "self-host lexer demo crashed: {:?}", out.status.code());

    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "DEF\nIDENT main\nLPAREN\nRPAREN\nARROW\nTYINT\nCOLON\nNEWLINE\n\
         INDENT\n\
         IDENT x\nASSIGN\nINT 1\nNEWLINE\n\
         IDENT print\nLPAREN\nSTR hi\nCOMMA\nFLOAT 2.5\nRPAREN\nNEWLINE\n\
         RETURN\nIDENT x\nNEWLINE\n\
         DEDENT\n\
         EOF\n"
    );
}

// ---- self-hosting stage 2: the Aoxn parser written in Aoxn (v0.11) ----

#[test]
fn selfhost_parser_ast_dump() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let demo = manifest.join("selfhost").join("parse_demo.ax");
    let exe = std::env::temp_dir()
        .join("axon-tests")
        .join(format!("selfhost-parse-{}{EXE}", std::process::id()));
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();

    aoxn::build_paths_exe(&[demo.display().to_string()], &exe, true)
        .expect("self-host parser demo failed to compile");
    let out = Command::new(&exe).output().expect("failed to run");
    let _ = std::fs::remove_file(&exe);
    assert!(out.status.success(), "self-host parser demo crashed: {:?}", out.status.code());

    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "BLOCK program\n\
         IMPORT \nSTR lib.ax\n\
         STRUCT P\nFIELD v\nTY-int \n\
         FN scale\n\
         tag4 \nVAR P2\n\
         PARAM arr\nTY-array  len=2\nTY-int \n\
         PARAM k\nTY-int \n\
         TY-int \n\
         BLOCK \n\
         LET total\nINT 0 0\n\
         tag11 x\nVAR arr\n\
         BLOCK \n\
         LET total\nBINARY \nVAR total\nBINARY \nVAR x\nVAR k\n\
         LET label\nBINARY \nSTR total=\nCALL str\nARG \nVAR total\n\
         IF \nBINARY \nVAR total\nINT 0 0\n\
         BLOCK \nRETURN \nVAR total\n\
         RETURN \ntag28 \nINT 1 1\n"
    );
}

// ---- self-hosting stage 3: the Aoxn type checker written in Aoxn (v0.11) ----

#[test]
fn selfhost_typechecker_accepts_and_rejects() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let demo = manifest.join("selfhost").join("tycheck_demo.ax");
    let exe = std::env::temp_dir()
        .join("axon-tests")
        .join(format!("selfhost-tycheck-{}{EXE}", std::process::id()));
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();

    aoxn::build_paths_exe(&[demo.display().to_string()], &exe, true)
        .expect("self-host typechecker demo failed to compile");
    let out = Command::new(&exe).output().expect("failed to run");
    let _ = std::fs::remove_file(&exe);
    assert!(out.status.success(), "self-host typechecker demo crashed: {:?}", out.status.code());

    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "typecheck OK\n\
         bad program rejected: '+' requires two int or two float operands, found (int, bool)\n\
         generics OK, instances:\n\
         \x20 wrap.i\n\
         \x20 wrap.b\n\
         \x20 id.i\n\
         \x20 id.b\n\
         bad generic call rejected: argument 2 of 'pair' must be T, found bool\n\
         length generics OK: first.i.?.3\n"
    );
}

// stage-2 exit criterion: the self-hosted front end (lexer + parser +
// checker) handles the entire stdlib in one program
#[test]
fn selfhost_frontend_handles_stdlib() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let stdlib = std::fs::read_to_string(manifest.join("stdlib").join("stdlib.ax")).unwrap();
    let mut checked = stdlib;
    checked.push_str("\ndef main() -> int:\n    return 0\n");
    let esc = checked
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");

    let abs = |p: &str| manifest.join(p).display().to_string().replace('\\', "/");
    let driver_src = format!(
        "import * from \"{}\"\nimport * from \"{}\"\nimport * from \"{}\"\n\n\
         def main() -> int:\n    \
         src = \"{esc}\"\n    \
         c = checker_new(lex_state_new(src))\n    \
         c = check_all(c)\n    \
         if c.err == 1:\n        \
         print(\"CHECK ERROR: \" + c.errmsg)\n        \
         return 1\n    \
         print(\"stdlib typechecks OK\")\n    \
         return 0\n",
        abs("selfhost/lexer.ax"),
        abs("selfhost/parser.ax"),
        abs("selfhost/typecheck.ax"),
    );

    let dir = std::env::temp_dir().join("axon-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let driver = dir.join(format!("selfhost-stdlib-{}.ax", std::process::id()));
    let exe = dir.join(format!("selfhost-stdlib-{}{EXE}", std::process::id()));
    std::fs::write(&driver, driver_src).unwrap();

    aoxn::build_paths_exe(&[driver.display().to_string()], &exe, true)
        .expect("self-host stdlib checker failed to compile");
    let out = Command::new(&exe).output().expect("failed to run");
    let _ = std::fs::remove_file(&exe);
    let _ = std::fs::remove_file(&driver);
    assert!(out.status.success(), "self-host stdlib checker crashed: {:?}", out.status.code());

    assert_eq!(String::from_utf8_lossy(&out.stdout), "stdlib typechecks OK\n");
}

// ---- self-hosting stage 4: codegen slice 1 (int functions -> native exe) ----

/// Directory containing the clang used for the C backend, for tests that run
/// processes which shell out to `clang` themselves (the self-hosted driver's
/// codegen and link steps do). Returns `None` when clang cannot be found
/// (callers skip with a message; CI always has clang).
fn clang_dir() -> Option<PathBuf> {
    aoxn::find_clang().map(|p| p.parent().map(Path::to_path_buf).unwrap_or_default())
}

/// PATH with clang's directory prepended, using the host separator (`;` on
/// Windows, `:` elsewhere).
fn path_with_clang(clang: &std::path::Path) -> String {
    let mut parts: Vec<PathBuf> = vec![clang.to_path_buf()];
    if let Some(existing) = std::env::var_os("PATH") {
        parts.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(parts)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[test]
fn selfhost_codegen_int_slice() {
    let Some(clang) = clang_dir() else {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    };
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let demo = manifest.join("selfhost").join("codegen_demo.ax");
    let dir = std::env::temp_dir().join("axon-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join(format!("selfhost-cg-{}{EXE}", std::process::id()));

    aoxn::build_paths_exe(&[demo.display().to_string()], &exe, true)
        .expect("self-host codegen demo failed to compile");

    let path = path_with_clang(&clang);
    let out = Command::new(&exe)
        .current_dir(&dir)
        .env("PATH", &path)
        .output()
        .expect("failed to run codegen demo");
    let _ = std::fs::remove_file(&exe);
    assert!(
        out.status.success(),
        "self-host codegen demo crashed: {:?} {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "codegen OK\n");

    // link the self-hosted object and run it
    let obj = dir.join("selfhost_out.obj");
    assert!(obj.exists(), "self-hosted object was not written");
    let exe_self = dir.join(format!("selfhost-cg-out-{}{EXE}", std::process::id()));
    aoxn::link(&obj, &exe_self).expect("linking the self-hosted object failed");
    let out_self = Command::new(&exe_self).output().expect("failed to run self-hosted exe");
    let _ = std::fs::remove_file(&exe_self);
    let _ = std::fs::remove_file(&obj);

    // parity: the Rust compiler must produce the same stdout and exit code
    let src = "def twice[T](x: T) -> T:\n    return x + x\n\ndef add(a: int, b: int) -> int:\n    return a + b\n\ndef fact(n: int) -> int:\n    if n <= 1:\n        return 1\n    return n * fact(n - 1)\n\ndef greet(name: string) -> string:\n    return \"hello, \" + name + \"!\"\n\ndef half(x: float) -> float:\n    return x / 2.0\n\nstruct Point:\n    x: int\n    y: int\n\ndef dist2(a: Point, b: Point) -> int:\n    dx = a.x - b.x\n    dy = a.y - b.y\n    return dx * dx + dy * dy\n\ndef origin() -> Point:\n    return Point(x=0, y=0)\n\ndef main() -> int:\n    t = twice(3)\n    for i in range(1, 6):\n        t = t + i * i\n    while t > 50:\n        t = t - 10\n    ok = t == 41\n    print(t)\n    print(fact(4))\n    print(t > 40)\n    print(ok)\n    print(-t)\n    msg = greet(\"aoxn\")\n    print(msg)\n    print(len(msg))\n    print(msg == \"hello, aoxn!\")\n    print(\"n=\" + str(t - 36))\n    print(f\"n squared = {(t - 36) * (t - 36)}\")\n    f = 1.5\n    g2 = f + 2.5\n    print(g2)\n    print(half(g2))\n    print(f < 2.0)\n    print(f == 1.5)\n    print(-f)\n    print(\"f=\" + str(f))\n    print(f\"half f = {half(f)}\")\n    p = Point(x=3, y=4)\n    q = Point(x=0, y=0)\n    print(dist2(p, q))\n    r = origin()\n    print(r.x)\n    p.x = 10\n    print(p.x)\n    s = p\n    s.y = 7\n    print(p.y)\n    print(s.y)\n    return add(t, fact(4)) % 100\n";
    let exe_rust = dir.join(format!("selfhost-cg-rs-{}{EXE}", std::process::id()));
    aoxn::build_exe(src, &exe_rust, true).expect("rust reference compile failed");
    let out_rust = Command::new(&exe_rust).output().expect("failed to run rust reference");
    let _ = std::fs::remove_file(&exe_rust);

    assert_eq!(
        String::from_utf8_lossy(&out_self.stdout),
        "41\n24\ntrue\ntrue\n-41\nhello, aoxn!\n12\ntrue\nn=5\nn squared = 25\n\
         4.000000\n2.000000\ntrue\ntrue\n-1.500000\nf=1.500000\nhalf f = 0.750000\n\
         25\n0\n10\n4\n7\n"
    );
    assert_eq!(out_self.status.code(), Some(65));
    assert_eq!(out_rust.stdout, out_self.stdout);
    assert_eq!(out_rust.status.code(), out_self.status.code());
}

// ---- self-hosting: the Aoxn-written driver compiles a file end to end ----

#[test]
fn selfhost_driver_links_hello() {
    let Some(clang) = clang_dir() else {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    };
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let demo = manifest.join("selfhost").join("driver_demo.ax");

    // ASCII fixture dir: the self-hosted loader opens paths with narrow fopen
    let dir = manifest
        .join("target")
        .join(format!("shdriver-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let hello = "def main() -> int:\n    print(\"hello, Aoxn\")\n    return 0\n";
    std::fs::write(dir.join("hello.ax"), hello).unwrap();

    let exe = dir.join(format!("driver{EXE}"));
    aoxn::build_paths_exe(&[demo.display().to_string()], &exe, true)
        .expect("self-host driver demo failed to compile");

    let path = path_with_clang(&clang);
    let out = Command::new(&exe)
        .current_dir(&dir)
        .env("PATH", &path)
        .output()
        .expect("failed to run driver demo");
    assert!(
        out.status.success(),
        "driver demo failed: {:?} {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "driver OK\n");

    // the executable produced by the Aoxn-written pipeline actually runs
    let produced = dir.join(format!("selfhost_hello{EXE}"));
    assert!(produced.exists(), "driver did not emit selfhost_hello.exe");
    let out_self = Command::new(&produced).output().expect("failed to run produced exe");

    // parity: the Rust compiler produces the same output
    let rust_exe = dir.join(format!("hello_rust{EXE}"));
    aoxn::build_paths_exe(
        &[dir.join("hello.ax").display().to_string()],
        &rust_exe,
        true,
    )
    .expect("rust reference compile failed");
    let out_rust = Command::new(&rust_exe).output().expect("failed to run rust reference");

    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(String::from_utf8_lossy(&out_self.stdout), "hello, Aoxn\n");
    assert_eq!(out_rust.stdout, out_self.stdout);
}

/// v0.42.0: the Aoxn-written lexer must see the same literals as the Rust one —
/// radix prefixes and the two new escapes. Both compilers build the same
/// fixture and their programs must print the same bytes.
#[test]
fn selfhost_driver_mirrors_radix_literals_and_escapes() {
    let Some(clang) = clang_dir() else {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    };
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let demo = manifest.join("selfhost").join("driver_demo.ax");

    let dir = manifest
        .join("target")
        .join(format!("shradix-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // keep to the subset stage-2 can emit: no dict/None/fn-ptr/raise, and no
    // argv (the self-hosted emitter has no `main(argc, argv)` mirror)
    let prog = "def main() -> int:\n    print(0x1F)\n    print(0b101)\n    print(0xff + 1)\n    print(0X10)\n    s = \"a\\rb\"\n    print(len(s))\n    print(load_u8(s, 1))\n    return 0\n";
    std::fs::write(dir.join("hello.ax"), prog).unwrap();

    let exe = dir.join(format!("driver{EXE}"));
    aoxn::build_paths_exe(&[demo.display().to_string()], &exe, true)
        .expect("self-host driver demo failed to compile");

    let path = path_with_clang(&clang);
    let out = Command::new(&exe)
        .current_dir(&dir)
        .env("PATH", &path)
        .output()
        .expect("failed to run driver demo");
    assert!(
        out.status.success(),
        "driver demo failed: {:?} {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout)
    );

    let produced = dir.join(format!("selfhost_hello{EXE}"));
    assert!(produced.exists(), "driver did not emit selfhost_hello.exe");
    let out_self = Command::new(&produced).output().expect("failed to run produced exe");

    let rust_exe = dir.join(format!("hello_rust{EXE}"));
    aoxn::build_paths_exe(&[dir.join("hello.ax").display().to_string()], &rust_exe, true)
        .expect("rust reference compile failed");
    let out_rust = Command::new(&rust_exe).output().expect("failed to run rust reference");

    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        String::from_utf8_lossy(&out_self.stdout),
        "31\n5\n256\n16\n3\n13\n"
    );
    assert_eq!(out_rust.stdout, out_self.stdout);
}

// ---- self-hosting: the Aoxn-written driver compiles a stdlib program ----

/// fixture: a stdlib-importing program exercising generics, arrays, Vec and
/// short-circuit logic 閳?the shared target for the self-hosting driver tests.
/// The second half pushes the self-hosted codegen through nested aggregates:
/// 2D arrays, structs with array-of-array fields, sub-array call arguments,
/// 2D for-in, value-semantics copies of compound structs.
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

/// expected stdout of STDLIB_USE_PROG under either compiler
const STDLIB_USE_OUT: &str = "1\n8\n2\n17\n4\napple\npear\n7\n30\ntrue\nfalse\n101\n5.000000\n\
                              1\n6\n40\n2\n3\n6\n51\n6\ng\n11\n33\n57\n7\n99\n57\n149\n\
                              apple\npear\n1\n";

#[test]
fn selfhost_driver_compiles_stdlib() {
    let Some(clang) = clang_dir() else {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    };
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let demo = manifest.join("selfhost").join("driver_stdlib_demo.ax");

    // ASCII fixture dir: the self-hosted loader opens paths with narrow fopen.
    // The fixture imports the real stdlib two levels up (target/<dir>/ -> repo).
    let dir = manifest
        .join("target")
        .join(format!("shstdlib-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("stdlib_use.ax"), STDLIB_USE_PROG).unwrap();

    let exe = dir.join(format!("driver{EXE}"));
    aoxn::build_paths_exe(&[demo.display().to_string()], &exe, true)
        .expect("self-host driver demo failed to compile");

    let path = path_with_clang(&clang);
    let out = Command::new(&exe)
        .current_dir(&dir)
        .env("PATH", &path)
        .output()
        .expect("failed to run driver demo");
    assert!(
        out.status.success(),
        "driver demo failed: {:?} {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "driver OK\n");

    // the executable produced by the Aoxn-written pipeline actually runs
    let produced = dir.join(format!("selfhost_stdlib{EXE}"));
    assert!(produced.exists(), "driver did not emit selfhost_stdlib.exe");
    let out_self = Command::new(&produced).output().expect("failed to run produced exe");

    // parity: the Rust compiler produces the same output
    let rust_exe = dir.join(format!("stdlib_use_rust{EXE}"));
    aoxn::build_paths_exe(
        &[dir.join("stdlib_use.ax").display().to_string()],
        &rust_exe,
        true,
    )
    .expect("rust reference compile failed");
    let out_rust = Command::new(&rust_exe).output().expect("failed to run rust reference");

    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(String::from_utf8_lossy(&out_self.stdout), STDLIB_USE_OUT);
    assert_eq!(out_self.status.code(), Some(0));
    assert_eq!(out_rust.stdout, out_self.stdout);
    assert_eq!(out_rust.status.code(), out_self.status.code());
}

// ---- self-hosting: the Aoxn-written driver compiles its own front end ----

#[test]
fn selfhost_driver_compiles_selfhost_frontend() {
    let Some(clang) = clang_dir() else {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    };
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let demo = manifest.join("selfhost").join("driver_frontend_demo.ax");

    let exe = manifest
        .join("target")
        .join(format!("shfront-driver-{}{EXE}", std::process::id()));
    aoxn::build_paths_exe(&[demo.display().to_string()], &exe, true)
        .expect("self-host frontend demo failed to compile");

    let path = path_with_clang(&clang);
    let out = Command::new(&exe)
        .current_dir(&manifest)
        .env("PATH", &path)
        .output()
        .expect("failed to run frontend driver demo");
    let _ = std::fs::remove_file(&exe);
    assert!(
        out.status.success(),
        "frontend driver demo failed: {:?} {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "driver OK lex\ndriver OK parse\n");

    // the products are the Aoxn lexer/parser compiled by the Aoxn compiler;
    // their behavior must match the Rust-compiled demos byte for byte
    for (name, bin) in [("lex_demo", "sh_lex"), ("parse_demo", "sh_parse")] {
        let produced = manifest.join("target").join(format!("{}{EXE}", bin));
        assert!(produced.exists(), "driver did not emit {}.exe", bin);
        let out_self = Command::new(&produced)
            .current_dir(&manifest)
            .output()
            .expect("failed to run produced exe");

        let rust_exe = manifest
            .join("target")
            .join(format!("{}-rust-{}{EXE}", bin, std::process::id()));
        aoxn::build_paths_exe(
            &[manifest
                .join("selfhost")
                .join(format!("{}.ax", name))
                .display()
                .to_string()],
            &rust_exe,
            true,
        )
        .expect("rust reference compile failed");
        let out_rust = Command::new(&rust_exe)
            .current_dir(&manifest)
            .output()
            .expect("failed to run rust reference");

        let _ = std::fs::remove_file(&produced);
        let _ = std::fs::remove_file(manifest.join("target").join(format!("{}.obj", bin)));
        let _ = std::fs::remove_file(&rust_exe);
        assert!(!out_self.stdout.is_empty(), "{} produced no output", name);
        assert_eq!(out_rust.stdout, out_self.stdout, "{} output mismatch", name);
        assert_eq!(out_rust.status.code(), out_self.status.code(), "{} exit mismatch", name);
    }
}

// ---- self-hosting fixed point: the Aoxn compiler compiles itself ----

#[test]
fn selfhost_driver_self_compiles() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let Some(clang) = clang_dir() else {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    };
    let demo = manifest.join("selfhost").join("driver_self_demo.ax");

    let exe = manifest
        .join("target")
        .join(format!("shself-driver-{}{EXE}", std::process::id()));
    aoxn::build_paths_exe(&[demo.display().to_string()], &exe, true)
        .expect("self-compile demo failed to compile");

    let path = path_with_clang(&clang);
    let out = Command::new(&exe)
        .current_dir(&manifest)
        .env("PATH", &path)
        .output()
        .expect("failed to run self-compile demo");
    let _ = std::fs::remove_file(&exe);
    assert!(
        out.status.success(),
        "self-compile demo failed: {:?} {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "selfcompile OK\n");

    // stage 2: the compiler built by the Aoxn compiler compiles a stdlib
    // program 閳?the fixed point closes when its product behaves identically
    let stage2 = manifest.join("target").join(format!("selfhost_stage2{EXE}"));
    assert!(stage2.exists(), "stage-2 compiler was not emitted");
    let dir = manifest
        .join("target")
        .join(format!("shself-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("stdlib_use.ax"), STDLIB_USE_PROG).unwrap();

    let out2 = Command::new(&stage2)
        .current_dir(&dir)
        .env("PATH", &path)
        .output()
        .expect("failed to run stage-2 compiler");
    assert!(
        out2.status.success(),
        "stage-2 compiler failed: {:?} {}",
        out2.status.code(),
        String::from_utf8_lossy(&out2.stdout)
    );
    assert_eq!(String::from_utf8_lossy(&out2.stdout), "driver OK\n");

    // stage-1: the same driver built directly by the Rust compiler. The
    // fixed point is verified at the artifact level: the generated C emitted
    // by the Rust-built compiler and by the Aoxn-built compiler must be
    // byte-identical (and so must the objects clang derives from them).
    let dir1 = manifest
        .join("target")
        .join(format!("shself1-{}", std::process::id()));
    std::fs::create_dir_all(&dir1).unwrap();
    std::fs::write(dir1.join("stdlib_use.ax"), STDLIB_USE_PROG).unwrap();
    let driver1 = manifest
        .join("target")
        .join(format!("shself-driver1-{}{EXE}", std::process::id()));
    aoxn::build_paths_exe(
        &[manifest
            .join("selfhost")
            .join("driver_stdlib_demo.ax")
            .display()
            .to_string()],
        &driver1,
        true,
    )
    .expect("rust-built stdlib driver failed to compile");
    let out1 = Command::new(&driver1)
        .current_dir(&dir1)
        .env("PATH", &path)
        .output()
        .expect("failed to run rust-built driver");
    let _ = std::fs::remove_file(&driver1);
    assert!(
        out1.status.success(),
        "rust-built driver failed: {:?} {}",
        out1.status.code(),
        String::from_utf8_lossy(&out1.stdout)
    );
    assert_eq!(String::from_utf8_lossy(&out1.stdout), "driver OK\n");

    let c_stage1 = std::fs::read_to_string(dir1.join("stdlib_use.c"))
        .expect("stage-1 (rust-built driver) C text missing");
    let c_stage2 = std::fs::read_to_string(dir.join("stdlib_use.c"))
        .expect("stage-2 (Aoxn-built driver) C text missing");
    let mut obj_stage1 = std::fs::read(dir1.join("selfhost_stdlib.obj"))
        .expect("stage-1 (rust-built driver) object missing");
    let mut obj_stage2 = std::fs::read(dir.join("selfhost_stdlib.obj"))
        .expect("stage-2 (Aoxn-built driver) object missing");
    let _ = std::fs::remove_dir_all(&dir1);

    let produced = dir.join(format!("selfhost_stdlib{EXE}"));
    assert!(produced.exists(), "stage-2 compiler did not emit a product");
    let out_self = Command::new(&produced).output().expect("failed to run stage-2 product");

    let rust_exe = dir.join(format!("stdlib_use_rust{EXE}"));
    aoxn::build_paths_exe(
        &[dir.join("stdlib_use.ax").display().to_string()],
        &rust_exe,
        true,
    )
    .expect("rust reference compile failed");
    let out_rust = Command::new(&rust_exe).output().expect("failed to run rust reference");

    let _ = std::fs::remove_file(manifest.join("target").join(format!("selfhost_stage2{EXE}")));
    let _ = std::fs::remove_file(manifest.join("target").join("selfhost_stage2.obj"));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        c_stage1.contains("int main(void)") && c_stage1.contains("aoxn_main"),
        "unexpected stage-1 C shape ({} bytes)",
        c_stage1.len()
    );
    assert_eq!(
        c_stage1, c_stage2,
        "stage-1 and stage-2 generated C differ: the compiler does not reproduce itself"
    );
    // COFF TimeDateStamp (bytes 4..8) is the wall-clock time of each clang
    // run — identical C still gets a different stamp per compile, so mask it
    // before the byte-exact comparison (ELF/Mach-O objects carry no stamp).
    if cfg!(windows) {
        for obj in [&mut obj_stage1, &mut obj_stage2] {
            for b in obj.iter_mut().skip(4).take(4) {
                *b = 0;
            }
        }
    }
    if obj_stage1 != obj_stage2 {
        let at = obj_stage1
            .iter()
            .zip(&obj_stage2)
            .position(|(a, b)| a != b);
        panic!(
            "stage-1 and stage-2 objects differ ({} vs {} bytes, first diff at {:?})",
            obj_stage1.len(),
            obj_stage2.len(),
            at
        );
    }
    assert_eq!(String::from_utf8_lossy(&out_self.stdout), STDLIB_USE_OUT);
    assert_eq!(out_self.status.code(), Some(0));
    assert_eq!(out_rust.stdout, out_self.stdout);
    assert_eq!(out_rust.status.code(), out_self.status.code());
}

// ---- self-hosting: multi-file import resolution in the Aoxn front end ----

#[test]
fn selfhost_frontend_handles_imports() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let abs = |p: &str| manifest.join(p).display().to_string().replace('\\', "/");

    // fixture tree: diamond include-once, a cycle, and a missing import.
    // NB: the self-hosted loader opens paths with narrow fopen, so keep the
    // fixture directory ASCII (temp_dir can contain non-ASCII user names).
    let dir = manifest
        .join("target")
        .join(format!("shload-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.ax"),
        "import * from \"./b.ax\"\nimport * from \"./c.ax\"\n\ndef main() -> int:\n    return b_val() + c_val()\n",
    )
    .unwrap();
    std::fs::write(dir.join("b.ax"), "import * from \"./d.ax\"\n\ndef b_val() -> int:\n    return d_val() + 1\n").unwrap();
    std::fs::write(dir.join("c.ax"), "import * from \"./d.ax\"\n\ndef c_val() -> int:\n    return d_val() + 2\n").unwrap();
    std::fs::write(dir.join("d.ax"), "def d_val() -> int:\n    return 10\n").unwrap();
    std::fs::write(dir.join("cyc_a.ax"), "import * from \"./cyc_b.ax\"\n\ndef fa() -> int:\n    return 0\n").unwrap();
    std::fs::write(dir.join("cyc_b.ax"), "import * from \"./cyc_a.ax\"\n\ndef fb() -> int:\n    return 0\n").unwrap();
    std::fs::write(dir.join("miss.ax"), "import * from \"./nope.ax\"\n\ndef fm() -> int:\n    return 0\n").unwrap();
    let fp = |name: &str| dir.join(name).display().to_string().replace('\\', "/");

    // The demo is `examples/stdlib_demo.ax` with its `import * from "stdlib"`
    // rewritten to a path relative to the fixture dir: the SELF-HOSTED loader
    // (selfhost/load.ax) is repo-bound and resolves relative paths only — it
    // knows nothing about an install root. Name-based stdlib imports are
    // covered by tests/install.rs.
    let demo_src = std::fs::read_to_string(manifest.join("examples").join("stdlib_demo.ax")).unwrap();
    std::fs::write(
        dir.join("demo.ax"),
        demo_src.replace("from \"stdlib\"", "from \"../../stdlib/stdlib.ax\""),
    )
    .unwrap();

    let driver_src = format!(
        "import * from \"{}\"\nimport * from \"{}\"\nimport * from \"{}\"\nimport * from \"{}\"\n\n\
         def main() -> int:\n    \
         r1 = load_program(\"{demo}\")\n    \
         if r1.err == 1:\n        \
         print(\"demo load err: \" + r1.errmsg)\n        \
         return 1\n    \
         c1 = checker_state()\n    \
         c1.p = r1.p\n    \
         c1 = check_all(c1)\n    \
         if c1.err == 1:\n        \
         print(\"demo check err: \" + c1.errmsg)\n        \
         return 1\n    \
         print(\"demo ok files=\" + str(r1.files.len) + \" instances=\" + str(c1.inst_names.len))\n    \
         r2 = load_program(\"{diamond}\")\n    \
         if r2.err == 1:\n        \
         print(\"diamond load err: \" + r2.errmsg)\n        \
         return 1\n    \
         c2 = checker_state()\n    \
         c2.p = r2.p\n    \
         c2 = check_all(c2)\n    \
         if c2.err == 1:\n        \
         print(\"diamond check err: \" + c2.errmsg)\n        \
         return 1\n    \
         print(\"diamond ok files=\" + str(r2.files.len))\n    \
         r3 = load_program(\"{cyc}\")\n    \
         if r3.err == 0:\n        \
         print(\"BUG: cycle accepted\")\n        \
         return 1\n    \
         print(\"cycle: \" + r3.errmsg)\n    \
         r4 = load_program(\"{miss}\")\n    \
         if r4.err == 0:\n        \
         print(\"BUG: missing import accepted\")\n        \
         return 1\n    \
         print(\"missing: \" + r4.errmsg)\n    \
         return 0\n",
        abs("selfhost/lexer.ax"),
        abs("selfhost/parser.ax"),
        abs("selfhost/typecheck.ax"),
        abs("selfhost/load.ax"),
        demo = fp("demo.ax"),
        diamond = fp("main.ax"),
        cyc = fp("cyc_a.ax"),
        miss = fp("miss.ax"),
    );

    let out_dir = std::env::temp_dir().join("axon-tests");
    std::fs::create_dir_all(&out_dir).unwrap();
    let driver = out_dir.join(format!("selfhost-load-{}.ax", std::process::id()));
    let exe = out_dir.join(format!("selfhost-load-{}{EXE}", std::process::id()));
    std::fs::write(&driver, driver_src).unwrap();

    aoxn::build_paths_exe(&[driver.display().to_string()], &exe, true)
        .expect("self-host loader driver failed to compile");
    let out = Command::new(&exe).output().expect("failed to run");
    let _ = std::fs::remove_file(&exe);
    let _ = std::fs::remove_file(&driver);
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(out.status.success(), "loader driver crashed: {:?}\n{text}", out.status.code());

    assert!(text.contains("demo ok files=2 instances=10"), "{text}");
    assert!(text.contains("diamond ok files=4"), "{text}");
    assert!(text.contains("cycle: circular import"), "{text}");
    assert!(text.contains("missing: cannot open"), "{text}");
}

// ---- optimizer levels (--O0/--O1/--O2/--O3) and the `run` build cache ----

/// same as `build_and_run` but through the optimization-level API
fn build_and_run_lvl(src: &str, opt_level: u8) -> String {
    let src = &dedent(src);
    let id = COUNTER.fetch_add(1, Ordering::SeqCst) + std::process::id() as usize;
    let dir = std::env::temp_dir().join("Aoxn-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let exe: PathBuf = dir.join(format!("o{opt_level}-{id}{EXE}"));

    match aoxn::build_exe_lvl(src, &exe, opt_level) {
        Ok(()) => {}
        Err(diags) => panic!("compilation failed at O{opt_level}: {diags:?}"),
    }

    let out = Command::new(&exe).output().expect("failed to run compiled program");
    let _ = std::fs::remove_file(&exe);
    let _ = std::fs::remove_file(exe.with_extension("obj"));
    assert!(
        out.status.success(),
        "O{opt_level} program exited with {:?}, stderr: {:?}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// recursion (inlining-sensitive) + loops + strings, so every level exercises
/// both the IR pipeline and the backend
const OPT_LEVEL_PROG: &str = r#"
    def fib(n: int) -> int:
        if n < 2:
            return n
        return fib(n - 1) + fib(n - 2)

    def main() -> int:
        total = 0
        for i in range(10):
            total = total + i
        print(total)
        print(fib(12))
        s = ""
        for i in range(5):
            s = s + str(i)
        print(s)
        return 0
"#;

#[test]
fn optimization_levels_agree_on_program_output() {
    // correctness must not depend on the level: O0..O3 all produce the same
    // program behavior (only code quality and compile time differ)
    let expected = build_and_run_lvl(OPT_LEVEL_PROG, 3);
    assert_eq!(expected, "45\n144\n01234\n");
    for level in [0u8, 1, 2] {
        assert_eq!(
            build_and_run_lvl(OPT_LEVEL_PROG, level),
            expected,
            "program output at O{level} differs from O3"
        );
    }
}

#[test]
fn c_text_is_opt_level_independent() {
    let entry = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("fib.ax")
        .display()
        .to_string();
    let o0 = aoxn::compile_paths_to_c_lvl(&[entry.clone()], 0).expect("O0 C text failed");
    let o1 = aoxn::compile_paths_to_c_lvl(&[entry.clone()], 1).expect("O1 C text failed");
    let o2 = aoxn::compile_paths_to_c_lvl(&[entry.clone()], 2).expect("O2 C text failed");
    let o3 = aoxn::compile_paths_to_c_lvl(&[entry], 3).expect("O3 C text failed");
    // the generated C does not depend on the optimization level: the level
    // selects the clang `-O` flags used when the text is compiled to an object
    assert_eq!(o0, o3, "O0 C text should match O3 C text");
    assert_eq!(o1, o3, "O1 C text should match O3 C text");
    assert_eq!(o2, o3, "O2 C text should match O3 C text");
}

#[test]
fn dependency_files_follows_import_chain() {
    let base = std::env::temp_dir().join(format!("aoxn-deps-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("lib.ax"), "def helper() -> int:\n    return 1\n").unwrap();
    std::fs::write(
        base.join("mid.ax"),
        "import * from \"./lib.ax\"\n\ndef mid() -> int:\n    return helper()\n",
    )
    .unwrap();
    std::fs::write(
        base.join("main.ax"),
        "import * from \"./mid.ax\"\n\ndef main() -> int:\n    print(mid())\n    return 0\n",
    )
    .unwrap();

    // the build-cache key must cover the whole transitive import set
    let entry = base.join("main.ax").display().to_string();
    let files = aoxn::dependency_files(&[entry]).expect("dependency scan failed");
    let names: Vec<String> = files
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["lib.ax", "main.ax", "mid.ax"], "transitive deps missing");

    // The bare Python-style specifier resolves against the importing file's
    // OWN directory, which the dependency scan only knows because
    // `scan_imports` reports whether a specifier was quoted. Miss that and
    // editing `util.ax` silently serves a stale exe.
    std::fs::write(base.join("util.ax"), "def twice(n: int) -> int:\n    return n * 2\n").unwrap();
    std::fs::write(
        base.join("bare.ax"),
        "import * from \"./lib.ax\"\nfrom util import twice\n\ndef main() -> int:\n    print(twice(2))\n    return 0\n",
    )
    .unwrap();
    let files = aoxn::dependency_files(&[base.join("bare.ax").display().to_string()])
        .expect("dependency scan failed");
    let names: Vec<String> = files
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(names.contains(&"util.ax".to_string()), "bare sibling import missing from the cache key: {names:?}");
    assert!(names.contains(&"lib.ax".to_string()), "quoted sibling import missing from the cache key: {names:?}");
    let _ = std::fs::remove_file(base.join("bare.ax"));

    // a missing entry disables the cache instead of producing a wrong key
    assert!(aoxn::dependency_files(&[base.join("nope.ax").display().to_string()]).is_none());

    // and the same program really does compile + run through the chain
    let exe = base.join(format!("main{EXE}"));
    aoxn::build_paths_opts_lvl(&[base.join("main.ax").display().to_string()], &exe, 3, &[], &[])
        .expect("compile through import chain failed");
    let out = Command::new(&exe).output().expect("failed to run");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n");
    let _ = std::fs::remove_dir_all(&base);
}

/// v0.29.1: a bare package import resolves its entry through the package's
/// `aoxn_modules/<pkg>/aoxn.json` `main` field, so a package whose entry is
/// not `index.ax` is importable. Before v0.29.1 this failed (the loader only
/// probed `aox_modules/<name>` for `index.ax`).
#[test]
fn pkg_manifest_main_entry_resolution() {
    let base = std::env::temp_dir().join(format!("Aoxn-pkg-main-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("aox_modules/mypkg/src")).unwrap();
    std::fs::write(
        base.join("aox_modules/mypkg/aoxn.json"),
        r#"{"name":"mypkg","version":"0.1.0","main":"src/lib.ax"}"#,
    )
    .unwrap();
    std::fs::write(
        base.join("aox_modules/mypkg/src/lib.ax"),
        "def pkg_value() -> int:\n    return 42\n",
    )
    .unwrap();
    std::fs::write(
        base.join("main.ax"),
        "import * from \"mypkg\"\n\ndef main() -> int:\n    print(pkg_value())\n    return 0\n",
    )
    .unwrap();

    let exe = base.join(format!("main{EXE}"));
    aoxn::build_paths_opts_lvl(&[base.join("main.ax").display().to_string()], &exe, 3, &[], &[])
        .expect("package import via manifest `main` should compile");
    let out = Command::new(&exe).output().expect("failed to run");
    assert!(out.status.success(), "stderr: {:?}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
    let _ = std::fs::remove_dir_all(&base);
}

/// v0.29.1: `exports` gates subpath imports — `import * from "mypkg/sub"`
/// resolves through `exports["./sub"]`, not a directory probe.
#[test]
fn pkg_manifest_subpath_exports() {
    let base = std::env::temp_dir().join(format!("Aoxn-pkg-sub-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("aox_modules/mypkg/src")).unwrap();
    std::fs::write(
        base.join("aox_modules/mypkg/aoxn.json"),
        r#"{"name":"mypkg","version":"0.1.0","exports":{".":"src/lib.ax","./sub":"src/sub.ax"}}"#,
    )
    .unwrap();
    std::fs::write(
        base.join("aox_modules/mypkg/src/lib.ax"),
        "def root_value() -> int:\n    return 7\n",
    )
    .unwrap();
    std::fs::write(
        base.join("aox_modules/mypkg/src/sub.ax"),
        "def sub_value() -> int:\n    return 9\n",
    )
    .unwrap();
    std::fs::write(
        base.join("main.ax"),
        "import * from \"mypkg\"\nimport * from \"mypkg/sub\"\n\ndef main() -> int:\n    print(root_value() + sub_value())\n    return 0\n",
    )
    .unwrap();

    let exe = base.join(format!("main{EXE}"));
    aoxn::build_paths_opts_lvl(&[base.join("main.ax").display().to_string()], &exe, 3, &[], &[])
        .expect("subpath export should compile");
    let out = Command::new(&exe).output().expect("failed to run");
    assert!(out.status.success(), "stderr: {:?}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "16\n");
    let _ = std::fs::remove_dir_all(&base);
}

/// v0.29.1 back-compat: a package without `aoxn.json` still imports via the
/// legacy `aox_modules/<name>/index.ax` probe — the manifest reader returns
/// None and `resolve_import` falls through.
#[test]
fn pkg_no_manifest_falls_back_to_probe() {
    let base = std::env::temp_dir().join(format!("Aoxn-pkg-legacy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("aox_modules/legacypkg")).unwrap();
    std::fs::write(
        base.join("aox_modules/legacypkg/index.ax"),
        "def legacy_value() -> int:\n    return 5\n",
    )
    .unwrap();
    std::fs::write(
        base.join("main.ax"),
        "import * from \"legacypkg\"\n\ndef main() -> int:\n    print(legacy_value())\n    return 0\n",
    )
    .unwrap();

    let exe = base.join(format!("main{EXE}"));
    aoxn::build_paths_opts_lvl(&[base.join("main.ax").display().to_string()], &exe, 3, &[], &[])
        .expect("legacy probe should still compile");
    let out = Command::new(&exe).output().expect("failed to run");
    assert!(out.status.success(), "stderr: {:?}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "5\n");
    let _ = std::fs::remove_dir_all(&base);
}

// ---- Python-parity syntax (v0.29.6) ----

/// `x += e` (and the rest of the family) is exactly `x = x + e`, on every
/// assignment target Python allows: a plain name, an array slot, a struct
/// field. Strings concatenate; floats stay float.
#[test]
fn augmented_assignment_python_style() {
    let out = build_and_run(
        r#"
        struct Counter:
            total: int

        def main() -> int:
            x = 5
            x += 3
            x -= 1
            x *= 4
            x /= 7
            x %= 3

            s = "ab"
            s += "cd"
            s += "!"

            f = 1.5
            f *= 2.0

            arr = [1, 2, 3]
            arr[0] += 10
            arr[2] *= 5

            c = Counter(total=1)
            c.total += 41

            print(x)
            print(s)
            print(f)
            print(arr[0])
            print(arr[2])
            print(c.total)
            return 0
        "#,
    );
    // x: 5+3=8, -1=7, *4=28, /7=4, %3=1
    assert_eq!(out, "1\nabcd!\n3.000000\n11\n15\n42\n");
}

/// augmented assignment on an undeclared name is still an error: the
/// expansion reads the variable before writing it.
#[test]
fn augmented_assignment_undeclared_is_an_error() {
    let msg = expect_compile_error(
        r#"
        def main() -> int:
            y += 1
            return 0
        "#,
    );
    assert!(msg.contains("unknown variable 'y'"), "{msg}");
}

/// `//` is the Python spelling of integer division; on two ints it is a
/// synonym of `/`, which already truncates.
#[test]
fn integer_division_slashes() {
    let out = build_and_run(
        r#"
        def main() -> int:
            print(7 // 2)
            print(-7 // 2)
            print(7 / 2)
            print(1 // 2)
            return 0
        "#,
    );
    assert_eq!(out, "3\n-3\n3\n0\n");
}

/// unary `+` is the identity, as in Python
#[test]
fn unary_plus_is_identity() {
    let out = build_and_run(
        r#"
        def main() -> int:
            x = 5
            print(+x)
            print(+(-3))
            f = 2.5
            print(+f)
            return 0
        "#,
    );
    assert_eq!(out, "5\n-3\n2.500000\n");
}

/// `a < b <= c` means `(a < b) and (b <= c)` and short-circuits: the
/// right-hand comparison is never evaluated once the left one is false.
#[test]
fn chained_comparison_python_style() {
    let out = build_and_run(
        r#"
        def blow_up(x: int) -> int:
            if x < 0:
                return -1
            return x

        def main() -> int:
            lo = 0
            mid = 5
            hi = 10
            print(lo < mid <= hi)     # true
            print(lo < mid >= hi)     # false
            print(lo <= lo <= lo)     # true
            print(hi > lo >= mid)     # false: the shared operand is `lo`, so
                                      # the second link is 0 >= 5
            # short-circuit: the false left side stops the chain before
            # blow_up is ever called (it would return -1 and break the chain)
            print(-5 < blow_up(1) < blow_up(2))   # true
            return 0
        "#,
    );
    assert_eq!(out, "true\nfalse\ntrue\nfalse\ntrue\n");
}

/// the operands of a chain are still type-checked pairwise: mixing a string
/// into one link of the chain is an error
#[test]
fn chained_comparison_type_error() {
    let msg = expect_compile_error(
        r#"
        def main() -> int:
            a = 1
            b = "x"
            c = 3
            print(a < b <= c)
            return 0
        "#,
    );
    assert!(msg.contains("requires two int"), "{msg}");
}

/// a chain over three links folds to short-circuiting `&&` in the emitted C
#[test]
fn chained_comparison_emits_short_circuit_and() {
    let c = aoxn::compile_to_c(
        &dedent(
            r#"
        def main() -> int:
            a = 1
            b = 2
            c = 3
            if a < b < c:
                return 1
            return 0
        "#,
        ),
        true,
    )
    .expect("chained comparison should compile");
    assert!(c.contains("&&"), "{c}");
}

// ---- v0.40.0: dict ------------------------------------------------------
//
// A dict is a HANDLE (`struct ax_dict_V*`). It was originally spelled as a
// 4-word struct passed by value, which was unsound twice over: a callee's
// growth/deletion never reached the caller, and the stale `len` then walked
// off a reallocated buffer — correct at -O0, a segfault at -O1 and above.
// The tests below run at -O3 by construction (`build_exe(.., true)`), so
// they are the regression net for exactly that.

/// insert / read / len / overwrite
#[test]
fn dict_basics() {
    let out = build_and_run(
        r#"
        def main() -> int:
            d: dict[int] = {"a": 1, "b": 2}
            print(d["a"])
            print(d["b"])
            print(len(d))
            d["a"] = 10
            print(d["a"])
            print(len(d))
            return 0
        "#,
    );
    assert_eq!(out, "1\n2\n2\n10\n2\n");
}

/// THE regression test: a mutation made through a handle passed to another
/// function is visible to the caller. Under the by-value spelling the callee
/// grew its own copy and the caller kept the old len.
#[test]
fn dict_mutation_through_a_callee_is_visible_to_the_caller() {
    let out = build_and_run(
        r#"
        def put(d: dict[int], k: string, v: int) -> void:
            d[k] = v

        def main() -> int:
            d: dict[int] = {}
            put(d, "x", 7)
            print(len(d))
            print(d["x"])
            i = 0
            while i < 10:
                put(d, str(i), i)
                i = i + 1
            print(len(d))
            print(d["9"])
            return 0
        "#,
    );
    assert_eq!(out, "1\n7\n11\n9\n");
}

/// `dict_del` used to shrink a copy: the caller still saw the key.
#[test]
fn dict_del_is_visible_to_the_caller() {
    let out = build_and_run(
        r#"
        def drop(d: dict[int], k: string) -> void:
            dict_del(d, k)

        def main() -> int:
            d: dict[int] = {"a": 1, "b": 2, "c": 3}
            print(dict_del(d, "b"))
            print(len(d))
            print(dict_has(d, "b"))
            print(d["a"] + d["c"])
            print(dict_del(d, "nope"))
            print(len(d))
            drop(d, "a")
            print(len(d))
            print(dict_has(d, "a"))
            return 0
        "#,
    );
    assert_eq!(out, "true\n2\nfalse\n4\nfalse\n2\n1\nfalse\n");
}

/// growth past the initial capacity, which is where the stale `len` used to
/// read past the end of the reallocated arrays
#[test]
fn dict_grows_past_the_initial_capacity() {
    let out = build_and_run(
        r#"
        def main() -> int:
            d: dict[int] = {}
            i = 0
            while i < 200:
                d[str(i)] = i * i
                i = i + 1
            print(len(d))
            print(d["0"])
            print(d["199"])
            return 0
        "#,
    );
    assert_eq!(out, "200\n0\n39601\n");
}

/// `for k in d` walks KEYS, in insertion order
#[test]
fn dict_iteration_walks_keys_in_insertion_order() {
    let out = build_and_run(
        r#"
        def main() -> int:
            d: dict[int] = {"z": 1, "a": 2, "m": 3}
            parts = ""
            for k in d:
                parts = parts + k
            print(parts)
            total = 0
            for k in d:
                total = total + d[k]
            print(total)
            return 0
        "#,
    );
    assert_eq!(out, "zam\n6\n");
}

/// a missing key raises; `dict_has` is the guard
#[test]
fn dict_missing_key_raises() {
    let out = build_and_run(
        r#"
        def main() -> int:
            d: dict[string] = {"name": "aoxn"}
            print(dict_has(d, "name"))
            print(dict_has(d, "nope"))
            try:
                print(d["nope"])
            except as e:
                print("caught: " + e)
            return 0
        "#,
    );
    assert_eq!(out, "true\nfalse\n\ncaught: dict key not found\n");
}

/// dicts of strings, and of structs (the value array holds a by-value struct)
#[test]
fn dict_of_strings_and_of_structs() {
    let out = build_and_run(
        r#"
        struct P:
            x: int
            y: int

        def main() -> int:
            s: dict[string] = {"name": "aoxn", "kind": "language"}
            print(s["name"] + "/" + s["kind"])
            print(len(s))
            p: dict[P] = {}
            p["origin"] = P(x=1, y=2)
            p["far"] = P(x=10, y=20)
            o = p["far"]
            print(o.x + o.y)
            print(len(p))
            return 0
        "#,
    );
    assert_eq!(out, "aoxn/language\n2\n30\n2\n");
}

/// non-string keys are rejected
#[test]
fn dict_key_must_be_a_string() {
    let msg = expect_compile_error(
        r#"
        def main() -> int:
            d: dict[int] = {}
            i = 1
            d[i] = 1
            return 0
        "#,
    );
    assert!(msg.contains("dict key must be a string"), "{msg}");
}

/// all values share one type
#[test]
fn dict_values_must_share_one_type() {
    let msg = expect_compile_error(
        r#"
        def main() -> int:
            d: dict[int] = {"a": 1, "b": "two"}
            print(len(d))
            return 0
        "#,
    );
    assert!(msg.contains("must share one type"), "{msg}");
}

/// `{}` cannot infer its value type — it needs the annotation
#[test]
fn empty_dict_literal_needs_an_annotation() {
    let msg = expect_compile_error(
        r#"
        def main() -> int:
            d = {}
            print(len(d))
            return 0
        "#,
    );
    assert!(msg.contains("needs an annotation"), "{msg}");
}

/// a dict is emitted as a pointer, not a by-value struct
#[test]
fn dict_is_emitted_as_a_handle() {
    let c = aoxn::compile_to_c(
        &dedent(
            r#"
        def main() -> int:
            d: dict[int] = {"a": 1}
            print(len(d))
            return 0
        "#,
        ),
        true,
    )
    .expect("dict program should compile");
    assert!(c.contains("struct ax_dict_i*"), "{c}");
}

// ---- v0.40.0: None / raise / try / except / fn pointers ---------------

/// `T | None` is a carrier struct with a presence tag
#[test]
fn none_optional_round_trip() {
    let out = build_and_run(
        r#"
        def main() -> int:
            a: int | None = None
            print(a is None)
            b: int | None = 7
            print(b is None)
            if b is not None:
                print(b + 1)
            return 0
        "#,
    );
    assert_eq!(out, "true\nfalse\n8\n");
}

/// a raise unwinds to the nearest handler, across frames; the normal path
/// still works afterwards
#[test]
fn raise_unwinds_to_the_nearest_handler() {
    let out = build_and_run(
        r#"
        def risky(n: int) -> int:
            if n < 0:
                raise "negative input: " + str(n)
            return n * 2

        def wrapper(n: int) -> int:
            return risky(n)

        def main() -> int:
            try:
                print(risky(5))
                raise "boom"
            except as e:
                print("caught: " + e)
            try:
                print(wrapper(-3))
            except as e:
                print("crossed frames: " + e)
            try:
                print(wrapper(4))
            except:
                print("never printed")
            print(risky(21))
            return 0
        "#,
    );
    // The `0` between the catch and "crossed frames" is CURRENT semantics,
    // pinned on purpose: a raise takes effect at the next propagation check,
    // and `print(wrapper(-3))` completes before its statement's check runs —
    // the unwound frame returns the zero value and printf prints it. Fixing
    // that means hoisting raising arguments out of the side-effecting call
    // (in codegen_c.rs AND its selfhost mirror), not tightening this test.
    assert_eq!(
        out,
        "10\ncaught: boom\n0\ncrossed frames: negative input: -3\n8\n42\n"
    );
}

/// a function used as a value and called indirectly; a pointer copies
#[test]
fn fnptr_call_through_a_variable() {
    let out = build_and_run(
        r#"
        def twice(x: int) -> int:
            return x * 2

        def add(a: int, b: int) -> int:
            return a + b

        def main() -> int:
            f = twice
            print(f(21))
            g = add
            print(g(20, 22))
            h = f
            print(h(5))
            print(to_int(twice) != 0)
            print(to_int(f) == to_int(h))
            return 0
        "#,
    );
    assert_eq!(out, "42\n42\n10\ntrue\ntrue\n");
}

// ---------------------------------------------------------------------------
// v0.42.0: radix literals, the two new escapes, assert/exit, argv, warnings
// ---------------------------------------------------------------------------

/// Build and run a program that is EXPECTED to fail, returning
/// (exit code, stdout, stderr).
fn build_and_run_status(src: &str, args: &[&str]) -> (Option<i32>, String, String) {
    let src = &dedent(src);
    let id = COUNTER.fetch_add(1, Ordering::SeqCst) + std::process::id() as usize;
    let dir = std::env::temp_dir().join("Aoxn-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let exe: PathBuf = dir.join(format!("t{id}{EXE}"));

    match build_exe(src, &exe, true) {
        Ok(()) => {}
        Err(diags) => panic!("compilation failed: {:?}", diags),
    }
    let out = Command::new(&exe).args(args).output().expect("failed to run compiled program");
    let _ = std::fs::remove_file(&exe);
    let _ = std::fs::remove_file(exe.with_extension("obj"));
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn radix_integer_literals() {
    let out = build_and_run(
        r#"
        def main() -> int:
            print(0x10)            # 16
            print(0X1f)            # 31
            print(0b101)           # 5
            print(0B1111)          # 15
            print(0xff + 1)        # 256
            print(0b1 << 8)        # 256 — usable in expressions, not just print
            return 0
        "#,
    );
    assert_eq!(out, "16\n31\n5\n15\n256\n256\n");
}

#[test]
fn radix_literal_diagnostics() {
    // a malformed literal names the offending digit instead of splitting into
    // `0` + an identifier ("unknown variable 'x10'", which is what v0.41 did)
    let msg = expect_compile_error("def main():\n    print(0xZZ)\n");
    assert!(
        msg.contains("invalid digit 'Z' in hexadecimal literal"),
        "unexpected message: {msg}"
    );
    let msg = expect_compile_error("def main():\n    print(0b12)\n");
    assert!(msg.contains("invalid digit '2' in binary literal"), "unexpected message: {msg}");
    let msg = expect_compile_error("def main():\n    print(0x)\n");
    assert!(msg.contains("needs at least one digit"), "unexpected message: {msg}");
    let msg = expect_compile_error("def main():\n    print(0xFFFFFFFFFFFFFFFF)\n");
    assert!(msg.contains("out of range"), "unexpected message: {msg}");
}

#[test]
fn string_escapes_cr_and_nul() {
    let out = build_and_run(
        r#"
        def main() -> int:
            s = "a\rb"
            print(len(s))            # 3 bytes: the CR is a real byte
            print(load_u8(s, 1))     # ... and it is 13
            print(load_u8(s, 0))     # 'a'
            t = "x\0y"
            # len() is strlen: everything after the NUL is invisible to the
            # string builtins, which is why \0 is for byte buffers
            print(len(t))
            print(load_u8(t, 1))
            print(load_u8(t, 2))
            return 0
        "#,
    );
    assert_eq!(out, "3\n13\n97\n1\n0\n121\n");
}

#[test]
fn assert_passes_silently() {
    let out = build_and_run(
        r#"
        def main() -> int:
            assert(1 + 1 == 2)
            assert(len("abc") == 3, "length is bytes")
            print("still here")
            return 0
        "#,
    );
    assert_eq!(out, "still here\n");
}

#[test]
fn assert_failure_reports_the_line_and_exits_1() {
    let (code, stdout, _stderr) = build_and_run_status(
        r#"
        def main() -> int:
            print("before")
            assert(1 == 2, "one is not two")
            print("unreachable in practice")
            return 0
        "#,
        &[],
    );
    assert_eq!(code, Some(1));
    // the report goes to stdout, like the uncaught-exception report and like
    // `print` itself (the runtime never writes to stderr)
    assert_eq!(stdout, "before\nassertion failed at line 3: one is not two\n");
}

#[test]
fn exit_sets_the_process_status() {
    let (code, stdout, _stderr) = build_and_run_status(
        r#"
        def main():
            print("leaving early")
            exit(3)
        "#,
        &[],
    );
    assert_eq!(code, Some(3));
    assert_eq!(stdout, "leaving early\n");
}

#[test]
fn argv_builtins_read_the_command_line() {
    let (code, stdout, _stderr) = build_and_run_status(
        r#"
        def main():
            print(argc())
            i = 0
            while i < argc():
                print(arg(i))
                i = i + 1
        "#,
        &["alpha", "beta"],
    );
    assert_eq!(code, Some(0));
    assert_eq!(stdout, "2\nalpha\nbeta\n");
}

#[test]
fn arg_without_arguments_and_past_the_end_is_empty() {
    let (code, stdout, _stderr) = build_and_run_status(
        r#"
        def main():
            print(argc())
            print("[" + arg(0) + "]")     # out of range: "" (never a crash)
            print("[" + arg(-1) + "]")
        "#,
        &[],
    );
    assert_eq!(code, Some(0));
    assert_eq!(stdout, "0\n[]\n[]\n");
}

#[test]
fn main_signature_is_validated() {
    // v0.41 let this through the checker and then died inside clang as an
    // `internal` error; it is a source-level mistake and says so now
    let msg = expect_compile_error("def main(argc: int) -> int:\n    return 0\n");
    assert!(msg.contains("'main' cannot take parameters"), "unexpected message: {msg}");
    assert!(msg.contains("argc()"), "the message must name the replacement: {msg}");
    let msg = expect_compile_error("def main() -> string:\n    return \"x\"\n");
    assert!(msg.contains("'main' must return int"), "unexpected message: {msg}");
    let msg = expect_compile_error("def main[T]() -> int:\n    return 0\n");
    assert!(msg.contains("'main' cannot be generic"), "unexpected message: {msg}");
}

#[test]
fn unused_local_warns_without_failing_the_build() {
    let src = dedent(
        r#"
        def helper() -> int:
            unused = 41
            other = 1
            return other

        def main():
            print(helper())
        "#,
    );
    let id = COUNTER.fetch_add(1, Ordering::SeqCst) + std::process::id() as usize;
    let dir = std::env::temp_dir().join("Aoxn-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let exe: PathBuf = dir.join(format!("t{id}{EXE}"));
    let _ = aoxn::take_warnings(); // start clean on this thread
    build_exe(&src, &exe, true).expect("a warning must not fail the compile");
    let _ = std::fs::remove_file(&exe);
    let warnings = aoxn::take_warnings();
    assert_eq!(warnings.len(), 1, "expected exactly one warning, got {warnings:?}");
    let w = &warnings[0];
    assert_eq!(w.severity, aoxn::Severity::Warning);
    assert_eq!(w.code, Some("W001"));
    assert!(w.message.contains("unused variable 'unused'"), "{}", w.message);
    assert_eq!(w.line, 2, "the warning points at the binding");
}

#[test]
fn a_read_local_does_not_warn() {
    let src = dedent(
        r#"
        def main():
            total = 0
            i = 0
            while i < 3:
                total = total + i
                i = i + 1
            print(total)
        "#,
    );
    let id = COUNTER.fetch_add(1, Ordering::SeqCst) + std::process::id() as usize;
    let dir = std::env::temp_dir().join("Aoxn-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let exe: PathBuf = dir.join(format!("t{id}{EXE}"));
    let _ = aoxn::take_warnings();
    build_exe(&src, &exe, true).expect("compile");
    let _ = std::fs::remove_file(&exe);
    assert!(aoxn::take_warnings().is_empty(), "augmented/loop reads count as uses");
}

#[test]
fn warning_json_has_a_severity_and_a_code() {
    let w = aoxn::Diag::warn("type", u32::MAX, 7, 3, "W001", "unused variable 'x'");
    let json = aoxn::report_to_json(&[], &[w]);
    assert!(json.contains("\"ok\":true"), "{json}");
    assert!(json.contains("\"severity\":\"warning\""), "{json}");
    assert!(json.contains("\"code\":\"W001\""), "{json}");
    // errors and warnings live in separate arrays
    let e = aoxn::Diag::at("type", u32::MAX, 1, 1, "boom");
    let json = aoxn::report_to_json(&[e], &[aoxn::Diag::warn("type", u32::MAX, 2, 2, "W001", "w")]);
    assert!(json.contains("\"ok\":false"), "{json}");
    assert_eq!(json.matches("\"severity\":\"error\"").count(), 1, "{json}");
    assert_eq!(json.matches("\"severity\":\"warning\"").count(), 1, "{json}");
}

/// A user `extern def` that contradicts a declaration the backend already
/// emitted is a C-compiler failure, not an internal one: the diagnostic must
/// be attributed to the `cc` stage (v0.42.0). `_setmode` is the cheapest such
/// contradiction — the wrapper `main` declares it with two parameters on
/// Windows only, so this assertion is Windows-scoped like the platform is.
#[cfg(windows)]
#[test]
fn c_backend_failures_are_reported_as_the_cc_stage() {
    let src = dedent(
        r#"
        extern def _setmode(a: int) -> int

        def main():
            print(_setmode(1))
        "#,
    );
    let id = COUNTER.fetch_add(1, Ordering::SeqCst) + std::process::id() as usize;
    let dir = std::env::temp_dir().join("Aoxn-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let exe: PathBuf = dir.join(format!("t{id}{EXE}"));
    let diags = build_exe(&src, &exe, true).expect_err("clang must reject the conflicting prototype");
    assert_eq!(diags[0].stage, "cc", "unexpected stage: {diags:?}");
    assert!(!diags[0].message.contains("internal error"), "{}", diags[0].message);
}

#[test]
fn clang_arg_list_parsing() {
    // newline-separated, so one flag may contain spaces
    let args = aoxn::parse_clang_args("-g\n-Xclang -load\n\n");
    assert_eq!(args, vec!["-g".to_string(), "-Xclang -load".to_string()]);
    assert!(aoxn::parse_clang_args("").is_empty());
    assert!(aoxn::parse_clang_args("\n  \n").is_empty());
}
