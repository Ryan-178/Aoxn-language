//! stdlib/random.ax: documented MINSTD vectors, bounds/rejection edges,
//! error-without-state-change, float ranges, and cross-process determinism.

use std::{fs, path::{Path, PathBuf}, process::Command};

const EXE: &str = if cfg!(windows) { ".exe" } else { "" };

const SRC: &str = r#"
import * from "stdlib/random.ax"

def main() -> int:
    r = random_seed(42)
    i = 0
    while i < 6:
        r = random_next(r)
        print(r.value)
        i = i + 1
    print("---")
    r = random_seed(42)
    r = random_below(r, 10)
    print(r.value)
    print(r.error)
    r = random_below(r, 1)
    print(r.value)
    print(r.error)
    r = random_below(r, 2147483646)
    print(r.value)
    print(r.error)
    r = random_range(r, 5, 5)
    print(r.value)
    print(r.error)
    r = random_range(r, 10, 9)
    print(r.error)
    before = r.state
    r = random_below(r, 0)
    print(r.error)
    print(r.state == before)
    r = random_below(r, 2147483647)
    print(r.error)
    print(r.state == before)
    print(random_seed(-5).state)
    print(random_seed(0).state)
    print(random_seed(2147483647).state)
    print(random_seed(2147483646).state)
    r = random_seed(42)
    r = random_float_next(r)
    print(r.fval > 0.0 and r.fval < 1.0)
    r = random_seed(42)
    r = random_float_range(r, 10.0, 20.0)
    print(r.fval >= 10.0 and r.fval < 20.0)
    r = random_seed(42)
    r = random_float_range(r, 1.0, 1.0)
    print(r.fval)
    print(r.error)
    print("DONE")
    return 0
"#;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn build(dir: &Path) -> PathBuf {
    let src = dir.join("random_driver.ax");
    fs::write(&src, SRC).unwrap();
    let exe = dir.join(format!("random_driver{EXE}"));
    aoxn::build_paths_opts(&[src.display().to_string()], &exe, true, &[], &[])
        .unwrap_or_else(|d| panic!("random driver failed: {d:?}"));
    exe
}

#[test]
fn random_vectors_bounds_and_errors() {
    let dir = root().join("target").join(format!("random-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let exe = build(&dir);
    let out = Command::new(&exe).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let expected = "\
705894
1126542223
1579310009
565444343
807934826
421520601
---
3
0
0
0
1579310008
0
5
0
1
1
true
1
true
2147483642
1
1
2147483646
true
true
1.000000
1
DONE
";
    assert_eq!(text, expected);

    // Cross-process determinism: a fresh process repeats the exact bytes.
    let again = Command::new(&exe).output().unwrap();
    assert_eq!(again.stdout, out.stdout);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn random_module_runs_through_the_selfhost_cli() {
    let Some(clang) = aoxn::find_clang() else {
        assert!(
            std::env::var_os("AOXN_TEST_REQUIRE_CLANG").is_none(),
            "clang required"
        );
        eprintln!("skipping: clang unavailable");
        return;
    };
    let dir = root().join("target").join(format!("random-selfhost-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = dir.join("random_driver.ax");
    fs::write(&src, SRC).unwrap();
    let cli = dir.join(format!("selfhost-cli{EXE}"));
    aoxn::build_paths_exe(
        &[root().join("selfhost/cli.ax").display().to_string()],
        &cli,
        true,
    )
    .expect("build the selfhost CLI");
    let out = Command::new(&cli)
        .current_dir(&dir)
        .env("AOXN_STDLIB", root().join("stdlib"))
        .env("AOXN_CACHE_DIR", dir.join("cache"))
        .env("AOXN_CLANG", &clang)
        .args(["run", "random_driver.ax"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "selfhost random failed: {} / {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("705894"), "{text}");
    assert!(text.contains("2147483642"), "{text}");
    assert!(text.ends_with("DONE\n"), "{text}");
    fs::remove_dir_all(&dir).unwrap();
}
