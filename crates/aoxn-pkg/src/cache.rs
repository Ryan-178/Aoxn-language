//! Global content-addressed cache (`$AOXN_HOME`, default `~/.aoxn`).
//!
//! Layout:
//! ```text
//! $AOXN_HOME/
//!   config.json              # global config (advisories source, ...)
//!   tarballs/<sha256>.tar.gz # content-addressed package tarballs
//!   registries/<sha8(url)>/  # per-registry snapshot (shallow git clone)
//! ```
//!
//! Safe to share read-only between machines/CI workers: everything is keyed
//! by content hash; writes are copy-then-verify.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::errors::PkgError;

/// A manifest checksum is exactly 64 lowercase hex digits (`sha256_hex`
/// below). Anything else is refused BEFORE it reaches a path, because the
/// places a checksum is used are filesystem-shaped and a registry supplies it.
pub fn validate_checksum(cksum: &str) -> Result<(), PkgError> {
    let ok = cksum.len() == 64
        && cksum.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if ok {
        return Ok(());
    }
    Err(PkgError::Registry(format!(
        "invalid checksum `{cksum}`: expected 64 lowercase hex digits (sha256)"
    )))
}

pub fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex::encode(h.finalize())
}

#[allow(dead_code)] // used by tarball tests; part of the cache API
pub fn sha256_file(path: &Path) -> Result<String, PkgError> {
    let data = std::fs::read(path)?;
    Ok(sha256_hex(&data))
}

#[derive(Debug, Clone)]
pub struct Cache {
    pub root: PathBuf,
}

impl Cache {
    /// Resolve the global cache root: `$AOXN_HOME` wins, else `~/.aoxn`.
    pub fn global() -> Cache {
        let root = std::env::var_os("AOXN_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".aoxn")
            });
        Cache { root }
    }

    pub fn create_dirs(&self) -> Result<(), PkgError> {
        std::fs::create_dir_all(self.tarballs())?;
        std::fs::create_dir_all(self.registries())?;
        Ok(())
    }

    pub fn tarballs(&self) -> PathBuf {
        self.root.join("tarballs")
    }

    pub fn registries(&self) -> PathBuf {
        self.root.join("registries")
    }

    pub fn config_path(&self) -> PathBuf {
        self.root.join("config.json")
    }

    /// Directory holding a registry's local snapshot (shallow clone), keyed
    /// by a short hash of its URL.
    pub fn registry_dir(&self, url: &str) -> PathBuf {
        let key: String = sha256_hex(url.as_bytes())[..12].to_string();
        self.registries().join(key)
    }

    /// The cache path for a tarball.
    ///
    /// The checksum is interpolated into a FILESYSTEM PATH, so it is
    /// validated here rather than trusted: a registry advertising
    /// `"checksum": "../escaped"` made this write to `<cache>/escaped.tar.gz`,
    /// outside the `tarballs/` namespace — and the write happens BEFORE the
    /// content-integrity check, so a failing check never undoes it. A real
    /// checksum is `hex::encode(sha256(..))`: exactly 64 lowercase hex
    /// digits, which is what this accepts (a lockfile written by any other
    /// tool is rejected outright instead of escaping).
    pub fn tar_path(&self, checksum: &str) -> Result<PathBuf, PkgError> {
        validate_checksum(checksum)?;
        Ok(self.tarballs().join(format!("{checksum}.tar.gz")))
    }

    /// Copy a tarball into the cache, keyed by the package's manifest
    /// checksum. The cache is transport storage; *integrity* is verified
    /// after extraction against the manifest hash (see `tarball::manifest_hash`),
    /// so no byte-level check happens here.
    pub fn store_tar(&self, src: &Path, checksum: &str) -> Result<PathBuf, PkgError> {
        let dst = self.tar_path(checksum)?;
        if dst.exists() {
            return Ok(dst);
        }
        self.create_dirs()?;
        let tmp = self.tarballs().join(format!(
            "{}.tmp-{}",
            &checksum[..12],
            std::process::id()
        ));
        std::fs::copy(src, &tmp)?;
        std::fs::rename(&tmp, &dst)?;
        Ok(dst)
    }

    /// Remove tarballs not referenced by `keep` (checksums) that are also
    /// older than `ttl_days` — the TTL protects a concurrent install that
    /// resolved but has not fetched yet. Returns the number deleted.
    pub fn gc_old(&self, keep: &HashSet<String>, ttl_days: u64) -> Result<usize, PkgError> {
        let ttl = std::time::Duration::from_secs(ttl_days * 24 * 3600);
        let mut deleted = 0;
        for entry in std::fs::read_dir(self.tarballs())? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(checksum) = name.strip_suffix(".tar.gz") else {
                continue;
            };
            if keep.contains(checksum) {
                continue;
            }
            let old = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|m| m.elapsed().ok())
                .map(|age| age >= ttl)
                .unwrap_or(false);
            if old {
                std::fs::remove_file(entry.path())?;
                deleted += 1;
            }
        }
        Ok(deleted)
    }

    /// Drop every registry snapshot (they re-fetch on demand).
    pub fn prune_registries(&self) -> Result<usize, PkgError> {
        let mut deleted = 0;
        if self.registries().exists() {
            for entry in std::fs::read_dir(self.registries())? {
                let entry = entry?;
                if entry.path().is_dir() {
                    std::fs::remove_dir_all(entry.path())?;
                    deleted += 1;
                }
            }
        }
        Ok(deleted)
    }

    /// Delete the entire cache.
    pub fn clean(&self) -> Result<(), PkgError> {
        if self.root.exists() {
            std::fs::remove_dir_all(&self.root)?;
        }
        Ok(())
    }

    pub fn size_on_disk(&self) -> u64 {
        fn walk(dir: &Path, total: &mut u64) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, total);
                } else if let Ok(m) = e.metadata() {
                    *total += m.len();
                }
            }
        }
        let mut total = 0;
        walk(&self.root, &mut total);
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_vector() {
        assert_eq!(
            sha256_hex(b"hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn registry_dir_is_stable_and_distinct() {
        let c = Cache {
            root: PathBuf::from("/tmp/x"),
        };
        let a = c.registry_dir("https://github.com/u/r");
        let b = c.registry_dir("https://github.com/u/r");
        let d = c.registry_dir("https://example.com/other");
        assert_eq!(a, b);
        assert_ne!(a, d);
    }

    /// The checksum is interpolated into a cache PATH, and the write happens
    /// before the content check that would have caught a bad archive. A
    /// registry advertising `"checksum": "../escaped"` wrote
    /// `<cache>/escaped.tar.gz`, outside the `tarballs/` namespace.
    #[test]
    fn a_malformed_checksum_never_becomes_a_path() {
        let c = Cache { root: std::env::temp_dir().join(format!("aoxn-pkg-cksum-{}", std::process::id())) };
        for bad in ["../escaped", "..", "deadbeef", "", &"A".repeat(64), &"g".repeat(64)] {
            let err = c.tar_path(bad).unwrap_err();
            assert!(
                format!("{err}").contains("expected 64 lowercase hex digits"),
                "checksum `{bad}` must be refused: {err}"
            );
        }
        // a real one still resolves inside the cache
        let ok = c.tar_path(&"d".repeat(64)).unwrap();
        assert!(ok.starts_with(c.tarballs()));
    }
}