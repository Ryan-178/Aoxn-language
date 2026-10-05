//! stdlib/math.ax tests (v0.44.0) — the stdlib-todo §0.6 NaN/Inf toolkit
//! plus the CRT externs: fdiv guards, NaN predicates, trig/exp/pow
//! identities, degrees/radians, lerp/remap.

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
import * from "stdlib/math.ax"

def feq(a: float, b: float) -> bool:
    d = a - b
    if d < 0.0:
        d = -d
    return d < 0.0000001

def main() -> int:
    fails = 0
    if feq(sin(pi() / 2.0), 1.0):
        print("PASS sin-pi/2")
    else:
        print("FAIL sin-pi/2")
        fails = fails + 1
    if feq(pow(2.0, 10.0), 1024.0) and feq(sqrt(16.0), 4.0):
        print("PASS pow-sqrt")
    else:
        print("FAIL pow-sqrt")
        fails = fails + 1
    if feq(exp(1.0), e()) and feq(log(e()), 1.0):
        print("PASS exp-log")
    else:
        print("FAIL exp-log")
        fails = fails + 1
    if feq(log2(8.0), 3.0) and feq(log10(1000.0), 3.0):
        print("PASS log2-log10")
    else:
        print("FAIL log2-log10")
        fails = fails + 1
    if feq(fmod(7.0, 3.0), 1.0) and feq(trunc(3.7), 3.0) and feq(trunc(-3.7), -3.0):
        print("PASS fmod-trunc")
    else:
        print("FAIL fmod-trunc")
        fails = fails + 1
    if feq(degrees(pi()), 180.0) and feq(radians(90.0), pi() / 2.0):
        print("PASS deg-rad")
    else:
        print("FAIL deg-rad")
        fails = fails + 1
    if feq(atan2(1.0, 1.0), pi() / 4.0) and feq(hypot(3.0, 4.0), 5.0):
        print("PASS atan2-hypot")
    else:
        print("FAIL atan2-hypot")
        fails = fails + 1
    if is_inf(fdiv(1.0, 0.0)) and is_inf(fdiv(-1.0, 0.0)) and is_nan(fdiv(0.0, 0.0)) and feq(fdiv(1.0, 2.0), 0.5):
        print("PASS fdiv-guards")
    else:
        print("FAIL fdiv-guards")
        fails = fails + 1
    if is_nan(NaN()) and not is_inf(2.5) and is_fin(2.5) and not is_fin(NaN()):
        print("PASS nan-inf-predicates")
    else:
        print("FAIL nan-inf-predicates")
        fails = fails + 1
    if feq(copysign_f(3.0, -1.0), -3.0) and feq(copysign_f(-3.0, 2.0), 3.0):
        print("PASS copysign")
    else:
        print("FAIL copysign")
        fails = fails + 1
    if feq(lerp(10.0, 20.0, 0.25), 12.5) and feq(remap(5.0, 0.0, 10.0, 100.0, 200.0), 150.0):
        print("PASS lerp-remap")
    else:
        print("FAIL lerp-remap")
        fails = fails + 1
    print("DONE fails=" + str(fails))
    return fails

"#;

#[test]
fn math_module_floats() {
    if aoxn::find_clang().is_none() {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    }
    // the drivers import stdlib modules BY NAME, which exercises the
    // AOXN_STDLIB resolution path (env -> <root>/lib/stdlib -> checkout)
    std::env::set_var("AOXN_STDLIB", abs("stdlib"));
    let dir = std::env::temp_dir().join(format!("aoxn-math_module_floats-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src_path = dir.join("math_module_floats.ax");
    let exe = dir.join(format!("math_module_floats{EXE}"));
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
        "sin-pi/2",
        "pow-sqrt",
        "exp-log",
        "log2-log10",
        "fmod-trunc",
        "deg-rad",
        "atan2-hypot",
        "fdiv-guards",
        "nan-inf-predicates",
        "copysign",
        "lerp-remap",
    ] {
        assert!(
            text.contains(&format!("PASS {marker}")),
            "missing PASS {marker}
{text}"
        );
    }
}
