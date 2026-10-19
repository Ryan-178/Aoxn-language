//! Workspace (monorepo) support.
//!
//! A workspace is an `aoxn.json` that declares `"workspace": {"members":
//! ["packages/*"]}`. One lockfile lives at the workspace root; members
//! depend on each other via path dependencies. `discover` finds both the
//! nearest manifest (the "current package") and the enclosing workspace.

use std::path::{Path, PathBuf};

use crate::errors::PkgError;
use crate::manifest::Manifest;

#[derive(Debug, Clone)]
pub struct Workspace {
    /// directory holding the root manifest + shared `aoxn.lock`
    pub root: PathBuf,
    /// member package directories (each with its own `aoxn.json`)
    pub members: Vec<PathBuf>,
}

impl Workspace {
    /// Expand the member patterns of `root_manifest` in `root_dir`.
    pub fn load(root_dir: &Path, root_manifest: &Manifest) -> Result<Workspace, PkgError> {
        let Some(decl) = &root_manifest.workspace else {
            return Err(PkgError::Config(format!(
                "{} does not declare a workspace",
                root_dir.display()
            )));
        };
        let mut members = Vec::new();
        for pattern in &decl.members {
            let matched = expand_pattern(root_dir, pattern)?;
            if matched.is_empty() {
                return Err(PkgError::Config(format!(
                    "workspace member pattern `{pattern}` matched nothing"
                )));
            }
            for dir in matched {
                if !members.contains(&dir) {
                    members.push(dir);
                }
            }
        }
        Ok(Workspace {
            root: root_dir.to_path_buf(),
            members,
        })
    }

    /// Every member manifest, loaded.
    pub fn member_manifests(&self) -> Result<Vec<(PathBuf, Manifest)>, PkgError> {
        let mut out = Vec::new();
        for dir in &self.members {
            let path = Manifest::path_for(dir);
            let m = Manifest::load(&path).map_err(|e| {
                PkgError::Config(format!("workspace member {}: {e}", dir.display()))
            })?;
            out.push((dir.clone(), m));
        }
        Ok(out)
    }

    /// All package dirs that participate in the shared lockfile: the root
    /// (if it is itself a package) plus every member.
    pub fn all_packages(&self, root_manifest: Option<Manifest>) -> Result<Vec<(PathBuf, Manifest)>, PkgError> {
        let mut out = Vec::new();
        if let Some(m) = root_manifest {
            out.push((self.root.clone(), m));
        }
        out.extend(self.member_manifests()?);
        Ok(out)
    }
}

/// Minimal glob: `*` matches within one path segment (so `packages/*` does
/// not cross directories). Anything else is a literal path.
fn expand_pattern(root: &Path, pattern: &str) -> Result<Vec<PathBuf>, PkgError> {
    let norm = pattern.replace('\\', "/");
    if !norm.contains('*') {
        let dir = root.join(&norm);
        if Manifest::exists_in(&dir) {
            return Ok(vec![dir]);
        }
        return Err(PkgError::Config(format!(
            "workspace member `{pattern}` has no aoxn.json"
        )));
    }
    let (head, rest) = match norm.split_once('/') {
        Some((h, r)) => (Some(h.to_string()), r.to_string()),
        None => (None, norm.clone()),
    };
    let scan_dir = match head {
        Some(h) if !h.contains('*') => root.join(h),
        _ => root.to_path_buf(),
    };
    let (prefix, suffix) = match rest.split_once('*') {
        Some((p, s)) => (p.to_string(), s.to_string()),
        None => (rest.clone(), String::new()),
    };
    let mut out = Vec::new();
    let entries = std::fs::read_dir(&scan_dir)
        .map_err(|e| PkgError::Config(format!("cannot list {}: {}", scan_dir.display(), crate::errors::io_reason_for(&e, &scan_dir, crate::errors::IoTarget::Dir))))?;
    let mut names: Vec<_> = entries.flatten().collect();
    names.sort_by_key(|e| e.file_name());
    for e in names {
        if !e.path().is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with(&prefix) && name.ends_with(&suffix) && name.len() >= prefix.len() + suffix.len()
        {
            let dir = e.path();
            if Manifest::exists_in(&dir) {
                out.push(dir);
            }
        }
    }
    Ok(out)
}

/// Find the nearest `aoxn.json` walking up from `start`, then continue up to
/// find an enclosing workspace. Returns `(project_dir, workspace_opt)`.
pub fn discover(start: &Path) -> Result<(PathBuf, Option<Workspace>), PkgError> {
    let start = if start.is_absolute() {
        start.to_path_buf()
    } else {
        std::env::current_dir()?.join(start)
    };
    let mut project = None;
    let mut cur: Option<&Path> = Some(start.as_path());
    let mut workspace = None;
    while let Some(dir) = cur {
        if Manifest::exists_in(dir) {
            if project.is_none() {
                project = Some(dir.to_path_buf());
            }
            let m = Manifest::load(&Manifest::path_for(dir))?;
            if m.workspace.is_some() {
                let ws = Workspace::load(dir, &m)?;
                // does the workspace actually contain the current project?
                let contains = project
                    .as_ref()
                    .map(|p| *p == ws.root || ws.members.iter().any(|mem| mem == p))
                    .unwrap_or(false);
                if contains {
                    workspace = Some(ws);
                    break;
                }
            }
        }
        cur = dir.parent();
    }
    let Some(project) = project else {
        return Err(PkgError::Config(format!(
            "no aoxn.json found in {} or any parent (run `aoxn init` first)",
            start.display()
        )));
    };
    Ok((project, workspace))
}

/// Where the shared lockfile lives: workspace root when inside a workspace,
/// otherwise next to the current manifest.
pub fn lockfile_path(project: &Path, ws: Option<&Workspace>) -> PathBuf {
    match ws {
        Some(ws) => ws.root.join("aoxn.lock"),
        None => project.join("aoxn.lock"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aoxn-ws-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write_manifest(dir: &Path, json: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(Manifest::path_for(dir), json).unwrap();
    }

    #[test]
    fn member_expansion_and_discovery() {
        let root = temp_root("expand");
        write_manifest(
            &root,
            r#"{"name":"root","version":"0.1.0","workspace":{"members":["packages/*"]}}"#,
        );
        write_manifest(&root.join("packages").join("a"), r#"{"name":"a","version":"0.1.0"}"#);
        write_manifest(&root.join("packages").join("b"), r#"{"name":"b","version":"0.1.0"}"#);
        std::fs::create_dir_all(root.join("packages").join("empty")).unwrap(); // no manifest

        let ws = Workspace::load(&root, &Manifest::load(&Manifest::path_for(&root)).unwrap()).unwrap();
        assert_eq!(ws.members.len(), 2);

        // discovery from inside a member finds the member + the workspace
        let (project, found) = discover(&root.join("packages").join("a")).unwrap();
        assert_eq!(project, root.join("packages").join("a"));
        let ws2 = found.expect("workspace should be discovered");
        assert_eq!(ws2.root, root);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn standalone_project_has_no_workspace() {
        let root = temp_root("standalone");
        write_manifest(&root, r#"{"name":"solo","version":"0.1.0"}"#);
        let (project, found) = discover(&root).unwrap();
        assert_eq!(project, root);
        assert!(found.is_none());
        let _ = std::fs::remove_dir_all(&root);
    }
}
