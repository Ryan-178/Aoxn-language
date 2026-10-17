//! `aoxn.json` — the package manifest.
//!
//! ```json
//! {
//!   "name": "my-app",
//!   "version": "0.1.0",
//!   "main": "src/main.ax",
//!   "types": "src/types.ax",
//!   "exports": {
//!     ".": "src/lib.ax",
//!     "./client": "src/client.ax"
//!   },
//!   "description": "optional",
//!   "dependencies": {
//!     "http": "^1.2",
//!     "json": { "version": "^1", "registry": "main" },
//!     "my-utils": { "path": "../my-utils" }
//!   },
//!   "devDependencies": { "test-harness": "^2" },
//!   "overrides": { "http": "1.4.2" },
//!   "workspace": { "members": ["packages/*"] }
//! }
//! ```
//!
//! `exports` (since v0.29.1) gates package entry points: when present, the
//! compiler resolves `import * from "pkg"` and `import * from "pkg/sub"`
//! against its keys (`"."` is the root, `"./sub"` a subpath) instead of
//! probing `index.ax`. `types` declares the TypeScript type-entry file
//! (consumed by a later TS-frontend pass; resolved-but-not-enforced today).
//!
//! `devDependencies` (v0.31.0) holds tooling that must not ship in a
//! production install. Dev and prod are resolved into **one** lockfile; the
//! distinction lives in `aoxn.lock` as a per-package `dev` flag, and
//! `aoxn install --prod` simply materializes the prod closure.
//! `overrides` (v0.31.0) force a requirement on a package wherever it
//! appears in the graph — the fix for an upstream release you cannot wait
//! for, pip's constraints-file / pnpm's `overrides` in one line.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::errors::PkgError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main: Option<String>,
    /// TypeScript type-entry file (e.g. `"src/types.ax"`). Resolved by the
    /// compiler's manifest reader but not yet enforced by the TS checker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub types: Option<String>,
    /// Subpath entry map (npm-style): `{"." : "src/lib.ax", "./client":
    /// "src/client.ax"}`. When present, gates all package imports.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub exports: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<String, DependencySpec>,
    /// tooling-only dependencies; resolved with `dependencies` into one
    /// lockfile but excluded from `aoxn install --prod` (v0.31.0)
    #[serde(
        default,
        rename = "devDependencies",
        alias = "dev_dependencies",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub dev_dependencies: BTreeMap<String, DependencySpec>,
    /// forced requirements, applied wherever the package appears in the
    /// graph (`{"http": "1.4.2"}`) — v0.31.0
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub overrides: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<WorkspaceDecl>,
    /// named registries; the key `default` is used when a dependency has no
    /// explicit `registry` hint. Values are git URLs (kind inferred),
    /// directory paths with `{"kind": "dir"}`, or `http://` URLs (kind
    /// inferred; read-only) with an optional explicit `{"kind": "http"}`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub registries: BTreeMap<String, RegistryDecl>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryDecl {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>, // "git" | "dir" | "http"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceDecl {
    /// glob patterns relative to this manifest's directory (`packages/*`)
    pub members: Vec<String>,
}

/// One entry in `dependencies`. Either a plain semver requirement string
/// (registry dependency) or an object with a `path` (local / workspace
/// member) and optional `version` + `registry` override.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencySpec {
    Registry {
        req: String,
        registry: Option<String>,
    },
    Path {
        path: PathBuf,
    },
}

impl Serialize for DependencySpec {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            DependencySpec::Registry { req, registry: None } => s.serialize_str(req),
            DependencySpec::Registry { req, registry: Some(r) } => {
                use serde::ser::SerializeMap;
                let mut map = s.serialize_map(Some(2))?;
                map.serialize_entry("version", req)?;
                map.serialize_entry("registry", r)?;
                map.end()
            }
            DependencySpec::Path { path } => {
                use serde::ser::SerializeMap;
                let mut map = s.serialize_map(Some(1))?;
                map.serialize_entry("path", &path.to_string_lossy())?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for DependencySpec {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = DependencySpec;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("version requirement string or {\"path\": ...} / {\"version\": ..., \"registry\": ...}")
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(DependencySpec::Registry {
                    req: v.to_string(),
                    registry: None,
                })
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut m: A) -> Result<Self::Value, A::Error> {
                let mut path = None;
                let mut version = None;
                let mut registry = None;
                while let Some(k) = m.next_key::<String>()? {
                    match k.as_str() {
                        "path" => path = Some(m.next_value::<String>()?),
                        "version" => version = Some(m.next_value::<String>()?),
                        "registry" => registry = Some(m.next_value::<String>()?),
                        other => return Err(serde::de::Error::unknown_field(other, &["path", "version", "registry"])),
                    }
                }
                if let Some(p) = path {
                    return Ok(DependencySpec::Path { path: PathBuf::from(p) });
                }
                match version {
                    Some(v) => Ok(DependencySpec::Registry { req: v, registry }),
                    None => Err(serde::de::Error::missing_field("version")),
                }
            }
        }
        d.deserialize_any(V)
    }
}

/// Package name rule: lowercase letters, digits, `-`, `_`; 1-64 chars;
/// must not start/end with `-` (keeps `name@version` keys unambiguous).
pub fn validate_name(name: &str) -> Result<(), PkgError> {
    let bad = |why: &str| Err(PkgError::Manifest(format!("invalid package name `{name}`: {why}")));
    if name.is_empty() || name.len() > 64 {
        return bad("must be 1-64 characters");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        return bad("only lowercase a-z, 0-9, '-' and '_' allowed");
    }
    if name.starts_with('-') || name.ends_with('-') {
        return bad("must not start or end with '-'");
    }
    Ok(())
}

impl Manifest {
    pub fn load(path: &Path) -> Result<Manifest, PkgError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| PkgError::Manifest(format!("cannot read {}: {e}", path.display())))?;
        let m: Manifest = serde_json::from_str(&text).map_err(|e| {
            PkgError::Manifest(format!("invalid {}: {e}", path.display()))
        })?;
        m.validate()?;
        Ok(m)
    }

    pub fn validate(&self) -> Result<(), PkgError> {
        validate_name(&self.name)?;
        if self.version.parse::<semver::Version>().is_err() {
            return Err(PkgError::Manifest(format!(
                "invalid version `{}` (semver expected, e.g. 1.2.3)",
                self.version
            )));
        }
        for (dep, spec) in self.dependencies.iter().chain(&self.dev_dependencies) {
            // The KEY becomes a directory: `aox_modules/<name>`, and that
            // directory is removed RECURSIVELY before extraction and before
            // any integrity check. A key like `../victim` therefore deleted
            // `app/victim` (reproduced). `validate_name` is the same strict
            // allowlist that governs the project's own name — lowercase
            // a-z, 0-9, `-`, `_` — which cannot spell a separator, a drive
            // letter or a dot segment, so no key can escape.
            validate_name(dep)?;
            match spec {
                DependencySpec::Registry { req, registry: _ } => {
                    if req.parse::<semver::VersionReq>().is_err() {
                        return Err(PkgError::Manifest(format!(
                            "invalid requirement for dependency `{dep}`: `{req}`"
                        )));
                    }
                }
                DependencySpec::Path { path } => {
                    if path.as_os_str().is_empty() {
                        return Err(PkgError::Manifest(format!(
                            "dependency `{dep}`: empty path"
                        )));
                    }
                }
            }
        }
        for (pkg, req) in &self.overrides {
            if req.parse::<semver::VersionReq>().is_err() {
                return Err(PkgError::Manifest(format!(
                    "invalid override for `{pkg}`: `{req}`"
                )));
            }
            validate_name(pkg)?;
        }
        Ok(())
    }

    /// A valid but empty manifest, for contexts that need a `Ctx` without a
    /// project on disk (`aoxn trust bootstrap`).
    pub fn synthetic() -> Manifest {
        Manifest {
            name: "aoxn-pkg-probe".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            main: None,
            types: None,
            exports: BTreeMap::new(),
            description: None,
            dependencies: BTreeMap::new(),
            dev_dependencies: BTreeMap::new(),
            overrides: BTreeMap::new(),
            workspace: None,
            registries: BTreeMap::new(),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), PkgError> {
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(path, text + "\n")?;
        Ok(())
    }

    /// Path to the package's `aoxn.json`.
    pub fn path_for(dir: &Path) -> PathBuf {
        dir.join("aoxn.json")
    }

    pub fn exists_in(dir: &Path) -> bool {
        Manifest::path_for(dir).exists()
    }

    /// The declared entry file for the package root (subpath `"."`).
    /// Resolution order mirrors the compiler's manifest reader: `exports["."]`
    /// first, then `main`, then the conventional probe order (`index.ax` then
    /// `main.ax`, also `.ts`/`.tsx`). Returns a path relative to the package
    /// root, or None when nothing exists on disk.
    pub fn entry_file(&self, pkg_dir: &Path) -> Option<PathBuf> {
        if let Some(target) = self.exports.get(".") {
            let p = PathBuf::from(target);
            return if pkg_dir.join(&p).exists() { Some(p) } else { None };
        }
        if let Some(main) = &self.main {
            let p = PathBuf::from(main);
            return if pkg_dir.join(&p).exists() { Some(p) } else { None };
        }
        for cand in ["index.ax", "index.ts", "index.tsx", "main.ax", "main.ts", "main.tsx"] {
            if pkg_dir.join(cand).exists() {
                return Some(PathBuf::from(cand));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_and_object_deps() {
        let m: Manifest = serde_json::from_str(
            r#"{
                "name": "app",
                "version": "0.1.0",
                "dependencies": {
                    "http": "^1.2",
                    "json": {"version": "^1", "registry": "mirror"},
                    "local-lib": {"path": "../local-lib"}
                }
            }"#,
        )
        .unwrap();
        assert_eq!(
            m.dependencies.get("http"),
            Some(&DependencySpec::Registry { req: "^1.2".into(), registry: None })
        );
        assert_eq!(
            m.dependencies.get("local-lib"),
            Some(&DependencySpec::Path { path: PathBuf::from("../local-lib") })
        );
    }

    #[test]
    fn rejects_bad_names() {
        assert!(validate_name("MyPkg").is_err());
        assert!(validate_name("-x").is_err());
        assert!(validate_name("a b").is_err());
        assert!(validate_name("ok_name-2").is_ok());
    }

    #[test]
    fn roundtrip_is_stable() {
        let m: Manifest = serde_json::from_str(
            r#"{"name":"a","version":"1.0.0","dependencies":{"b":"^0.1"}}"#,
        )
        .unwrap();
        let text = serde_json::to_string_pretty(&m).unwrap();
        let m2: Manifest = serde_json::from_str(&text).unwrap();
        assert_eq!(serde_json::to_string_pretty(&m2).unwrap(), text);
    }

    #[test]
    fn parses_exports_and_types() {
        let m: Manifest = serde_json::from_str(
            r#"{
                "name": "lib",
                "version": "1.0.0",
                "types": "src/types.ax",
                "exports": {
                    ".": "src/lib.ax",
                    "./client": "src/client.ax"
                }
            }"#,
        )
        .unwrap();
        assert_eq!(m.types.as_deref(), Some("src/types.ax"));
        assert_eq!(m.exports.get("."), Some(&"src/lib.ax".to_string()));
        assert_eq!(m.exports.get("./client"), Some(&"src/client.ax".to_string()));
    }

    #[test]
    fn exports_roundtrip_omits_empty() {
        let m: Manifest = serde_json::from_str(
            r#"{"name":"a","version":"1.0.0","exports":{".":"./lib.ax"}}"#,
        )
        .unwrap();
        let text = serde_json::to_string_pretty(&m).unwrap();
        assert!(text.contains("exports"));
        assert!(!text.contains("types"));
        let m2: Manifest = serde_json::from_str(&text).unwrap();
        assert_eq!(m2.exports.get("."), Some(&"./lib.ax".to_string()));
    }

    #[test]
    fn deny_unknown_fields_still_holds() {
        let res: Result<Manifest, _> = serde_json::from_str(
            r#"{"name":"a","version":"1.0.0","bogus":true}"#,
        );
        assert!(res.is_err(), "deny_unknown_fields must reject unknown keys");
    }

    #[test]
    fn parses_dev_dependencies_and_overrides() {
        let m: Manifest = serde_json::from_str(
            r#"{
                "name": "app",
                "version": "0.1.0",
                "dependencies": { "http": "^1" },
                "devDependencies": { "harness": { "path": "../harness" } },
                "overrides": { "zlib": "1.4.2" }
            }"#,
        )
        .unwrap();
        assert_eq!(m.dependencies.len(), 1);
        assert_eq!(m.dev_dependencies.len(), 1);
        assert!(matches!(
            m.dev_dependencies.get("harness"),
            Some(DependencySpec::Path { .. })
        ));
        assert_eq!(m.overrides.get("zlib").map(String::as_str), Some("1.4.2"));
        m.validate().unwrap();
    }

    #[test]
    fn dev_dependencies_accept_the_snake_case_spelling() {
        let m: Manifest =
            serde_json::from_str(r#"{"name":"a","version":"1.0.0","dev_dependencies":{"x":"^1"}}"#)
                .unwrap();
        assert_eq!(m.dev_dependencies.get("x").is_some(), true);
    }

    #[test]
    fn overrides_roundtrip_omit_when_empty() {
        let m: Manifest =
            serde_json::from_str(r#"{"name":"a","version":"1.0.0","dependencies":{"b":"^1"}}"#)
                .unwrap();
        let text = serde_json::to_string_pretty(&m).unwrap();
        assert!(!text.contains("devDependencies"));
        assert!(!text.contains("overrides"), "empty tables must not be written");
        // and the snake_case alias never leaks into the written form
        let with_dev = Manifest {
            dev_dependencies: BTreeMap::from([("x".to_string(), DependencySpec::Registry {
                req: "^1".into(),
                registry: None,
            })]),
            ..Manifest::synthetic()
        };
        let text = serde_json::to_string_pretty(&with_dev).unwrap();
        assert!(text.contains("devDependencies"), "{text}");
        assert!(!text.contains("dev_dependencies"), "{text}");
    }

    #[test]
    fn invalid_override_is_rejected_at_load() {
        let m: Manifest =
            serde_json::from_str(r#"{"name":"a","version":"1.0.0","overrides":{"z":"~~1"}}"#)
                .unwrap();
        let err = m.validate().unwrap_err();
        assert!(format!("{err}").contains("override"), "{err}");
    }

    #[test]
    fn dev_dependency_requirements_are_validated_too() {
        let m: Manifest = serde_json::from_str(
            r#"{"name":"a","version":"1.0.0","devDependencies":{"x":"not-a-range"}}"#,
        )
        .unwrap();
        let err = m.validate().unwrap_err();
        assert!(format!("{err}").contains("invalid requirement"), "{err}");
    }

    /// A dependency KEY becomes `aox_modules/<key>`, and that directory is
    /// removed RECURSIVELY before extraction and before any integrity check —
    /// so the key `"../victim"` deleted `app/victim` outright (reproduced).
    /// The project's own name has been validated by a strict allowlist since
    /// long before this; the keys were not, and they are the ones that reach
    /// a filesystem.
    #[test]
    fn dependency_keys_are_validated_as_names() {
        for bad in ["../victim", "..", "a/b", "a\\b", "C:name", "./x", "..\\..\\y"] {
            let json_bad = bad.replace(0x5Cu8 as char, "\\\\");
            let m: Manifest = serde_json::from_str(&format!(
                r#"{{"name":"app","version":"1.0.0","dependencies":{{"{json_bad}":"^1.0.0"}}}}"#
            ))
            .unwrap();
            let err = m.validate().unwrap_err();
            assert!(
                format!("{err}").contains("invalid package name"),
                "dependency key `{bad}` must be refused as a name: {err}"
            );
        }
        // devDependencies reach the same directory, so they get the same rule
        let d: Manifest = serde_json::from_str(
            r#"{"name":"app","version":"1.0.0","devDependencies":{"../victim":"^1.0.0"}}"#,
        )
        .unwrap();
        assert!(format!("{:?}", d.validate().unwrap_err()).contains("invalid package name"));
        // and a legitimate key still passes
        let ok: Manifest = serde_json::from_str(
            r#"{"name":"app","version":"1.0.0","dependencies":{"my-lib_2":"^1.0.0"}}"#,
        )
        .unwrap();
        ok.validate().unwrap();
    }
}
