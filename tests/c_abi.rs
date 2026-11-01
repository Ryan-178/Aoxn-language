//! Native ABI tests against separately compiled, fixed-prototype C definitions.
//! Aoxn int is C long long, NOT C int. The current backend maps bool to
//! C int (not _Bool), float to double and string to char*. No variadic FFI.
//! Aggregates below test Aoxn's INTERNAL value ABI, not C struct interop.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "aoxn-c-abi-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn checked(cmd: &mut Command, stage: &str) -> std::process::Output {
    let out = cmd.output().unwrap_or_else(|e| panic!("{stage}: {e}"));
    assert!(out.status.success(), "{stage}: {:?}\nstdout: {}\nstderr: {}",
        out.status.code(), String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    out
}

fn run_native(src: &str, helper: Option<&str>, marker: &str) {
    let Some(clang) = aoxn::find_clang() else {
        assert!(
            std::env::var_os("AOXN_TEST_REQUIRE_CLANG").is_none(),
            "{marker}: clang is required for this test run; set AOXN_CLANG"
        );
        eprintln!("SKIP {marker}: clang not found; set AOXN_CLANG");
        return;
    };
    // Exercise both unoptimized and optimized calling/copying code.
    for level in [0, 3] {
        let scratch = Scratch::new();
        let obj = scratch.0.join("program").with_extension(aoxn::platform::obj_ext());
        let exe = scratch.0.join("program").with_extension(aoxn::platform::exe_ext());
        aoxn::compile_to_object_lvl(src, &obj, level)
            .unwrap_or_else(|d| panic!("{marker} O{level}: {d:?}"));
        if let Some(c) = helper {
            let c_path = scratch.0.join("helper.c");
            let c_obj = scratch.0.join("helper").with_extension(aoxn::platform::obj_ext());
            std::fs::write(&c_path, c).unwrap();
            checked(Command::new(&clang).args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-Wstrict-prototypes"])
                .arg(format!("-O{level}")).arg("-c").arg(&c_path).arg("-o").arg(&c_obj), "compile C helper");
            // This revision has no Options/extra_objects API. Use its public
            // object compiler, then the same driver/host link flags as link_opts,
            // with BOTH real objects; no process-global AOXN_CLANG_ARGS mutation.
            let mut link = Command::new(&clang);
            link.arg(&obj).arg(&c_obj).arg("-o").arg(&exe);
            if let Some(flag) = aoxn::platform::stack_link_flag() { link.arg(flag); }
            link.args(aoxn::extra_clang_args());
            for lib in aoxn::platform::default_link_libs() { link.arg(format!("-l{lib}")); }
            checked(&mut link, "link Aoxn and C objects");
        } else {
            aoxn::link(&obj, &exe).unwrap_or_else(|d| panic!("link {marker}: {d:?}"));
        }
        let out = checked(&mut Command::new(&exe), marker);
        assert_eq!(String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"), format!("{marker}\n"));
        eprintln!("EXECUTED {marker} O{level} with {}", clang.display());
    }
}

const C_HELPER: &str = r#"
_Static_assert(sizeof(long long) == 8, "Aoxn int needs 64-bit long long");
_Static_assert(sizeof(double) == 8, "Aoxn float needs 64-bit double");
long long abi_integer(long long x) { return x - 4294967296LL; }
double abi_float(double x) { return x * 2.0 - 0.5; }
/* Deliberately int: this is the backend's bool ABI, not C _Bool. */
int abi_boolean(int x) { return x == 0 ? 1 : 0; }
char *abi_string(char *x) { return x + 2; }
long long abi_positions(long long a, double b, int c, char *d,
                        double e, long long f, char *g, int h,
                        long long i, double j, int k, char *l) {
    return a == -4294967301LL && b == 1.25 && c == 1 && d[0] == 'A' &&
           e == -2.5 && f == 8589934609LL && g[0] == 'B' && h == 0 &&
           i == -17179869187LL && j == 8.125 && k == 1 && l[0] == 'C';
}
long long abi_zero(void) { return -8589934593LL; }
double (*abi_factory(int enabled))(double) {
    return enabled ? abi_float : (double (*)(double))0;
}
long long (*abi_zero_factory(void))(void) { return abi_zero; }
"#;

#[test]
fn extern_scalars_and_register_stack_positions() {
    run_native(r#"
extern def abi_integer(x: int) -> int
extern def abi_float(x: float) -> float
extern def abi_boolean(x: bool) -> bool
extern def abi_string(x: string) -> string
extern def abi_positions(a: int, b: float, c: bool, d: string, e: float, f: int, g: string, h: bool, i: int, j: float, k: bool, l: string) -> int
extern def abi_zero() -> int
extern def abi_factory(enabled: bool) -> fn(float) -> float
extern def abi_zero_factory() -> fn() -> int

def main() -> int:
    assert(abi_integer(-8589934593) == -12884901889)
    assert(abi_float(-3.25) == -7.0)
    assert(abi_boolean(False))
    assert(not abi_boolean(True))
    assert(abi_string("xxnative") == "native")
    assert(abi_string("xx") == "")
    assert(abi_positions(-4294967301, 1.25, True, "Alpha", -2.5, 8589934609, "Beta", False, -17179869187, 8.125, True, "Charlie") == 1)
    assert(abi_zero() == -8589934593)
    callback = abi_factory(True)
    assert(callback(-3.25) == -7.0)
    zero_callback = abi_zero_factory()
    assert(zero_callback() == -8589934593)
    print("PASS extern-scalars")
    return 0
"#, Some(C_HELPER), "PASS extern-scalars");
}

#[test]
fn internal_struct_array_copy_parameters_and_returns() {
    run_native(r#"
struct Record:
    n: int
    f: float
    b: bool
    s: string
    items: [int; 3]

def change_record(r: Record, delta: int) -> Record:
    r.n += delta
    r.f = -2.5
    r.b = False
    r.s = "changed"
    r.items[1] = 99
    return r

def change_array(a: [int; 3], delta: int) -> [int; 3]:
    a[0] += delta
    a[2] = -8589934593
    return a

def main() -> int:
    original = Record(n=4294967301, f=1.25, b=True, s="original", items=[11, 22, 33])
    copied = original
    copied.n = 7
    copied.items[0] = 88
    assert(original.n == 4294967301)
    assert(original.items[0] == 11)
    changed = change_record(original, 9)
    assert(changed.n == 4294967310)
    assert(changed.f == -2.5 and not changed.b and changed.s == "changed")
    assert(changed.items[0] == 11 and changed.items[1] == 99 and changed.items[2] == 33)
    assert(original.n == 4294967301 and original.f == 1.25 and original.b and original.s == "original")
    assert(original.items[1] == 22)
    changed.items[0] = 100
    assert(original.items[0] == 11)
    a = [1, 2, 3]
    b = a
    b[1] = 77
    assert(a[1] == 2)
    c = change_array(a, 10)
    assert(c[0] == 11 and c[1] == 2 and c[2] == -8589934593)
    assert(a[0] == 1 and a[2] == 3)
    c[1] = 90
    assert(a[1] == 2 and b[1] == 77)
    print("PASS internal-aggregates")
    return 0
"#, None, "PASS internal-aggregates");
}

#[test]
fn function_pointers_explicit_scalar_and_zero_argument_signatures() {
    run_native(r#"
def integer(x: int) -> int:
    return x - 4294967296

def floating(x: float) -> float:
    return x * 2.0

def boolean(x: bool) -> bool:
    return not x

def text(x: string) -> string:
    return x + "!"

def zero() -> int:
    return -8589934593

def mixed(a: int, b: float, c: bool, d: string, e: int, f: float) -> string:
    assert(a == -4294967301 and b == 1.25 and c and d == "mixed" and e == 8589934609 and f == -2.5)
    return d

def invoke(cb: fn(int) -> int, x: int) -> int:
    return cb(x)

def choose() -> fn(int) -> int:
    return integer

def choose_float(enabled: bool) -> fn(float) -> float:
    assert(enabled)
    return floating

def choose_zero() -> fn() -> int:
    return zero

def main() -> int:
    fi: fn(int) -> int = integer
    ff: fn(float) -> float = floating
    fb: fn(bool) -> bool = boolean
    fs: fn(string) -> string = text
    fz: fn() -> int = zero
    fm: fn(int, float, bool, string, int, float) -> string = mixed
    assert(fi(-8589934593) == -12884901889)
    assert(ff(-3.25) == -6.5)
    assert(fb(False) and not fb(True))
    assert(fs("callback") == "callback!")
    assert(fz() == -8589934593)
    assert(fm(-4294967301, 1.25, True, "mixed", 8589934609, -2.5) == "mixed")
    assert(invoke(fi, 8589934593) == 4294967297)
    returned = choose()
    assert(returned(8589934593) == 4294967297)
    returned_float = choose_float(True)
    assert(returned_float(-3.25) == -6.5)
    returned_zero = choose_zero()
    assert(returned_zero() == -8589934593)
    address = to_int(ff)
    restored = address as fn(float) -> float
    assert(restored(1.125) == 2.25)
    print("PASS function-pointers")
    return 0
"#, None, "PASS function-pointers");
}

#[test]
fn illegal_extern_signatures_are_type_diagnostics_without_clang() {
    // Check only restrictions actually enforced by this revision. In particular,
    // do not claim that all aggregate externs are rejected: they currently aren't.
    let cases = [
        ("extern def main() -> int\n", "'main' cannot be declared extern"),
        ("extern def bad(x: void) -> int\n", "cannot have type void"),
        ("extern def bad(cb: fn(int) -> int) -> int\n", "cannot take a function-pointer parameter"),
        ("extern def bad(x: int = 3) -> int\n", "cannot have a default parameter value"),
        ("extern def bad[T](x: T) -> T\n", "extern functions cannot be generic"),
    ];
    for (decl, expected) in cases {
        let src = format!("{decl}\ndef main() -> int:\n    return 0\n");
        // Extern main must not be hidden by a duplicate normal main.
        let src = if decl.contains("def main(") { decl.to_string() } else { src };
        let diags = aoxn::compile_to_c(&src, false).expect_err(decl);
        assert!(!diags.is_empty(), "{decl}");
        assert!(diags.iter().all(|d| d.stage == "type"), "{decl}: {diags:?}");
        assert!(diags.iter().any(|d| d.message.contains(expected)), "{decl}: {diags:?}");
    }
}
