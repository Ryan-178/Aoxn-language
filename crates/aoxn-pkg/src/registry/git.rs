//! Git-repository registry backend.
//!
//! The registry is a git repo laid out as:
//! ```text
//! packages/<name>/index.json
//! packages/<name>/<version>.tar.gz
//! trust.json                     # optional: curated trust index (v0.31.0)
//! advisories/*.json              # optional: advisory DB for `aoxn audit`
//! ```
//!
//! - install: one shallow fetch of the whole registry into the global cache
//!   (`registries/<sha8(url)>/`); offline installs read the last snapshot.
//! - publish / yank: full clone to a temp dir, edit `index.json`, commit,
//!   tag `pkg/<name>/v<version>` (publish only), push. Publishing is
//!   idempotent: an identical re-push (same checksum) reports success.
//!
//! A registry that also carries `trust.json` and `advisories/` is a
//! *curated* registry — see `docs/trusted-registry.md`. The git backend is
//! the one that makes all three roles cheap, which is why `aoxn trust
//! bootstrap` wires all of them to the same URL.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::{read_json_or_default, write_json, PackageIndex, PublishOutcome, PublishRequest, Registry};
use crate::cache::Cache;
use crate::errors::PkgError;

pub struct GitRegistry {
    url: String,
    /// local snapshot directory (shallow clone in the global cache)
    snapshot: PathBuf,
}

fn run_git(args: &[&str], cwd: Option<&Path>) -> Result<String, PkgError> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    cmd.arg("-c").arg("core.autocrlf=false");
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let out = cmd.output().map_err(|e| {
        PkgError::Registry(format!(
            "cannot run git ({e}); git must be on PATH to talk to git registries"
        ))
    })?;
    if !out.status.success() {
        return Err(PkgError::Registry(format!(
            "git {} failed: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

impl GitRegistry {
    pub fn new(url: &str, cache: &Cache) -> GitRegistry {
        GitRegistry {
            url: url.to_string(),
            snapshot: cache.registry_dir(url),
        }
    }

    /// Make sure a fresh-enough snapshot exists. In offline mode an existing
    /// snapshot is used as-is; without one it is a hard error.
    fn ensure_snapshot(&self, offline: bool) -> Result<(), PkgError> {
        let have_snapshot = self.snapshot.join(".git").exists();
        if !have_snapshot {
            if offline {
                return Err(PkgError::Offline(format!(
                    "registry `{}` was never fetched and no cached snapshot exists",
                    self.url
                )));
            }
            if let Some(parent) = self.snapshot.parent() {
                std::fs::create_dir_all(parent)?;
            }
            // clone into a temp dir then swap, so a failed clone never leaves
            // a half-populated snapshot that offline mode would trust
            let tmp = self.snapshot.with_extension("tmp");
            let _ = std::fs::remove_dir_all(&tmp);
            let out = Command::new("git")
                .args(["clone", "--depth", "1", &self.url])
                .arg(&tmp)
                .output()
                .map_err(|e| PkgError::Registry(format!("cannot run git: {e}")))?;
            if !out.status.success() {
                let _ = std::fs::remove_dir_all(&tmp);
                return Err(PkgError::Registry(format!(
                    "cloning registry `{}` failed: {}",
                    self.url,
                    String::from_utf8_lossy(&out.stderr).trim()
                )));
            }
            let _ = std::fs::remove_dir_all(&self.snapshot);
            std::fs::rename(&tmp, &self.snapshot)?;
            return Ok(());
        }
        if !offline {
            // best-effort refresh; on failure (airplane, flaky network) the
            // existing snapshot is still usable
            if run_git(&["fetch", "--depth", "1", "origin"], Some(&self.snapshot)).is_ok() {
                let _ = run_git(&["reset", "--hard", "FETCH_HEAD"], Some(&self.snapshot));
            }
        }
        Ok(())
    }

    fn pkg_dir(&self, name: &str) -> PathBuf {
        self.snapshot.join("packages").join(name)
    }

    fn index_path(&self, name: &str) -> PathBuf {
        self.pkg_dir(name).join("index.json")
    }

    /// Commit `index.json` (and any new tarball) plus a tag, and push.
    fn commit_and_push(&self, work: &Path, msg: &str) -> Result<(), PkgError> {
        run_git(&["add", "-A"], Some(work))?;
        // detect "nothing to commit" (idempotent re-publish)
        let status = run_git(&["status", "--porcelain"], Some(work))?;
        if !status.is_empty() {
            run_git(
                &[
                    "-c",
                    "user.name=aoxn-pkg",
                    "-c",
                    "user.email=aoxn@localhost",
                    "commit",
                    "-m",
                    msg,
                ],
                Some(work),
            )?;
        }
        run_git(&["push", "origin", "HEAD"], Some(work))?;
        Ok(())
    }

    /// Push a single tag; already-present tags are fine (idempotent retry).
    fn push_tag(&self, tag: &str) -> Result<(), PkgError> {
        match run_git(&["push", "origin", tag], None) {
            Ok(_) => Ok(()),
            Err(e) => {
                let msg = format!("{e}");
                if msg.contains("already exists") {
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    }
}

impl Registry for GitRegistry {
    fn url(&self) -> &str {
        &self.url
    }

    fn index(&mut self, name: &str) -> Result<PackageIndex, PkgError> {
        self.ensure_snapshot(false)?;
        let path = self.index_path(name);
        if !path.exists() {
            return Err(PkgError::PackageNotFound(name.to_string()));
        }
        let idx: PackageIndex = read_json_or_default(&path)?;
        Ok(idx)
    }

    fn all_names(&mut self) -> Result<Vec<String>, PkgError> {
        self.ensure_snapshot(false)?;
        let packages = self.snapshot.join("packages");
        let mut names = Vec::new();
        if packages.exists() {
            for e in std::fs::read_dir(&packages)? {
                let e = e?;
                if e.path().is_dir() {
                    names.push(e.file_name().to_string_lossy().to_string());
                }
            }
        }
        names.sort();
        Ok(names)
    }

    fn fetch_tarball(
        &mut self,
        cache: &Cache,
        name: &str,
        version: &str,
        checksum: &str,
        _tarball_sha256: Option<&str>,
        offline: bool,
    ) -> Result<PathBuf, PkgError> {
        let cached = cache.tar_path(checksum);
        let cached = cached?;
        if cached.exists() {
            return Ok(cached);
        }
        self.ensure_snapshot(offline)?;
        let src = self.pkg_dir(name).join(format!("{version}.tar.gz"));
        if !src.exists() {
            return Err(PkgError::Registry(format!(
                "registry snapshot for `{}` lacks {name}@{version} \
                 (snapshot may predate the publish; retry online)",
                self.url
            )));
        }
        let stored = cache.store_tar(&src, checksum)?;
        Ok(stored)
    }

    fn fork(&self) -> Result<Box<dyn Registry>, PkgError> {
        Ok(Box::new(GitRegistry {
            url: self.url.clone(),
            snapshot: self.snapshot.clone(),
        }))
    }

    fn trust(&mut self) -> Result<Option<crate::trust::TrustIndex>, PkgError> {
        self.ensure_snapshot(false)?;
        crate::trust::load_trust_file(&self.snapshot.join("trust.json"))
    }

    fn advisories_dir(&mut self) -> Result<Option<PathBuf>, PkgError> {
        self.ensure_snapshot(false)?;
        let dir = self.snapshot.join("advisories");
        Ok(if dir.is_dir() { Some(dir) } else { None })
    }

    fn publish(&mut self, req: &PublishRequest, dry_run: bool) -> Result<PublishOutcome, PkgError> {
        // Concurrent-publish safety: the whole apply+push sequence retries
        // on push rejection (someone else pushed first). Idempotency is
        // decided by index.json content alone (version present + same
        // checksum => success), never by tag state, so a half-finished
        // previous attempt (branch pushed, tag not) converges on retry.
        const MAX_ATTEMPTS: usize = 3;
        let mut last_err: Option<PkgError> = None;
        for attempt in 0..MAX_ATTEMPTS {
            match self.publish_attempt(req, dry_run) {
                Ok(outcome) => return Ok(outcome),
                Err(PublishAttemptError::Retryable(e)) => {
                    last_err = Some(e);
                    std::thread::sleep(std::time::Duration::from_millis(500 * (attempt as u64 + 1)));
                }
                Err(PublishAttemptError::Fatal(e)) => return Err(e),
            }
        }
        Err(PkgError::Registry(format!(
            "publishing {}@{} kept failing with concurrent pushes; another publish just landed —              retry in a moment ({})",
            req.name,
            req.version,
            last_err.map(|e| format!("{e}")).unwrap_or_default()
        )))
    }

    fn set_yank(&mut self, name: &str, version: &str, yanked: bool) -> Result<(), PkgError> {
        let work = std::env::temp_dir().join(format!(
            "aoxn-yank-{}-{}",
            crate::cache::sha256_hex(self.url.as_bytes())[..8].to_string(),
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&work);
        let out = Command::new("git")
            .args(["clone", &self.url])
            .arg(&work)
            .output()
            .map_err(|e| PkgError::Registry(format!("cannot run git: {e}")))?;
        if !out.status.success() {
            let _ = std::fs::remove_dir_all(&work);
            return Err(PkgError::Registry(format!(
                "cloning registry failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        let result = (|| {
            let idx_path = work.join("packages").join(name).join("index.json");
            let mut idx: PackageIndex = read_json_or_default(&idx_path)?;
            let Some(entry) = idx.versions.get_mut(version) else {
                return Err(PkgError::Registry(format!(
                    "{name}@{version} not found in registry"
                )));
            };
            if entry.yanked == yanked {
                return Ok(()); // idempotent
            }
            entry.yanked = yanked;
            write_json(&idx_path, &idx)?;
            let verb = if yanked { "yank" } else { "unyank" };
            self.commit_and_push(&work, &format!("{verb} {name}@{version}"))
        })();
        let _ = std::fs::remove_dir_all(&work);
        result
    }
}

impl GitRegistry {
    fn publish_attempt(&self, req: &PublishRequest, dry_run: bool) -> Result<PublishOutcome, PublishAttemptError> {
        // work on a full clone in a temp dir (the cache snapshot is shallow)
        let work = std::env::temp_dir().join(format!(
            "aoxn-publish-{}-{}",
            crate::cache::sha256_hex(self.url.as_bytes())[..8].to_string(),
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&work);
        let out = Command::new("git")
            .args(["clone", &self.url])
            .arg(&work)
            .output()
            .map_err(|e| PublishAttemptError::Fatal(PkgError::Registry(format!("cannot run git: {e}"))))?;
        if !out.status.success() {
            let _ = std::fs::remove_dir_all(&work);
            return Err(PublishAttemptError::Fatal(PkgError::Registry(format!(
                "cloning registry `{}` failed: {}",
                self.url,
                String::from_utf8_lossy(&out.stderr).trim()
            ))));
        }

        let result = (|| -> Result<PublishOutcome, PublishAttemptError> {
            let idx_path = work.join("packages").join(&req.name).join("index.json");
            let mut idx: PackageIndex = read_json_or_default(&idx_path)
                .map_err(PublishAttemptError::Fatal)?;
            idx.name = req.name.clone();
            if let Some(existing) = idx.get(&req.version) {
                return if existing.checksum == req.checksum {
                    Ok(PublishOutcome::AlreadyPublished)
                } else {
                    Err(PublishAttemptError::Fatal(PkgError::VersionConflict(format!(
                        "{}@{} is already published with a different checksum                          (published tarballs are immutable); bump the version instead",
                        req.name, req.version
                    ))))
                };
            }
            if !dry_run {
                // published versions must be strictly newer than every
                // existing (non-yanked) one
                if let Some((latest, _)) = idx.latest(true) {
                    let latest_v: semver::Version = latest.parse().map_err(|_| {
                        PublishAttemptError::Fatal(PkgError::Registry(format!(
                            "registry has non-semver version `{latest}`"
                        )))
                    })?;
                    let this: semver::Version = req.version.parse().map_err(|_| {
                        PublishAttemptError::Fatal(PkgError::VersionConflict(format!(
                            "`{}` is not a semver version", req.version
                        )))
                    })?;
                    if this <= latest_v {
                        return Err(PublishAttemptError::Fatal(PkgError::VersionConflict(format!(
                            "cannot publish {}: registry already has {}                              (versions must be strictly increasing)",
                            req.version, latest
                        ))));
                    }
                }
                let tgz_dest = work
                    .join("packages")
                    .join(&req.name)
                    .join(format!("{}.tar.gz", req.version));
                std::fs::create_dir_all(tgz_dest.parent().unwrap())
                    .map_err(|e| PublishAttemptError::Fatal(PkgError::Io(e)))?;
                std::fs::copy(&req.tarball, &tgz_dest)
                    .map_err(|e| PublishAttemptError::Fatal(PkgError::Io(e)))?;
                idx.versions.insert(
                    req.version.clone(),
                    super::IndexVersion {
                        checksum: req.checksum.clone(),
                        tarball_sha256: req.tarball_sha256.clone(),
                        dependencies: req.dependencies.clone(),
                        aoxn: req.aoxn.clone(),
                        ..Default::default()
                    },
                );
                write_json(&idx_path, &idx).map_err(PublishAttemptError::Fatal)?;
                // order matters: land the commit first, the tag second — a
                // crash between them is repaired by the retry loop above
                self.commit_and_push(&work, &format!("publish {}@{}", req.name, req.version))
                    .map_err(PublishAttemptError::classify)?;
                self.push_tag(&format!("pkg/{}/v{}", req.name, req.version))
                    .map_err(PublishAttemptError::classify)?;
            }
            Ok(PublishOutcome::Published)
        })();

        let _ = std::fs::remove_dir_all(&work);
        result
    }
}

/// Failure classification for the publish retry loop: a rejected push is
/// worth retrying (someone else published concurrently); anything else is
/// a hard stop.
enum PublishAttemptError {
    Retryable(PkgError),
    Fatal(PkgError),
}

impl PublishAttemptError {
    fn classify(e: PkgError) -> PublishAttemptError {
        let msg = format!("{e}");
        if msg.contains("rejected") || msg.contains("non-fast-forward")
            || msg.contains("failed to push") || msg.contains("fetch first")
        {
            PublishAttemptError::Retryable(e)
        } else {
            PublishAttemptError::Fatal(e)
        }
    }
}
