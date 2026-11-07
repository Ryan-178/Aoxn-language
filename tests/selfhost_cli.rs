//! Differential tests for the Rust-built and Aoxn-built selfhost CLI.
//!
//! The selfhost CLI is not a forwarder: every command below is served by
//! selfhost/cli.ax + driver.ax. `run` uses a real argv vector (no shell) and
//! propagates the child exit code; options reach clang through argv too.
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn exe_suffix() -> &'static str {
    if cfg!(windows) {
        ".exe"
    } else {
        ""
    }
}

struct Ctx {
    root: PathBuf,
    dir: PathBuf,
    path: std::ffi::OsString,
    clang: PathBuf,
}

impl Ctx {
    fn invoke(&self, exe: &Path, args: &[&str]) -> Output {
        Command::new(exe)
            .current_dir(&self.dir)
            .env("PATH", &self.path)
            .env("AOXN_STDLIB", self.root.join("stdlib"))
            .env("AOXN_CACHE_DIR", self.dir.join("cache"))
            .env("AOXN_CLANG", &self.clang)
            .args(args)
            .output()
            .unwrap()
    }

    fn invoke_no_clang(&self, exe: &Path, args: &[&str]) -> Output {
        Command::new(exe)
            .current_dir(&self.dir)
            .env("PATH", &self.path)
            .env("AOXN_STDLIB", self.root.join("stdlib"))
            .env("AOXN_CACHE_DIR", self.dir.join("cache"))
            .env_remove("AOXN_CLANG")
            .args(args)
            .output()
            .unwrap()
    }
}

fn status(out: &Output, expected: i32, what: &str) {
    assert_eq!(
        out.status.code(),
        Some(expected),
        "{what}\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn cache_entries(dir: &Path) -> usize {
    fs::read_dir(dir)
        .map(|it| it.filter_map(|e| e.ok()).count())
        .unwrap_or(0)
}

#[test]
fn selfhost_cli_matches_contract_and_rust_reference() {
    let Some(clang) = aoxn::find_clang() else {
        assert!(
            std::env::var_os("AOXN_TEST_REQUIRE_CLANG").is_none(),
            "clang required"
        );
        eprintln!("skipping: clang unavailable");
        return;
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir = root
        .join("target")
        .join(format!("selfhost-cli-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("directory with spaces")).unwrap();
    let mut paths = vec![clang.parent().unwrap().to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let ctx = Ctx {
        root: root.clone(),
        dir: dir.clone(),
        path: std::env::join_paths(paths).unwrap(),
        clang: clang.clone(),
    };

    let stage1 = dir.join(format!("rust-cli{}", exe_suffix()));
    let cli_src = root.join("selfhost/cli.ax");
    aoxn::build_paths_exe(&[cli_src.display().to_string()], &stage1, true)
        .expect("Rust-build the selfhost CLI");

    // stage2 is produced by stage1 itself.
    let stage2_name = format!("selfhost-cli{}", exe_suffix());
    let out = ctx.invoke(
        &stage1,
        &["build", &cli_src.display().to_string(), "-o", &stage2_name],
    );
    status(&out, 0, "stage2 build");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        stage2_name,
        "build prints the output path"
    );
    let stage2 = dir.join(&stage2_name);
    assert!(stage2.exists(), "selfhost build must produce a second CLI");

    let source = "def main() -> int:\n    print(\"literal %s %n\")\n    return 0\n";
    fs::write(dir.join("directory with spaces/input.ax"), source).unwrap();
    fs::write(
        dir.join("broken.ax"),
        "def main() -> int:\n    return missing\n",
    )
    .unwrap();
    fs::write(dir.join("parse.ax"), "def main(\n").unwrap();
    fs::write(dir.join("lex.ax"), "@\n").unwrap();
    fs::write(
        dir.join("import.ax"),
        "import * from \"./absent.ax\"\ndef main() -> int:\n    return 0\n",
    )
    .unwrap();
    fs::write(
        dir.join("warn.ax"),
        "def main() -> int:\n    x = 1\n    print(\"ok\")\n    return 0\n",
    )
    .unwrap();
    fs::write(
        dir.join("args.ax"),
        "def main() -> int:\n    print(argc())\n    i = 0\n    while i < argc():\n        print(\"[\" + arg(i) + \"]\")\n        i = i + 1\n    exit(3)\n    return 0\n",
    )
    .unwrap();
    fs::write(
        dir.join("dep.ax"),
        "def helper() -> int:\n    return 1\n",
    )
    .unwrap();
    fs::write(
        dir.join("uses_dep.ax"),
        "import * from \"./dep.ax\"\ndef main():\n    print(helper())\n",
    )
    .unwrap();
    fs::write(
        dir.join("bad_builtin.ax"),
        "def main() -> int:\n    return arg(True)\n",
    )
    .unwrap();
    fs::write(
        dir.join("codegen_gap.ax"),
        "def main() -> int:\n    print(\"a\", 1)\n    return 0\n",
    )
    .unwrap();
    let expected_c = aoxn::compile_paths_to_c(
        &[dir
            .join("directory with spaces/input.ax")
            .display()
            .to_string()],
        true,
    )
    .unwrap();

    let rust_cli = PathBuf::from(env!("CARGO_BIN_EXE_aoxn"));

    for exe in [&stage1, &stage2] {
        let name = exe.file_name().unwrap().to_string_lossy().into_owned();
        // --- help/version ---
        for args in [vec!["help"], vec!["--help"], vec!["-h"]] {
            let out = ctx.invoke(exe, &args);
            status(&out, 0, &name);
            assert!(String::from_utf8_lossy(&out.stdout).contains("Usage:"));
        }
        let out = ctx.invoke(exe, &["version"]);
        status(&out, 0, &name);
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "aoxn 0.55.0");
        let rust = Command::new(&rust_cli).arg("version").output().unwrap();
        assert_eq!(out.stdout, rust.stdout);

        // --- usage errors ---
        for args in [
            vec![],
            vec!["run"],
            vec!["build"],
            vec!["c", "--json"],
            vec!["help", "x"],
            vec!["check", "broken.ax", "extra.ax"],
            vec!["build", "broken.ax", "-o"],
            vec!["c", "broken.ax", "-o", "x"],
            vec!["build", "broken.ax", "-o", "x", "-o", "y"],
            vec!["build", "broken.ax", "--O0", "--O1"],
            vec!["build", "broken.ax", "--backend", "llvm"],
            vec!["build", "broken.ax", "--tailwind"],
            vec!["build", "broken.ax", "--emit-assets", "x"],
            vec!["nope"],
        ] {
            let out = ctx.invoke(exe, &args);
            status(&out, 2, &format!("{name} {args:?}"));
            if !args.is_empty() {
                assert!(
                    String::from_utf8_lossy(&out.stderr).contains("error:"),
                    "{name} {args:?}"
                );
            }
        }

        // --- check/c ---
        let out = ctx.invoke(exe, &["check", "directory with spaces/input.ax"]);
        status(&out, 0, &name);
        assert!(out.stdout.is_empty());
        let out = ctx.invoke(
            exe,
            &["check", "directory with spaces/input.ax", "--json"],
        );
        status(&out, 0, &name);
        assert!(out.stdout.is_empty(), "JSON report belongs on stderr");
        assert_eq!(
            String::from_utf8_lossy(&out.stderr).trim(),
            "{\"ok\":true,\"errors\":[],\"warnings\":[]}"
        );
        let out = ctx.invoke(exe, &["check", "warn.ax", "--json"]);
        status(&out, 0, &name);
        assert!(out.stdout.is_empty(), "JSON report belongs on stderr");
        let text = String::from_utf8_lossy(&out.stderr);
        assert!(text.contains("\"code\":\"W001\""), "{text}");
        assert!(text.contains("unused variable 'x'"), "{text}");
        assert!(text.contains("\"line\":2,\"col\":5"), "{text}");
        let out = ctx.invoke(exe, &["check", "warn.ax"]);
        status(&out, 0, &name);
        assert!(String::from_utf8_lossy(&out.stderr).contains("unused variable 'x'"));
        let out = ctx.invoke(exe, &["check", "broken.ax", "--json"]);
        status(&out, 1, &name);
        assert!(out.stdout.is_empty(), "error JSON belongs on stderr");
        let text = String::from_utf8_lossy(&out.stderr);
        assert!(text.contains("\"ok\":false"), "{text}");
        assert!(text.contains("\"stage\":\"type\""), "{text}");
        assert!(text.contains("unknown variable 'missing'"), "{text}");
        let rust = Command::new(&rust_cli)
            .current_dir(&dir)
            .args(["check", "broken.ax", "--json"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), rust.status.code());
        assert!(
            String::from_utf8_lossy(&rust.stderr).contains("\"stage\":\"type\""),
            "rust: {} / {}",
            String::from_utf8_lossy(&rust.stdout),
            String::from_utf8_lossy(&rust.stderr)
        );

        let out = ctx.invoke(exe, &["c", "directory with spaces/input.ax"]);
        status(&out, 0, &name);
        assert_eq!(
            out.stdout,
            expected_c.as_bytes(),
            "c must emit exact text without banners or extra newline"
        );
        let rust = Command::new(&rust_cli)
            .current_dir(&dir)
            .args(["c", "directory with spaces/input.ax"])
            .output()
            .unwrap();
        assert_eq!(out.stdout, rust.stdout);

        // --- failures ---
        for file in [
            "missing.ax",
            "broken.ax",
            "parse.ax",
            "lex.ax",
            "import.ax",
            "bad_builtin.ax",
            "codegen_gap.ax",
        ] {
            for command in ["check", "c", "build"] {
                let out = ctx.invoke(exe, &[command, file]);
                status(&out, 1, &format!("{name} {command} {file}"));
                assert!(
                    String::from_utf8_lossy(&out.stderr).contains("error:")
                        || String::from_utf8_lossy(&out.stderr).contains("["),
                    "{name} {command} {file}: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
            }
        }
        let out = ctx.invoke_no_clang(exe, &["check", "directory with spaces/input.ax"]);
        status(&out, 0, &name);
        assert!(out.stdout.is_empty(), "check needs no clang");

        // --- source-overwrite guard ---
        for output in [
            "./directory with spaces/input.ax",
            "directory with spaces/../directory with spaces/input.ax",
        ] {
            let out = ctx.invoke(
                exe,
                &["build", "directory with spaces/input.ax", "-o", output],
            );
            status(&out, 1, &name);
            assert_eq!(
                fs::read_to_string(dir.join("directory with spaces/input.ax")).unwrap(),
                source
            );
        }

        // --- output names: argv spawn means no shell injection ---
        if cfg!(windows) {
            for output in ["bad&echo PWNED", "bad%PATH%", "bad!name"] {
                let out = ctx.invoke(
                    exe,
                    &["build", "directory with spaces/input.ax", "-o", output],
                );
                status(&out, 0, &format!("{name} output {output}"));
                assert!(dir.join(output).is_file());
                assert!(!dir.join("PWNED").exists(), "shell injection happened");
            }
            let out = ctx.invoke(
                exe,
                &["build", "directory with spaces/input.ax", "-o", "bad\"name"],
            );
            status(&out, 1, &format!("{name} invalid windows filename"));
        } else {
            let output = "apostrophe'output";
            let out = ctx.invoke(
                exe,
                &["build", "directory with spaces/input.ax", "-o", output],
            );
            status(&out, 0, &name);
            assert!(dir.join(output).is_file());
        }
        let named = format!("directory with spaces/output app{}", exe_suffix());
        let out = ctx.invoke(
            exe,
            &[
                "build",
                "directory with spaces/input.ax",
                "-o",
                &named,
            ],
        );
        status(&out, 0, &name);
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            named
        );
        let run = Command::new(dir.join(&named)).output().unwrap();
        status(&run, 0, "built program");
        assert_eq!(run.stdout, b"literal %s %n\n");

        let default = dir.join(format!("directory with spaces/input{}", exe_suffix()));
        let _ = fs::remove_file(&default);
        let out = ctx.invoke(exe, &["build", "directory with spaces/input.ax"]);
        status(&out, 0, &name);
        assert!(default.is_file());
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            format!("directory with spaces/input{}", exe_suffix())
        );
        let dash = format!("-output{}", exe_suffix());
        let out = ctx.invoke(
            exe,
            &["build", "directory with spaces/input.ax", "-o", &dash],
        );
        status(&out, 0, &name);
        assert!(dir.join(&dash).is_file());

        // --- options reach the compiler ---
        let opts_exe = format!("opts{}", exe_suffix());
        for opt in ["--O0", "--O1", "--O2", "--O3", "-O0"] {
            let out = ctx.invoke(
                exe,
                &["build", "directory with spaces/input.ax", "-o", &opts_exe, opt],
            );
            status(&out, 0, &format!("{name} {opt}"));
        }
        let out = ctx.invoke(
            exe,
            &[
                "build",
                "directory with spaces/input.ax",
                "-o",
                &opts_exe,
                "--backend",
                "c",
                "--cc-warnings",
                "--clang-arg",
                "-g",
            ],
        );
        status(&out, 0, &format!("{name} clang flags"));
        let out = ctx.invoke(
            exe,
            &[
                "build",
                "directory with spaces/input.ax",
                "-o",
                &opts_exe,
                "--clang-arg",
                "-f;echo PWNED",
            ],
        );
        status(&out, 1, &format!("{name} unsafe clang arg"));
        assert!(!String::from_utf8_lossy(&out.stderr).contains("PWNED\n"));

        // --- run: exact argv, empty strings, special characters, exit code ---
        let run_out = ctx.invoke(
            exe,
            &["run", "args.ax", "--", "hello world", "", "a&b", "%PATH%"],
        );
        status(&run_out, 3, &format!("{name} run"));
        assert_eq!(
            String::from_utf8_lossy(&run_out.stdout),
            "4\n[hello world]\n[]\n[a&b]\n[%PATH%]\n"
        );
        // cached run must behave identically and stay cached
        let cache_dir = dir.join("cache");
        let first_count = cache_entries(&cache_dir);
        let cached = ctx.invoke(
            exe,
            &["run", "args.ax", "--", "hello world", "", "a&b", "%PATH%"],
        );
        status(&cached, 3, &format!("{name} cached run"));
        assert_eq!(cached.stdout, run_out.stdout);
        assert_eq!(cache_entries(&cache_dir), first_count, "cache hit rebuilt");

        // one dependency change invalidates the entry
        fs::write(dir.join("dep.ax"), "def helper() -> int:\n    return 7\n").unwrap();
        let out = ctx.invoke(exe, &["run", "uses_dep.ax"]);
        status(&out, 0, &name);
        assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n");
        // a codegen-affecting option is a different key
        let before = cache_entries(&cache_dir);
        let out = ctx.invoke(exe, &["run", "args.ax", "--O0", "--", "x"]);
        status(&out, 3, &name);
        assert!(cache_entries(&cache_dir) > before, "option change reused cache");

        // --- symbols ---
        fs::write(
            dir.join("syms.ax"),
            "struct P:\n    x: int\n\nextern def sqrt(x: float) -> float\n\ndef twice(x: int) -> int:\n    return x * 2\n\ndef main() -> int:\n    return twice(2)\n",
        )
        .unwrap();
        let out = ctx.invoke(exe, &["symbols", "syms.ax"]);
        status(&out, 0, &name);
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("struct    P"), "{text}");
        assert!(text.contains("function  twice"), "{text}");
        assert!(text.contains("syms.ax:4:1"), "{text}");
        assert!(text.contains("syms.ax:6:1"), "{text}");
        let out = ctx.invoke(exe, &["symbols", "syms.ax", "--json"]);
        status(&out, 0, &name);
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.starts_with("{\"symbols\":["), "{text}");
        assert!(text.contains("\"kind\":\"struct\""), "{text}");
        assert!(text.contains("\"signature\":\"def twice(x: int) -> int\""), "{text}");
        let rust = Command::new(&rust_cli)
            .current_dir(&dir)
            .args(["symbols", "syms.ax", "--json"])
            .output()
            .unwrap();
        let rust_text = String::from_utf8_lossy(&rust.stdout);
        for needle in [
            "\"name\":\"P\"",
            "\"name\":\"sqrt\"",
            "\"name\":\"twice\"",
            "\"name\":\"main\"",
            "\"signature\":\"def twice(x: int) -> int\"",
        ] {
            assert!(rust_text.contains(needle), "rust symbols: {rust_text}");
            assert!(text.contains(needle), "{name} symbols: {text}");
        }

        // --- doctor / version ---
        let out = ctx.invoke(exe, &["doctor", "--json", "--no-smoke"]);
        status(&out, 0, &name);
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("\"name\":\"clang\""), "{text}");
        assert!(text.contains("\"ok\":true"), "{text}");
        assert!(text.contains("\"smoke\":null"), "{text}");
        let out = ctx.invoke(exe, &["doctor", "--no-smoke"]);
        status(&out, 0, &name);
        assert!(String::from_utf8_lossy(&out.stdout).contains("Aoxn 0.55.0 doctor"));
    }

    // The socket stdlib module must be buildable by the selfhost CLI with the
    // same link contract as the Rust compiler.
    let sock_exe = format!("sock{}", exe_suffix());
    let sock_src = root.join("examples/socket_echo.ax").display().to_string();
    let out = if cfg!(windows) {
        ctx.invoke(
            &stage2,
            &["build", &sock_src, "-o", &sock_exe, "-l", "ws2_32"],
        )
    } else {
        ctx.invoke(&stage2, &["build", &sock_src, "-o", &sock_exe])
    };
    status(&out, 0, "selfhost socket build");
    assert!(dir.join(&sock_exe).is_file());

    let _ = fs::remove_dir_all(&dir);
}
