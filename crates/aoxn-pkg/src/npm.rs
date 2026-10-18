//! npm bridge: a one-shot import of Aoxn packages hosted on any
//! npm-compatible registry (npm itself, Verdaccio, GitHub Packages, …).
//!
//! Design (decided 2026-10-02, "一次性导入工具" over "registry proxy"):
//! - the **npm CLI is the transport** (`npm view` for metadata, `npm pack`
//!   for the tarball). This keeps the crate TLS-free — npm handles auth and
//!   https, and respects the user's `.npmrc` (private registries just
//!   work). A registry-proxy backend would need an HTTPS client in this
//!   crate, which the zero-build-script dependency constraint forbids.
//! - only **Aoxn packages** can be imported: the tarball must contain an
//!   `aoxn.json` in its package root. Plain JavaScript packages are
//!   rejected with an explanation — Aoxn cannot link JavaScript.
//! - the imported package lands in `vendor/<name>/` and is recorded in
//!   `aoxn.json` as a **path dependency** (`{"path": "vendor/<name>"}`).
//!   This reuses the whole existing pipeline (materialization shims,
//!   manifest-hash integrity via `install`) with zero new concepts;
//!   `vendor/` is excluded from packing like `aox_modules/`.
//! - the npm version + integrity provenance is echoed in the output; the
//!   vendored tree itself is the source of truth afterwards.

use std::path::{Path, PathBuf};

use crate::context::DepScope;
use crate::errors::PkgError;
use crate::install::DEFAULT_JOBS;
use crate::manifest::{DependencySpec, Manifest};
use crate::tarball;
use crate::ui::current;

/// npm tarballs are rooted at `package/`.
const NPM_TARBALL_ROOT: &str = "package";

/// Where imported npm packages are vendored, relative to the project.
const VENDOR_DIR: &str = "vendor";

pub fn import(specs: &[String], from: Option<&str>, dry_run: bool) -> Result<i32, PkgError> {
    if specs.is_empty() && from.is_none() {
        return Err(PkgError::Manifest(
            "nothing to import: pass npm specs like `aoxn npm-import my-pkg@1.2` \
             (or --from package.json)"
                .into(),
        ));
    }
    let project = std::env::current_dir()?;
    if !Manifest::exists_in(&project) {
        return Err(PkgError::Manifest(format!(
            "no aoxn.json in {} — run `aoxn init` first; npm imports are \
             recorded as path dependencies of an existing project",
            project.display()
        )));
    }

    // ---- build the work list: (npm spec with an explicit version)
    let mut wanted: Vec<String> = Vec::new();
    for spec in specs {
        let (name, _version) = parse_spec(spec);
        crate::manifest::validate_name(&name).map_err(|_| {
            PkgError::Manifest(format!(
                "`{name}` does not look like an npm package name; npm specs are \
                 `name`, `name@version` or `@scope/name@version`"
            ))
        })?;
        wanted.push(spec.clone());
    }
    if let Some(file) = from {
        wanted.extend(package_json_specs(Path::new(file))?);
        if wanted.is_empty() {
            return Err(PkgError::Manifest(format!(
                "{file} declares no dependencies (nothing to import)"
            )));
        }
    }

    let ui = current();
    let mut imported: Vec<String> = Vec::new();
    for spec in &wanted {
        let (name, version) = parse_spec(spec);
        let doc = npm_view(spec)?;
        let version = match version {
            Some(v) => v,
            // a range (or bare name) may match several published versions —
            // npm returns an array then; pick the newest
            None => pick_version(&doc).ok_or_else(|| {
                PkgError::Registry(format!("npm has no version of `{name}` matching `{spec}`"))
            })?,
        };
        let integrity = doc
            .get("dist")
            .and_then(|d| d.get("integrity"))
            .and_then(|v| v.as_str())
            .map(str::to_string);

        ui.step(&format!("importing {name}@{version}"));
        let full_spec = format!("{name}@{version}");
        let tgz = npm_pack(&full_spec)?;
        verify_integrity(&tgz, integrity.as_deref(), &full_spec)?;

        let vendored = import_tarball(&project, &tgz, spec, dry_run)?;
        imported.push(format!("{name}@{version} -> {vendored}"));
    }

    if !dry_run {
        crate::manage::install_cmd(false, false, DepScope::All, DEFAULT_JOBS, false, false, false)?;
    }
    if !imported.is_empty() && !dry_run {
        ui.success(&format!(
            "imported {} (recorded as path dependencies; `import * from \"<name>\"` now works)",
            imported.join(", ")
        ));
    }
    Ok(0)
}

/// The post-download half: verify nothing unexpected, vendor the tree,
/// record the path dependency. Split out so tests can drive it without npm.
fn import_tarball(
    project: &Path,
    tgz: &Path,
    _spec: &str,
    dry_run: bool,
) -> Result<String, PkgError> {
    // unpack to a scratch dir first: the aoxn manifest decides the dep name
    let scratch = std::env::temp_dir().join(format!(
        "aoxn-npm-import-{}-{}",
        crate::cache::sha256_hex(project.to_string_lossy().as_bytes())[..8].to_string(),
        std::process::id()
    ));
    let pkg_dir = scratch.join("pkg");
    let _ = std::fs::remove_dir_all(&scratch);
    tarball::unpack(tgz, &pkg_dir, NPM_TARBALL_ROOT)?;

    let manifest_path = Manifest::path_for(&pkg_dir);
    if !manifest_path.exists() {
        let _ = std::fs::remove_dir_all(&scratch);
        return Err(PkgError::Manifest(
            "the npm package has no aoxn.json in its root — it is a plain \
             JavaScript package, which Aoxn cannot link. Only Aoxn packages \
             published to npm (with their aoxn.json included) can be imported"
                .into(),
        ));
    }
    let pkg_manifest = Manifest::load(&manifest_path)?;
    let name = pkg_manifest.name.clone();
    crate::manifest::validate_name(&name)?;

    let vendored_rel = format!("{VENDOR_DIR}/{name}");
    let vendored = project.join(&vendored_rel);
    if !dry_run {
        let _ = std::fs::remove_dir_all(&vendored); // re-import overwrites
        std::fs::create_dir_all(vendored.parent().unwrap())?;
        std::fs::rename(&pkg_dir, &vendored).or_else(|_| {
            // cross-device rename: fall back to copy
            copy_dir(&pkg_dir, &vendored)
        })?;
    }
    let _ = std::fs::remove_dir_all(&scratch);

    // record `<name>: {"path": "vendor/<name>"}` in the project manifest
    let manifest_path = Manifest::path_for(project);
    let mut manifest = Manifest::load(&manifest_path)?;
    manifest
        .dependencies
        .insert(name.clone(), DependencySpec::Path { path: PathBuf::from(vendored_rel.clone()) });
    if !dry_run {
        manifest.save(&manifest_path)?;
    }
    Ok(vendored_rel)
}

// ---------------------------------------------------------------------------
// npm CLI transport

fn npm_bin() -> String {
    std::env::var("AOXN_NPM").unwrap_or_else(|_| {
        if cfg!(windows) {
            "npm.cmd".to_string()
        } else {
            "npm".to_string()
        }
    })
}

fn run_npm(args: &[&str]) -> Result<String, PkgError> {
    let out = std::process::Command::new(npm_bin())
        .args(args)
        .output()
        .map_err(|e| {
            PkgError::Other(format!(
                "cannot run `{}` ({e}) — the npm bridge shells out to the npm \
                 CLI (`npm view` / `npm pack`) as its transport; install \
                 Node.js/npm or point AOXN_NPM at the binary",
                npm_bin()
            ))
        })?;
    if !out.status.success() {
        return Err(PkgError::Registry(format!(
            "npm {} failed: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// `npm view <spec> --json` → parsed doc. A version-range spec can match
/// several versions; npm then returns an array of docs — the caller picks.
fn npm_view(spec: &str) -> Result<serde_json::Value, PkgError> {
    let stdout = run_npm(&["view", spec, "--json"])?;
    let doc: serde_json::Value = serde_json::from_str(stdout.trim())
        .map_err(|e| PkgError::Registry(format!("npm view {spec}: unparseable output: {e}")))?;
    Ok(doc)
}

/// Newest version doc from an `npm view` result (object or array form).
fn pick_version(doc: &serde_json::Value) -> Option<String> {
    let docs: Vec<&serde_json::Value> = match doc {
        serde_json::Value::Array(a) => a.iter().collect(),
        d @ serde_json::Value::Object(_) => vec![d],
        _ => vec![],
    };
    let mut best: Option<(semver::Version, String)> = None;
    for d in docs {
        let v = d.get("version")?.as_str()?;
        let sv: semver::Version = v.parse().ok()?;
        if best.as_ref().map(|(b, _)| sv > *b).unwrap_or(true) {
            best = Some((sv, v.to_string()));
        }
    }
    best.map(|(_, v)| v)
}

/// `npm pack <spec> --pack-destination <fresh tmp>` → path of the tgz.
fn npm_pack(spec: &str) -> Result<PathBuf, PkgError> {
    let dir = std::env::temp_dir().join(format!("aoxn-npm-pack-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let out = run_npm(&["pack", spec, "--pack-destination", dir.to_string_lossy().as_ref()])?;
    let _ = out;
    let mut tgzs: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "tgz").unwrap_or(false))
        .collect();
    if tgzs.len() != 1 {
        return Err(PkgError::Registry(format!(
            "npm pack {spec}: expected exactly one tarball in {}, got {}",
            dir.display(),
            tgzs.len()
        )));
    }
    Ok(tgzs.pop().unwrap())
}

// ---------------------------------------------------------------------------
// helpers

/// Split an npm spec into (name, explicit version). Scoped names
/// (`@scope/pkg@1.2`) keep their leading `@` intact.
fn parse_spec(spec: &str) -> (String, Option<String>) {
    let searchable = spec.strip_prefix('@').map(|s| s).unwrap_or(spec);
    let offset = spec.len() - searchable.len();
    match searchable.rfind('@') {
        Some(i) => (
            spec[..offset + i].to_string(),
            Some(spec[offset + i + 1..].to_string()),
        ),
        None => (spec.to_string(), None),
    }
}

/// Verify a downloaded tarball against npm's `dist.integrity`
/// (`sha512-<base64>`); absent integrity info is tolerated (older
/// registries) — the manifest-hash check at install still covers the tree.
fn verify_integrity(tgz: &Path, integrity: Option<&str>, spec: &str) -> Result<(), PkgError> {
    let Some(integrity) = integrity else {
        return Ok(());
    };
    let Some(b64) = integrity.strip_prefix("sha512-") else {
        return Err(PkgError::Registry(format!(
            "{spec}: unsupported npm integrity format `{integrity}` (expected sha512)"
        )));
    };
    let expected = base64_decode_sha(b64)
        .ok_or_else(|| PkgError::Registry(format!("{spec}: malformed integrity digest")))?;
    let data = std::fs::read(tgz)?;
    use sha2::{Digest, Sha512};
    let mut h = Sha512::new();
    h.update(&data);
    let actual: [u8; 64] = h.finalize().into();
    if actual[..] != expected[..] {
        return Err(PkgError::Registry(format!(
            "npm tarball for {spec} does not match its dist.integrity — \
             the download is corrupt or was tampered with"
        )));
    }
    Ok(())
}

/// Minimal standard base64 decoder (padding required) for integrity digests.
fn base64_decode_sha(s: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a') as u32 + 26),
            b'0'..=b'9' => Some((c - b'0') as u32 + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for chunk in s.as_bytes().chunks(4) {
        let mut acc = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            acc |= val(c)? << (18 - 6 * i);
        }
        let n = chunk.len();
        out.push((acc >> 16) as u8);
        if n > 2 {
            out.push((acc >> 8) as u8);
        }
        if n > 3 {
            out.push(acc as u8);
        }
    }
    Some(out)
}

/// Specs from an npm `package.json` (`dependencies` + `devDependencies`),
/// as `name@range` entries the normal import path can resolve.
fn package_json_specs(file: &Path) -> Result<Vec<String>, PkgError> {
    let text = std::fs::read_to_string(file)
        .map_err(|e| PkgError::Manifest(format!("cannot read {}: {}", file.display(), crate::errors::io_reason(&e))))?;
    let doc: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| PkgError::Manifest(format!("{} is not valid JSON: {e}", file.display())))?;
    let mut out = Vec::new();
    for section in ["dependencies", "devDependencies"] {
        if let Some(map) = doc.get(section).and_then(|v| v.as_object()) {
            for (name, range) in map {
                let range = range.as_str().unwrap_or("*");
                // ranges with spaces (`^1 >=2`) or semver operators that are
                // not npm specs would confuse `npm view` — pass through as-is
                out.push(format!("{name}@{range}"));
            }
        }
    }
    Ok(out)
}

fn copy_dir(src: &Path, dst: &Path) -> Result<(), PkgError> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aoxn-npm-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(p: &Path, content: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    #[test]
    fn specs_split_at_the_last_at_sign() {
        assert_eq!(parse_spec("left-pad"), ("left-pad".into(), None));
        assert_eq!(
            parse_spec("left-pad@1.3.0"),
            ("left-pad".into(), Some("1.3.0".into()))
        );
        assert_eq!(
            parse_spec("@scope/pkg@^1.2"),
            ("@scope/pkg".into(), Some("^1.2".into()))
        );
        assert_eq!(parse_spec("@scope/pkg"), ("@scope/pkg".into(), None));
    }

    #[test]
    fn base64_sha512_vector() {
        // sha512 of empty string, base64: known value
        assert_eq!(
            base64_decode_sha("z4PhNX7vuL3xVChQ1m2AB9Yg5AULVxXcg/SpIdNs6c5H0NE8XYXysP+DGNKHfuwvY7kxvUdBeoGlODJ6+SfaPg==").unwrap().len(),
            64
        );
        assert_eq!(base64_decode_sha("aGVsbG8="), Some(b"hello".to_vec()));
        assert_eq!(base64_decode_sha("!!!" ), None);
    }

    fn make_npm_tarball(dir: &Path, with_manifest: bool) -> PathBuf {
        let pkg = dir.join("pkg-src");
        write(
            &pkg.join("aoxn.json"),
            r#"{ "name": "imported-pkg", "version": "2.0.0", "main": "lib.ax" }"#,
        );
        write(&pkg.join("lib.ax"), "pub fn hi() -> int { 1 }");
        if !with_manifest {
            std::fs::remove_file(pkg.join("aoxn.json")).unwrap();
        }
        // npm tarballs are rooted at `package/`
        let tgz = dir.join("imported-pkg-2.0.0.tgz");
        tarball::pack(&pkg, NPM_TARBALL_ROOT, &tgz).unwrap();
        tgz
    }

    fn init_project(dir: &Path) {
        write(
            &dir.join("aoxn.json"),
            r#"{ "name": "app", "version": "0.1.0" }"#,
        );
    }

    #[test]
    fn import_vendors_and_records_path_dependency() {
        let tmp = temp_root("ok");
        let tgz = make_npm_tarball(&tmp, true);
        let project = tmp.join("project");
        init_project(&project);

        let vendored = import_tarball(&project, &tgz, "imported-pkg@2.0.0", false).unwrap();
        assert_eq!(vendored, "vendor/imported-pkg");
        assert!(project.join("vendor/imported-pkg/aoxn.json").exists());
        assert!(project.join("vendor/imported-pkg/lib.ax").exists());

        let m = Manifest::load(&Manifest::path_for(&project)).unwrap();
        assert_eq!(
            m.dependencies.get("imported-pkg"),
            Some(&DependencySpec::Path { path: PathBuf::from("vendor/imported-pkg") })
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn reimport_overwrites_the_vendored_tree() {
        let tmp = temp_root("re");
        let tgz = make_npm_tarball(&tmp, true);
        let project = tmp.join("project");
        init_project(&project);

        import_tarball(&project, &tgz, "imported-pkg@2.0.0", false).unwrap();
        // change the tarball content and re-import
        let pkg = tmp.join("pkg-src");
        write(&pkg.join("lib.ax"), "pub fn hi() -> int { 2 }");
        tarball::pack(&pkg, NPM_TARBALL_ROOT, &tgz).unwrap();
        import_tarball(&project, &tgz, "imported-pkg@2.0.0", false).unwrap();
        assert!(project
            .join("vendor/imported-pkg/lib.ax")
            .exists());
        let content =
            std::fs::read_to_string(project.join("vendor/imported-pkg/lib.ax")).unwrap();
        assert!(content.contains("{ 2 }"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn plain_js_package_is_rejected() {
        let tmp = temp_root("js");
        let tgz = make_npm_tarball(&tmp, false);
        let project = tmp.join("project");
        init_project(&project);
        let err = import_tarball(&project, &tgz, "left-pad@1.3.0", false).unwrap_err();
        assert!(format!("{err}").contains("no aoxn.json"), "got: {err}");
        assert!(!project.join("vendor").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn dry_run_touches_nothing() {
        let tmp = temp_root("dry");
        let tgz = make_npm_tarball(&tmp, true);
        let project = tmp.join("project");
        init_project(&project);
        import_tarball(&project, &tgz, "imported-pkg@2.0.0", true).unwrap();
        assert!(!project.join("vendor").exists());
        let m = Manifest::load(&Manifest::path_for(&project)).unwrap();
        assert!(m.dependencies.is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn integrity_mismatch_is_detected() {
        let tmp = temp_root("bad");
        let tgz = make_npm_tarball(&tmp, true);
        use sha2::{Digest, Sha512};
        let mut h = Sha512::new();
        h.update(b"other bytes");
        let wrong = base64_encode(&h.finalize());
        let err = verify_integrity(&tgz, Some(&format!("sha512-{wrong}")), "x@1").unwrap_err();
        assert!(format!("{err}").contains("dist.integrity"), "got: {err}");
        assert!(verify_integrity(&tgz, None, "x@1").is_ok());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// tiny base64 encoder for the test above
    fn base64_encode(data: &[u8]) -> String {
        const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in data.chunks(3) {
            let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
            let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
            out.push(T[(n >> 18) as usize & 63] as char);
            out.push(T[(n >> 12) as usize & 63] as char);
            if chunk.len() > 1 {
                out.push(T[(n >> 6) as usize & 63] as char);
            } else {
                out.push('=');
            }
            if chunk.len() > 2 {
                out.push(T[n as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
        out
    }

    #[test]
    fn package_json_deps_are_enumerated() {
        let tmp = temp_root("pkgjson");
        write(
            &tmp.join("package.json"),
            r#"{ "dependencies": { "a": "^1.0.0", "@s/b": "~2" }, "devDependencies": { "c": "*" } }"#,
        );
        let specs = package_json_specs(&tmp.join("package.json")).unwrap();
        // serde_json's map iterates in sorted-key order
        assert_eq!(specs, vec!["@s/b@~2", "a@^1.0.0", "c@*"]);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
