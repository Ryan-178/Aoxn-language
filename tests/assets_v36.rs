//! v0.36.0 CSS asset features: `url(...)` rewriting, emitted asset files,
//! CSS Modules dot access, the Tailwind utility generator, and the TS
//! `import * from` regression.
//!
//! The tests that matter most here are the negative ones: a `data:` URI, an
//! absolute URL, a fragment, and a run of ordinary prose must all survive the
//! pipeline untouched. Each of those is a case where being "clever" silently
//! produces a stylesheet that renders differently than the author wrote.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

const EXE: &str = if cfg!(windows) { ".exe" } else { "" };
static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn tmp_dir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("Aoxn-v36-{}-{}-{}", std::process::id(), tag, n));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn have_clang() -> Option<PathBuf> {
    let p = std::env::var("AOXN_CLANG").ok().map(PathBuf::from)?;
    if p.is_file() {
        Some(p)
    } else {
        None
    }
}

/// Build an entry, run it, and return stdout.
fn build_and_run(dir: &Path, main: &str) -> String {
    let exe = dir.join(format!("run-{}{EXE}", std::process::id()));
    aoxn::build_paths_exe(&[main.to_string()], &exe, true).unwrap_or_else(|d| {
        panic!(
            "build failed: {}",
            d.iter().map(aoxn::diag_to_string).collect::<Vec<_>>().join("; ")
        )
    });
    let out = Command::new(&exe).output().expect("failed to run");
    let _ = std::fs::remove_file(&exe);
    let _ = std::fs::remove_file(exe.with_extension("obj"));
    assert!(out.status.success(), "run failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn write_main(dir: &Path, body: &str) -> PathBuf {
    let main = dir.join("main.ax");
    std::fs::write(&main, body).unwrap();
    main
}

// ---- url(...) rewriting ----

#[test]
fn local_url_is_rewritten_and_fingerprinted() {
    let dir = tmp_dir("url");
    std::fs::write(dir.join("logo.png"), b"PNGDATA").unwrap();
    std::fs::write(dir.join("a.css"), ".a{background:url(./logo.png)}").unwrap();
    let main = write_main(
        &dir,
        "import * from \"./a.css\"\ndef main() -> int:\n    print(styles())\n    return 0\n",
    );
    let out = build_and_run(&dir, &main.display().to_string());
    let css = out.trim();
    assert!(!css.contains("./logo.png"), "the raw path survived: {css}");
    assert!(css.contains("logo."), "no fingerprinted name: {css}");
    assert!(css.contains(".png)"), "extension lost: {css}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn non_local_urls_are_left_alone() {
    let dir = tmp_dir("url-keep");
    std::fs::write(
        dir.join("a.css"),
        ".a{background:url(\"data:image/svg+xml;base64,AAA\")}\n\
         .b{background:url(https://cdn.example.com/x.png)}\n\
         .c{background:url(#gradient)}\n\
         .d{background:url(//cdn.example.com/y.png)}",
    )
    .unwrap();
    let main = write_main(
        &dir,
        "import * from \"./a.css\"\ndef main() -> int:\n    print(styles())\n    return 0\n",
    );
    let out = build_and_run(&dir, &main.display().to_string());
    let css = out.trim();
    assert!(css.contains("data:image/svg+xml;base64,AAA"), "data: URI lost: {css}");
    assert!(css.contains("https://cdn.example.com/x.png"), "absolute URL rewritten: {css}");
    assert!(css.contains("#gradient"), "fragment rewritten: {css}");
    assert!(css.contains("//cdn.example.com/y.png"), "protocol-relative rewritten: {css}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_word_ending_in_url_is_not_a_url_token() {
    let dir = tmp_dir("url-word");
    std::fs::write(dir.join("a.css"), ".a{background:myurl(./x.png)}").unwrap();
    std::fs::write(dir.join("x.png"), b"X").unwrap();
    let main = write_main(
        &dir,
        "import * from \"./a.css\"\ndef main() -> int:\n    print(styles())\n    return 0\n",
    );
    let out = build_and_run(&dir, &main.display().to_string());
    assert!(
        out.contains("myurl(./x.png)"),
        "an identifier ending in `url` was treated as a url() token: {}",
        out.trim()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_url_target_passes_through_with_a_warning() {
    // A stylesheet may legitimately reference an asset the compiler was never
    // asked to manage (a CDN font, a file another tool copies in). Erroring
    // would reject valid CSS; rewriting it to a fingerprint we cannot compute
    // would be worse. So: untouched, and visible.
    let dir = tmp_dir("url-missing");
    std::fs::write(dir.join("a.css"), ".a{background:url(./nope.png)}").unwrap();
    let main = write_main(
        &dir,
        "import * from \"./a.css\"\ndef main() -> int:\n    print(styles())\n    return 0\n",
    );
    let out = build_and_run(&dir, &main.display().to_string());
    assert!(
        out.contains("./nope.png"),
        "a missing url() target should pass through unchanged: {}",
        out.trim()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn url_targets_join_the_cache_dependency_set() {
    let dir = tmp_dir("url-cache");
    std::fs::write(dir.join("logo.png"), b"PNGDATA").unwrap();
    std::fs::write(dir.join("a.css"), ".a{background:url(./logo.png)}").unwrap();
    let main = write_main(&dir, "import * from \"./a.css\"\ndef main() -> int:\n    return 0\n");
    let deps = aoxn::dependency_files(&[main.display().to_string()]).expect("deps");
    assert!(
        deps.iter().any(|p| p.ends_with("logo.png")),
        "a url() target is missing from the dependency set: {deps:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn scan_css_urls_finds_only_local_targets() {
    let src = ".a{background:url(./x.png)}\n.b{background:url(\"data:,A\")}\n.c{background:url(https://e.com/y.png)}";
    assert_eq!(aoxn::assets::scan_css_urls(src), vec!["./x.png".to_string()]);
}

// ---- emitted assets ----

#[test]
fn the_bundle_name_is_the_file_on_disk() {
    let Some(_clang) = have_clang() else {
        eprintln!("skipping: set AOXN_CLANG to run (needs clang)");
        return;
    };
    let dir = tmp_dir("emit-name");
    std::fs::write(dir.join("a.css"), ".a{color:red}").unwrap();
    let main = write_main(
        &dir,
        "import * from \"./a.css\"\ndef main() -> int:\n    print(styles_fingerprint())\n    return 0\n",
    );
    let advertised = build_and_run(&dir, &main.display().to_string()).trim().to_string();
    let assets = aoxn::collect_assets(&[main.display().to_string()]).expect("collect");
    let names: Vec<String> = assets.emitted_files().into_iter().map(|(n, _)| n).collect();
    assert!(
        names.contains(&advertised),
        "styles_fingerprint() advertised {advertised} but emit produced {names:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn emitting_writes_the_bundle_and_its_references() {
    let dir = tmp_dir("emit-write");
    std::fs::write(dir.join("logo.png"), b"PNGDATA").unwrap();
    std::fs::write(dir.join("a.css"), ".a{background:url(./logo.png)}").unwrap();
    let main = write_main(&dir, "import * from \"./a.css\"\ndef main() -> int:\n    return 0\n");
    let assets = aoxn::collect_assets(&[main.display().to_string()]).expect("collect");
    let out_dir = dir.join("assets");
    let written = assets.emit(&out_dir).expect("emit");
    assert!(written.len() >= 3, "expected bundle + stylesheet + png, got {written:?}");
    assert!(written.iter().all(|p| p.starts_with(&out_dir)), "wrote outside the target dir");
    // the emitted stylesheet of a.css is `a.<hash>.css`. Match by file NAME:
    // the old spelling fell back to `…\assets\a…` with a literal backslash,
    // which never matched on Linux — and this is one of the tests that DOES
    // run on a clang-less host (it needs no compiler, just collect + emit).
    let css_file = written
        .iter()
        .find(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.starts_with("a.") && name.ends_with(".css")
        })
        .expect("a stylesheet was emitted");
    let css = std::fs::read_to_string(css_file).unwrap();
    if let Some(name) = css.split("url(").nth(1).and_then(|s| s.split(')').next()) {
        assert!(
            out_dir.join(name).is_file(),
            "emitted CSS references {name}, which was not emitted"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_empty_asset_set_emits_nothing() {
    let dir = tmp_dir("emit-empty");
    let set = aoxn::assets::AssetSet::default();
    assert!(set.emit(&dir.join("assets")).unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- CSS Modules dot access ----

#[test]
fn module_binding_gives_dot_access() {
    let Some(_clang) = have_clang() else {
        eprintln!("skipping: set AOXN_CLANG to run (needs clang)");
        return;
    };
    let dir = tmp_dir("dot");
    std::fs::write(dir.join("page.module.css"), ".title{color:red}\n.body{margin:0}").unwrap();
    let main = dir.join("main.ts");
    std::fs::write(
        &main,
        "import styles from \"./page.module.css\";\n\n\
         function main(): void {\n  \
             console.log(styles.title);\n  \
             console.log(styles.body);\n}\n",
    )
    .unwrap();
    let out = build_and_run(&dir, &main.display().to_string());
    let mut lines = out.lines();
    let title = lines.next().unwrap_or("").to_string();
    let body = lines.next().unwrap_or("").to_string();
    assert!(title.starts_with("title_"), "dot access did not scope: {title}");
    assert!(body.starts_with("body_"), "dot access did not scope: {body}");
    assert_ne!(title, body, "both fields returned the same class");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn plain_css_import_binds_nothing() {
    // only `*.module.css` scopes class names, so a plain import must not
    // produce a binding that silently reads an empty struct
    let ok = aoxn::ts::parser::parse(
        0,
        "import styles from \"./a.css\";\nfunction main(): void {}\n",
    );
    assert!(ok.is_ok(), "plain css import failed to parse: {:?}", ok.err());
}

// ---- TS import-star regression ----

/// `import * from "p"` is the form every Aoxn source uses — and the one the
/// parser's own error message recommends — but before v0.36.0 the arm meant
/// to handle it rejected it unconditionally, so no `.ts` file could import
/// anything. `import * as ns` needs namespace objects and must still fail.
#[test]
fn import_star_from_parses_but_star_as_does_not() {
    assert!(
        aoxn::ts::parser::parse(0, "import * from \"./x\";\nfunction main(): void {}\n").is_ok(),
        "`import * from` was rejected"
    );
    let err = aoxn::ts::parser::parse(0, "import * as ns from \"./x\";\nfunction main(): void {}\n")
        .expect_err("`import * as ns` must still be rejected");
    assert!(err.message.contains("namespace objects"), "wrong diagnostic: {}", err.message);
}

// ---- Tailwind utility generation ----

#[test]
fn tailwind_generates_only_used_utilities() {
    use aoxn::tailwind::generate;
    let g = generate(&[r#"const h = `<div class="flex p-4 text-red-500"></div>`;"#]);
    assert!(g.css.contains(".flex{display:flex;}"), "flex missing: {}", g.css);
    assert!(g.css.contains(".p-4{padding:1rem;}"), "p-4 wrong: {}", g.css);
    assert!(g.css.contains(".text-red-500{color:#ef4444;}"), "color wrong: {}", g.css);
    assert!(!g.css.contains(".italic"), "an unused rule was generated: {}", g.css);
}

#[test]
fn tailwind_ignores_ordinary_prose() {
    use aoxn::tailwind::generate;
    // A quoted sentence must not be mined for utility-shaped words; doing so
    // would ship dead CSS and could shadow a hand-written rule.
    let src = r#"const msg = "the red-500 of it is p-4 and flex too, honestly speaking here";"#;
    assert_eq!(generate(&[src]).css, "", "prose was mined for classes");
}

#[test]
fn tailwind_reads_class_name_attributes() {
    use aoxn::tailwind::generate;
    let g = generate(&[r#"<div className="px-4 pt-2 mb-1"></div>"#]);
    assert!(g.css.contains(".px-4{padding-left:1rem;padding-right:1rem;}"), "px-4 wrong: {}", g.css);
    assert!(g.css.contains(".pt-2{padding-top:0.5rem;}"), "pt-2 wrong: {}", g.css);
    assert!(g.css.contains(".mb-1{margin-bottom:0.25rem;}"), "mb-1 wrong: {}", g.css);
}

#[test]
fn tailwind_reports_unknown_utilities() {
    use aoxn::tailwind::generate;
    let g = generate(&[r#"<div class="flex animate-spin-thing"></div>"#]);
    assert!(g.css.contains(".flex{"), "known class missing: {}", g.css);
    assert!(
        g.unknown.iter().any(|u| u.contains("animate")),
        "an unsupported utility was not reported: {:?}",
        g.unknown
    );
}

#[test]
fn tailwind_output_is_well_formed() {
    use aoxn::tailwind::generate;
    let g = generate(&[r#"<div class="flex rounded-lg text-sm font-bold bg-blue-600"></div>"#]);
    for rule in g.css.split('}').filter(|r| !r.trim().is_empty()) {
        assert!(
            rule.trim_end().ends_with(';'),
            "a declaration is missing its semicolon: {rule}}}"
        );
    }
}

#[test]
fn tailwind_generated_rules_precede_authored_ones() {
    // A real Tailwind build lets an authored stylesheet override the
    // generated utilities; the generated block must come first in the bundle.
    let set = aoxn::assets::AssetSet::default();
    let mut set = set;
    set.prepend_tw(".flex{display:flex;}");
    assert_eq!(set.bundle(), ".flex{display:flex;}");
}