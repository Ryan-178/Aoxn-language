//! Directory-tree registry backend: same layout as the git backend but no
//! version control. Used for air-gapped CI (a synced folder is the
//! registry), local testing, and the end-to-end test suite.

use std::path::{Path, PathBuf};

use super::{read_json_or_default, write_json, PackageIndex, PublishOutcome, PublishRequest, Registry};
use crate::cache::Cache;
use crate::errors::PkgError;

pub struct DirRegistry {
    root: PathBuf,
    url: String,
}

impl DirRegistry {
    pub fn new(root: &Path) -> DirRegistry {
        let root = root.to_path_buf();
        let url = root.to_string_lossy().to_string();
        DirRegistry { root, url }
    }

    fn pkg_dir(&self, name: &str) -> PathBuf {
        self.root.join("packages").join(name)
    }

    fn index_path(&self, name: &str) -> PathBuf {
        self.pkg_dir(name).join("index.json")
    }

    fn require_root(&self) -> Result<(), PkgError> {
        if !self.root.is_dir() {
            return Err(PkgError::Registry(format!(
                "directory registry `{}` does not exist",
                self.root.display()
            )));
        }
        Ok(())
    }
}

impl Registry for DirRegistry {
    fn url(&self) -> &str {
        &self.url
    }

    fn index(&mut self, name: &str) -> Result<PackageIndex, PkgError> {
        self.require_root()?;
        let path = self.index_path(name);
        if !path.exists() {
            return Err(PkgError::PackageNotFound(name.to_string()));
        }
        read_json_or_default(&path)
    }

    fn all_names(&mut self) -> Result<Vec<String>, PkgError> {
        self.require_root()?;
        let packages = self.root.join("packages");
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
        _offline: bool,
    ) -> Result<PathBuf, PkgError> {
        let cached = cache.tar_path(checksum);
        let cached = cached?;
        if cached.exists() {
            return Ok(cached);
        }
        let src = self.pkg_dir(name).join(format!("{version}.tar.gz"));
        if !src.exists() {
            return Err(PkgError::Registry(format!(
                "registry `{}` lacks {name}@{version} tarball",
                self.root.display()
            )));
        }
        cache.store_tar(&src, checksum)
    }

    fn fork(&self) -> Result<Box<dyn Registry>, PkgError> {
        Ok(Box::new(DirRegistry::new(&self.root)))
    }

    fn trust(&mut self) -> Result<Option<crate::trust::TrustIndex>, PkgError> {
        crate::trust::load_trust_file(&self.root.join("trust.json"))
    }

    fn advisories_dir(&mut self) -> Result<Option<PathBuf>, PkgError> {
        let dir = self.root.join("advisories");
        Ok(if dir.is_dir() { Some(dir) } else { None })
    }

    fn publish(&mut self, req: &PublishRequest, dry_run: bool) -> Result<PublishOutcome, PkgError> {
        self.require_root()?;
        let mut idx: PackageIndex = read_json_or_default(&self.index_path(&req.name))?;
        idx.name = req.name.clone();
        if let Some(existing) = idx.get(&req.version) {
            return if existing.checksum == req.checksum {
                Ok(PublishOutcome::AlreadyPublished)
            } else {
                Err(PkgError::VersionConflict(format!(
                    "{}@{} is already published with a different checksum \
                     (published tarballs are immutable); bump the version instead",
                    req.name, req.version
                )))
            };
        }
        if !dry_run {
            if let Some((latest, _)) = idx.latest(true) {
                let latest_v: semver::Version = latest.parse().map_err(|_| {
                    PkgError::Registry(format!("registry has non-semver version `{latest}`"))
                })?;
                let this: semver::Version = req
                    .version
                    .parse()
                    .map_err(|_| PkgError::VersionConflict(format!("`{}` is not semver", req.version)))?;
                if this <= latest_v {
                    return Err(PkgError::VersionConflict(format!(
                        "cannot publish {}: registry already has {} (versions must be strictly increasing)",
                        req.version, latest
                    )));
                }
            }
            let tgz_dest = self
                .pkg_dir(&req.name)
                .join(format!("{}.tar.gz", req.version));
            std::fs::create_dir_all(tgz_dest.parent().unwrap())?;
            std::fs::copy(&req.tarball, &tgz_dest)?;
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
            write_json(&self.index_path(&req.name), &idx)?;
        }
        Ok(PublishOutcome::Published)
    }

    fn set_yank(&mut self, name: &str, version: &str, yanked: bool) -> Result<(), PkgError> {
        self.require_root()?;
        let idx_path = self.index_path(name);
        let mut idx: PackageIndex = read_json_or_default(&idx_path)?;
        let Some(entry) = idx.versions.get_mut(version) else {
            return Err(PkgError::Registry(format!("{name}@{version} not found in registry")));
        };
        if entry.yanked == yanked {
            return Ok(());
        }
        entry.yanked = yanked;
        write_json(&idx_path, &idx)
    }
}
