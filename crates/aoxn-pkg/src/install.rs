//! Install: resolve → lock → materialize `aox_modules/`.
//!
//! Materialization rules (what makes `import { x } from "pkg"` work with a
//! **zero-change compiler**):
//! - registry deps: the tarball is extracted to `<pkg>/aox_modules/<name>/`;
//!   if the package's entry file is not a root-level `index.ax`, a shim
//!   `index.ax` (`import * from "./<entry>"`) is generated — the loader
//!   merges names transitively, so the shim is fully transparent.
//! - path deps (workspace members): `<pkg>/aox_modules/<name>/` contains
//!   only a shim `index.ax` whose import path points at the member's real
//!   entry file — sources of truth stay in place, edits are live.
//! - everything under `aox_modules/` is managed: stale entries are pruned
//!   on every install.
//!
//! v0.31.0 additions: dev dependencies share the single lockfile and are cut
//! out at materialization time (`--prod`); tarballs download in parallel;
//! the declared minimum compiler version is enforced; curated registries get
//! their trust index consulted before anything is written.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use crate::cache::Cache;
use crate::context::{Ctx, DepScope};
use crate::errors::PkgError;
use crate::lockfile::{req_key, Lockfile, LockedPackage};
use crate::manifest::{DependencySpec, Manifest};
use crate::registry::Registry;
use crate::resolve::{resolve_deps, Resolution, Selected};
use crate::tarball;
use crate::ui::Ui;

/// Default download concurrency (pnpm-style). Override with `--jobs` or
/// `AOXN_JOBS`; `1` restores the old strictly serial behavior.
pub const DEFAULT_JOBS: usize = 8;

pub struct InstallOpts {
    pub force_resolve: bool,
    /// CI mode: never resolve or touch the lockfile; fail when it does not
    /// exactly satisfy the manifests
    pub frozen: bool,
    pub dry_run: bool,
    pub scope: DepScope,
    pub jobs: usize,
}

impl Default for InstallOpts {
    fn default() -> Self {
        InstallOpts {
            force_resolve: false,
            frozen: false,
            dry_run: false,
            scope: DepScope::All,
            jobs: DEFAULT_JOBS,
        }
    }
}

pub struct InstallReport {
    pub resolved: usize,
    pub installed: Vec<String>,
    pub removed: Vec<String>,
    pub reused_cache: usize,
    pub downloaded: usize,
    /// dev-only packages left out of `aox_modules/` by this install
    pub skipped_dev: Vec<String>,
}

impl InstallReport {
    /// Machine-readable form for `aoxn install --json` (what pip calls its
    /// `--report`): a flat, stable description of what the install settled.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "resolved": self.resolved,
            "installed": self.installed,
            "removed": self.removed,
            "reused_cache": self.reused_cache,
            "downloaded": self.downloaded,
            "skipped_dev": self.skipped_dev,
        })
    }
}

/// Compute the manifest requirements fingerprint:
/// `"<owner>/<dep>" -> "<req>@<registry>"`.
///
/// The owner-qualified key matters in a workspace (two members may pin the
/// same dependency differently, and a bare name let them overwrite each
/// other); the value deliberately carries no filesystem path, so renaming or
/// moving the project no longer invalidates the lockfile fast path.
fn requirements_fingerprint(packages: &[(PathBuf, Manifest)]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (_, m) in packages {
        for (dep, spec) in m.dependencies.iter().chain(&m.dev_dependencies) {
            if let DependencySpec::Registry { req, registry } = spec {
                let url = registry.clone().unwrap_or_default();
                out.insert(req_key(&m.name, dep), format!("{req}@{url}"));
            }
        }
    }
    out
}

/// Names of the production dependencies across every package in the
/// workspace — the roots of the prod half of the graph. A path dependency
/// declared outside `devDependencies` counts as a prod root too.
fn prod_roots_of(
    packages: &[(PathBuf, Manifest)],
    path_deps: &[(String, PathBuf, bool)],
) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    for (_, m) in packages {
        for (dep, spec) in &m.dependencies {
            if let DependencySpec::Registry { .. } = spec {
                out.insert(dep.clone());
            }
        }
    }
    for (name, _, declared_dev) in path_deps {
        if !declared_dev {
            out.insert(name.clone());
        }
    }
    out.into_iter().collect()
}

pub fn install(ctx: &mut Ctx, ui: &Ui, opts: InstallOpts) -> Result<InstallReport, PkgError> {
    let packages = ctx.all_package_manifests()?;
    let old_lock = ctx.load_lockfile()?.unwrap_or_else(Lockfile::empty);

    // ---- 1. decide whether resolution is needed
    let reqs = requirements_fingerprint(&packages);
    let lock_matches_manifest = old_lock.requirements == reqs;
    let needs_resolve = opts.force_resolve || old_lock.packages.is_empty() || !lock_matches_manifest;
    if !needs_resolve {
        // in-sync fast path: belt-and-braces — every manifest constraint
        // must be satisfied by the locked versions
        if let Err(why) = old_lock.satisfies_manifests(&reqs) {
            if opts.frozen {
                return Err(PkgError::Lockfile(format!(
                    "--frozen: lockfile does not satisfy the manifests: {why}"
                )));
            }
            // stale-but-recoverable: fall through to a re-resolve
            return install(
                ctx,
                ui,
                InstallOpts {
                    force_resolve: true,
                    frozen: false,
                    ..opts
                },
            );
        }
    }
    if opts.frozen && needs_resolve {
        return Err(PkgError::Lockfile(format!(
            "--frozen: {} is out of date with the manifests (run `aoxn install` once to update it, or commit a fresh aoxn.lock in CI)",
            ctx.lockfile_path().display()
        )));
    }

    let resolution: Resolution;
    let mut lock = old_lock.clone();
    let mut newly_added: Vec<String> = Vec::new();
    let path_deps = ctx.path_dependencies()?;
    if needs_resolve {
        // dev and prod resolve together into one lockfile
        let roots = ctx.roots(DepScope::All)?;
        let default_registry = if roots.is_empty() {
            String::new()
        } else {
            ctx.registry_url(None)?
        };
        let overrides = collect_overrides(&packages);
        let locked = old_lock.locked_versions();
        let prev: HashSet<String> = locked.keys().cloned().collect();
        let res = resolve_deps(roots, overrides, ctx, &default_registry, &locked)?;

        check_engines(&res)?;

        // typosquat guard: newly added packages compared against registry names
        let added = crate::resolve::added_packages(&res, &prev);
        newly_added = added.clone();
        typosquat_check(ctx, ui, &added)?;
        trust_warnings(ctx, ui, &res);

        // yank / deprecation warnings for everything selected
        for (name, sel) in &res.packages {
            if sel.yanked {
                ui.warn(&format!(
                    "{name}@{} is yanked in the registry but pinned in your lockfile; plan a migration (aoxn update --latest {name})",
                    sel.version
                ));
            }
            if let Some(msg) = &sel.deprecated {
                ui.warn(&format!(
                    "{name}@{} is deprecated{}",
                    sel.version,
                    if msg.is_empty() { String::new() } else { format!(": {msg}") }
                ));
            }
        }

        // ---- 2. write the new lockfile
        let mut new_lock = Lockfile::empty();
        new_lock.requirements = reqs.clone();
        for (name, sel) in &res.packages {
            new_lock.insert(LockedPackage {
                name: name.clone(),
                version: sel.version.clone(),
                registry: sel.registry.clone(),
                checksum: sel.checksum.clone(),
                tarball_sha256: sel.tarball_sha256.clone(),
                dev: false,
                dependencies: sel.dependencies.keys().map(|d| d.clone()).collect(),
            });
        }
        // path dependencies are part of the lock too, marked `path+<abs>`
        // with an empty checksum (their source of truth is the local tree)
        for (name, target, _) in &path_deps {
            if new_lock.get(name).is_some() {
                continue;
            }
            let m = Manifest::load(&Manifest::path_for(target))?;
            new_lock.insert(LockedPackage {
                name: name.to_string(),
                version: m.version,
                registry: format!("path+{}", target.display()),
                checksum: String::new(),
                tarball_sha256: None,
                dev: false,
                dependencies: m.dependencies.keys().map(|d| d.clone()).collect(),
            });
        }
        new_lock.mark_dev_only(&prod_roots_of(&packages, &path_deps));
        report_lock_diff(ui, &old_lock, &new_lock);
        lock = new_lock;
        resolution = res;
    } else {
        // lockfile is authoritative — no resolution, no network
        ui.info(&format!(
            "using locked versions from {} ({} packages)",
            ctx.lockfile_path().display(),
            lock.packages.len()
        ));
        // build a pseudo-resolution from the lock for materialization
        let mut pkgs = BTreeMap::new();
        for lp in &lock.packages {
            pkgs.insert(
                lp.name.clone(),
                Selected {
                    version: lp.version.clone(),
                    registry: lp.registry.clone(),
                    checksum: lp.checksum.clone(),
                    tarball_sha256: lp.tarball_sha256.clone(),
                    dependencies: lp
                        .dependencies
                        .iter()
                        .filter_map(|d| {
                            d.split_once('@').map(|(n, v)| (n.to_string(), v.to_string()))
                        })
                        .collect(),
                    deprecated: None,
                    yanked: false,
                    min_aoxn: None,
                },
            );
        }
        resolution = Resolution { packages: pkgs };
    }

    if opts.dry_run {
        return Ok(InstallReport {
            resolved: lock.packages.len(),
            installed: newly_added,
            removed: diff_removed(&old_lock, &lock),
            reused_cache: 0,
            downloaded: 0,
            skipped_dev: Vec::new(),
        });
    }

    // ---- 3. prefetch tarballs (grouped per registry, verified against the
    //         lockfile checksums)
    let (reused, downloaded) = prefetch(ctx, ui, &resolution, opts.jobs)?;

    // ---- 4. materialize aox_modules for every package dir
    let mut removed_total: Vec<String> = Vec::new();
    let mut skipped_dev: Vec<String> = Vec::new();
    let mut materialized: Vec<String> = Vec::new();
    for (dir, manifest) in &packages {
        let (installed, removed, skipped) = materialize(
            dir,
            manifest,
            &resolution,
            &lock,
            &path_deps,
            &ctx.cache,
            opts.scope,
            ui,
        )?;
        materialized.extend(installed);
        removed_total.extend(removed);
        skipped_dev.extend(skipped);
    }
    materialized.sort();
    materialized.dedup();
    skipped_dev.sort();
    skipped_dev.dedup();

    // ---- 5. persist the lockfile
    if needs_resolve {
        lock.save(&ctx.lockfile_path())?;
        ui.info(&format!(
            "wrote {} ({} packages)",
            ctx.lockfile_path().display(),
            lock.packages.len()
        ));
    }

    Ok(InstallReport {
        resolved: lock.packages.len(),
        installed: materialized,
        removed: removed_total,
        reused_cache: reused,
        downloaded,
        skipped_dev,
    })
}

/// Merge every workspace member's `overrides` (a union: members may each
/// force a different package, and a key nobody overrides keeps its value).
fn collect_overrides(packages: &[(PathBuf, Manifest)]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (_, m) in packages {
        for (k, v) in &m.overrides {
            out.insert(k.clone(), v.clone());
        }
    }
    out
}

/// Names reachable from the production roots — everything else in the
/// lockfile is dev-only and gets pruned by `aoxn install --prod`.
#[cfg(test)]
fn prod_closure(
    packages: &[(PathBuf, Manifest)],
    res: &Resolution,
    path_deps: &[(String, PathBuf, bool)],
) -> BTreeSet<String> {
    // build the same shape install() builds, then ask the lockfile
    let mut lf = Lockfile::empty();
    for (name, sel) in &res.packages {
        lf.insert(LockedPackage {
            name: name.clone(),
            version: sel.version.clone(),
            registry: sel.registry.clone(),
            checksum: sel.checksum.clone(),
            tarball_sha256: sel.tarball_sha256.clone(),
            dev: false,
            dependencies: sel.dependencies.keys().map(|d| d.clone()).collect(),
        });
    }
    for (name, _, _) in path_deps {
        if lf.get(name).is_none() {
            lf.insert(LockedPackage {
                name: name.clone(),
                version: "0.0.0".into(),
                registry: "path+x".into(),
                checksum: String::new(),
                tarball_sha256: None,
                dev: false,
                dependencies: vec![],
            });
        }
    }
    lf.mark_dev_only(&prod_roots_of(packages, path_deps));
    lf.closure(&prod_roots_of(packages, path_deps))
}

/// Reject packages whose declared minimum compiler version is newer than the
/// running one. `IndexVersion.aoxn` was parsed by every registry backend
/// since v0.29.0 and never consulted; a package can legitimately need
/// language or stdlib features the installed compiler lacks, and finding
/// that out mid-compile is far worse than finding it out here.
fn check_engines(res: &Resolution) -> Result<(), PkgError> {
    let running = env!("CARGO_PKG_VERSION");
    let Ok(current) = semver::Version::parse(running) else {
        return Ok(());
    };
    // report every offender at once rather than one per run
    let mut offenders: Vec<(&String, &Selected)> = res
        .packages
        .iter()
        .filter(|(_, sel)| {
            sel.min_aoxn
                .as_deref()
                .and_then(|r| semver::VersionReq::parse(r).ok())
                .map(|req| !req.matches(&current))
                .unwrap_or(false)
        })
        .collect();
    if offenders.is_empty() {
        return Ok(());
    }
    offenders.sort_by_key(|(name, _)| name.as_str());
    let detail = offenders
        .iter()
        .map(|(name, sel)| {
            format!(
                "{}@{} needs aoxn >= {}",
                name,
                sel.version,
                sel.min_aoxn.as_deref().unwrap_or("?")
            )
        })
        .collect::<Vec<_>>()
        .join("\n  ");
    Err(PkgError::engines(
        offenders[0]
            .1
            .min_aoxn
            .clone()
            .unwrap_or_default(),
        detail,
    ))
}

/// Consult each registry's trust index and name the packages it has no
/// reviewed record for. A registry without a `trust.json` stays silent — a
/// warning that fires on every install in the world is one nobody reads.
fn trust_warnings(ctx: &mut Ctx, ui: &Ui, res: &Resolution) {
    let mut by_registry: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, sel) in &res.packages {
        if sel.registry.starts_with("path+") {
            continue;
        }
        by_registry
            .entry(sel.registry.clone())
            .or_default()
            .push(name.clone());
    }
    for (url, names) in by_registry {
        let Ok(reg) = ctx.registry_for(&url) else {
            continue;
        };
        let Ok(Some(trust)) = reg.trust() else {
            continue;
        };
        crate::trust::report_untrusted(ui, &trust, &url, &names);
    }
}

#[derive(Clone)]
struct TarballRef {
    name: String,
    version: String,
    checksum: String,
    tarball_sha256: Option<String>,
}

/// Download every missing tarball, up to `jobs` at a time per registry.
/// Returns (reused from cache, downloaded).
///
/// Each worker gets its own forked registry handle — the backends carry
/// mutable state and cannot be shared across threads. Progress is reported
/// once for the whole phase rather than per package: the per-package step
/// line rewrites one terminal row, which several threads would fight over.
fn prefetch(
    ctx: &mut Ctx,
    ui: &Ui,
    resolution: &Resolution,
    jobs: usize,
) -> Result<(usize, usize), PkgError> {
    let mut reused = 0usize;
    let mut missing: BTreeMap<String, Vec<TarballRef>> = BTreeMap::new();
    for (name, sel) in &resolution.packages {
        // path deps have no registry tarball to fetch
        if sel.registry.starts_with("path+") {
            continue;
        }
        if ctx.cache.tar_path(&sel.checksum)?.exists() {
            reused += 1;
            continue;
        }
        missing.entry(sel.registry.clone()).or_default().push(TarballRef {
            name: name.clone(),
            version: sel.version.clone(),
            checksum: sel.checksum.clone(),
            tarball_sha256: sel.tarball_sha256.clone(),
        });
    }

    let total: usize = missing.values().map(Vec::len).sum();
    if total == 0 {
        ui.info(&format!("all {reused} tarball(s) already cached"));
        return Ok((reused, 0));
    }

    // Slice each registry's backlog into one chunk per worker, and fork a
    // handle for each chunk.
    let jobs = jobs.max(1);
    let mut plan: Vec<(Vec<TarballRef>, Box<dyn Registry>)> = Vec::new();
    for (url, wanted) in &missing {
        let workers = jobs.min(wanted.len()).max(1);
        let chunk = wanted.len().div_ceil(workers);
        for part in wanted.chunks(chunk) {
            if part.is_empty() {
                continue;
            }
            let reg = ctx.registry_for(url)?;
            plan.push((part.to_vec(), reg.fork()?));
        }
    }

    let offline = ctx.offline;
    let cache = ctx.cache.clone();
    let step = ui.step(&format!("fetching {total} tarball(s)"));
    let serial = jobs == 1 || plan.len() <= 1;

    let mut downloaded = 0usize;
    let outcome: Result<(), PkgError> = if serial {
        let mut result = Ok(());
        'outer: for (part, reg) in plan.iter_mut() {
            for t in part.iter() {
                match reg.fetch_tarball(
                    &cache,
                    &t.name,
                    &t.version,
                    &t.checksum,
                    t.tarball_sha256.as_deref(),
                    offline,
                ) {
                    Ok(_) => downloaded += 1,
                    Err(e) => {
                        result = Err(e);
                        break 'outer;
                    }
                }
            }
        }
        result
    } else {
        let joined = std::thread::scope(|s| {
            let mut threads = Vec::new();
            for (part, reg) in plan {
                let cache = cache.clone();
                threads.push(s.spawn(move || -> Result<usize, PkgError> {
                    let mut reg = reg;
                    let mut n = 0usize;
                    for t in &part {
                        reg.fetch_tarball(
                            &cache,
                            &t.name,
                            &t.version,
                            &t.checksum,
                            t.tarball_sha256.as_deref(),
                            offline,
                        )?;
                        n += 1;
                    }
                    Ok(n)
                }));
            }
            threads
                .into_iter()
                .map(|h| h.join())
                .collect::<Vec<_>>()
        });
        let mut result = Ok(());
        for r in joined {
            match r {
                Ok(Ok(n)) => downloaded += n,
                Ok(Err(e)) => {
                    result = Err(e);
                    break;
                }
                Err(_) => {
                    result = Err(PkgError::other("a download worker thread panicked"));
                    break;
                }
            }
        }
        result
    };

    match outcome {
        Ok(()) => {
            step.done_ok_msg(&format!("downloaded {downloaded}, {reused} from cache"));
            Ok((reused, downloaded))
        }
        Err(e) => {
            step.done_fail("");
            Err(e)
        }
    }
}

/// Materialize one package dir's `aox_modules/`.
/// Returns (installed, removed, dev-only packages skipped).
#[allow(clippy::too_many_arguments)]
fn materialize(
    dir: &Path,
    manifest: &Manifest,
    resolution: &Resolution,
    lock: &Lockfile,
    path_deps: &[(String, PathBuf, bool)],
    cache: &Cache,
    scope: DepScope,
    ui: &Ui,
) -> Result<(Vec<String>, Vec<String>, Vec<String>), PkgError> {
    let modules_dir = dir.join("aox_modules");
    let mut keep: BTreeSet<String> = BTreeSet::new();
    let mut installed = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    // registry deps of this manifest (prod and dev, per scope)
    let mut specs: Vec<(&String, &DependencySpec)> = Vec::new();
    if scope.include_prod() {
        specs.extend(manifest.dependencies.iter());
    }
    if scope.include_dev() {
        specs.extend(manifest.dev_dependencies.iter());
    }
    for (dep, spec) in specs {
        if !matches!(spec, DependencySpec::Registry { .. }) {
            continue;
        }
        // A package listed in both tables is a prod dependency; under
        // `--prod` the dev table is not iterated at all, so it simply never
        // lands in `keep` and the prune below removes it. Which packages
        // that was is reported from the lockfile once pruning has run.
        let Some(lp) = lock.get(dep) else {
            continue;
        };
        keep.insert(dep.clone());
        if let Some(sel) = resolution.packages.get(dep) {
            install_registry_dep(cache, &modules_dir, dep, &sel.version, &sel.checksum)?;
        } else {
            install_registry_dep(cache, &modules_dir, dep, &lp.version, &lp.checksum)?;
        }
        installed.push(dep.clone());
    }

    // path deps (workspace members / local packages)
    for (name, target, declared_dev) in path_deps {
        let declared_here = manifest.dependencies.iter().any(|(d, s)| {
            matches!(s, DependencySpec::Path { .. }) && d == name
        }) || manifest.dev_dependencies.iter().any(|(d, s)| {
            matches!(s, DependencySpec::Path { .. }) && d == name
        });
        // every member sees every workspace member's shim, because a member
        // may be built on its own
        let depends = declared_here || manifest.workspace.is_some();
        if !depends {
            continue;
        }
        if scope == DepScope::Prod && *declared_dev && !declared_here {
            skipped.push(name.clone());
            continue;
        }
        keep.insert(name.clone());
        let target_manifest = Manifest::load(&Manifest::path_for(target))?;
        let Some(entry) = target_manifest.entry_file(target) else {
            return Err(PkgError::Manifest(format!(
                "path dependency `{name}` ({}): no entry file and no `main` in its manifest",
                target.display()
            )));
        };
        let shim_dir = modules_dir.join(name);
        std::fs::create_dir_all(&shim_dir)?;
        let rel = relative_path(&shim_dir, &target.join(&entry));
        std::fs::write(
            shim_dir.join("index.ax"),
            format!("import * from \"{rel}\"\n"),
        )?;
        installed.push(name.clone());
    }

    // prune managed-but-stale entries
    //
    // A name starting with `.` can never be a package (`validate_name` admits
    // only a-z / 0-9 / `-` / `_`), so a dot entry in `aox_modules/` belongs to
    // tooling, not to the lock. It is left alone: the sweep's job is to
    // reconcile package names against the lock, and swallowing other tools'
    // state would make this directory hostile. (Real precedent: the IDE's
    // language service drops its own marker here.)
    let mut removed = Vec::new();
    if modules_dir.exists() {
        for e in std::fs::read_dir(&modules_dir)? {
            let e = e?;
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            if !keep.contains(&name) {
                let p = e.path();
                if p.is_dir() {
                    std::fs::remove_dir_all(&p)?;
                } else {
                    std::fs::remove_file(&p)?;
                }
                removed.push(name);
            }
        }
    }
    // Which dev-only packages this run left out, read back from the lockfile
    // rather than from the iteration above: under `--prod` the dev table is
    // never visited, so the pruning pass is what actually excludes them.
    for p in &lock.packages {
        if p.dev && !keep.contains(&p.name) {
            skipped.push(p.name.clone());
        }
    }
    let _ = ui;
    skipped.sort();
    skipped.dedup();
    Ok((installed, removed, skipped))
}

/// Extract a locked registry dep into `modules_dir/<name>` (verify checksum
/// first — the cache entry is content-addressed, but verify anyway: cheap
/// and it pins the supply chain to the lockfile).
fn install_registry_dep(
    cache: &Cache,
    modules_dir: &Path,
    name: &str,
    version: &str,
    checksum: &str,
) -> Result<(), PkgError> {
    let tarball = cache.tar_path(checksum)?;
    if !tarball.exists() {
        return Err(PkgError::Offline(format!(
            "{name}@{version} is not cached yet and resolution was skipped; \
             run `aoxn install` online once"
        )));
    }
    // Defence in depth for the same reason: `dest` is about to be passed to
    // `remove_dir_all`. A dependency name reaches this function from the
    // manifest AND from registry metadata, and `Manifest::validate` already
    // applies the strict allowlist — but the operation in this function is a
    // recursive delete, so it checks the one property it actually depends on
    // (the name is a single safe path component) at the point of use.
    crate::manifest::validate_name(name).map_err(|_| {
        PkgError::Registry(format!("refusing to install package `{name}`: not a valid package name"))
    })?;
    let dest = modules_dir.join(name);
    // `join` of a validated name cannot escape; this asserts that the two
    // still agree, and fails loudly rather than deleting something.
    if dest.parent() != Some(&modules_dir) || dest.file_name().is_none() {
        return Err(PkgError::Registry(format!(
            "refusing to install package `{name}`: destination escapes {}",
            modules_dir.display()
        )));
    }
    if dest.exists() {
        // The tree may already be exactly what the lockfile describes, and on
        // a warm install -- the `aoxn pkg install` every CI re-run and
        // staleness check performs -- the old rm -r it took to find out was
        // the dominant cost. Measured on this machine, one project with 256
        // tiny packages: full warm install ~2.5 s, of which ~1.7 s was
        // `remove_dir_all` and most of the rest was re-extracting and
        // re-hashing the SAME bytes that had just been deleted.
        //
        // So measure BEFORE destroying. The compare is the same tree read the
        // extract-then-verify below does, but it must EXCLUDE `index.ax`:
        // that file is written here (write_shim_if_needed, below) after the
        // extract-time verify, and only when the manifest's entry is not
        // already index.ax. The checksum pins the PUBLISHED tree, which never
        // contained our shim; a warm tree differs from it by that one file of
        // our own making, and the skip is how the compare says so.
        //
        // The entry probe closes one hole the hash alone cannot see: if the
        // entry IS index.ax and a hand deletes that file, the excluding-hash
        // still matches -- but the loader needs an entry, so the tree must be
        // rebuilt. Anything else that made the tree wrong ALSO mismatches and
        // falls through to the rebuild, which is the pin working.
        //
        // A hand-edited package FILE before the install therefore still
        // rebuilds: only a tree genuinely worth keeping is kept. If the skip
        // ever skipped anything real, this is the line to revisit first.
        let ok_to_keep = if let Ok(actual) =
            crate::tarball::manifest_hash_skipping(&dest, "index.ax")
        {
            actual == checksum
        } else {
            false
        };
        if ok_to_keep {
            // The tree is kept, but the shim is REGENERATED if it is missing
            // or differs from what the manifest demands: it is excluded from
            // the compare above, so a hand-deleted shim leaves the hash
            // intact -- and `write_shim_if_needed` is what puts it back
            // without an extract. The loader resolves through the manifest
            // either way; this keeps the install dir self-healing exactly as
            // the always-rebuild did, minus the rebuild.
            write_shim_if_needed(&dest)?;
            return Ok(());
        }
        std::fs::remove_dir_all(&dest)?;
    }
    std::fs::create_dir_all(modules_dir)?;
    tarball::unpack(&tarball, &dest, name)?;
    // integrity: the extracted tree must hash to the lockfile checksum.
    // This is the supply-chain anchor — what you compile is what the
    // lockfile describes, regardless of how the tarball was packed.
    let actual = match crate::tarball::manifest_hash(&dest) {
        Ok(h) => h,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dest);
            return Err(e);
        }
    };
    if actual != checksum {
        let _ = std::fs::remove_dir_all(&dest);
        return Err(PkgError::Integrity {
            name: name.to_string(),
            version: version.to_string(),
        });
    }
    write_shim_if_needed(&dest)?;
    Ok(())
}

/// If the package's entry is not a root `index.ax`, generate one so the
/// compiler's `aox_modules/<name>` → `index.ax` probe reaches it.
fn write_shim_if_needed(pkg_dir: &Path) -> Result<(), PkgError> {
    let manifest = Manifest::load(&Manifest::path_for(pkg_dir))?;
    if let Some(entry) = manifest.entry_file(pkg_dir) {
        if entry == Path::new("index.ax") {
            return Ok(());
        }
        let rel = format!("/{}", entry.to_string_lossy().replace('\\', "/"));
        let text = format!("import * from \".{rel}\"\n");
        // Compare-then-write: this now runs on EVERY kept-tree install, and
        // rewriting a byte-identical shim would restamp its mtime for
        // nothing. A missing or mangled shim is written; a correct one is
        // left exactly as it is.
        let shim = pkg_dir.join("index.ax");
        if std::fs::read_to_string(&shim).ok().as_deref() != Some(text.as_str()) {
            std::fs::write(&shim, text)?;
        }
    }
    Ok(())
}

/// Relative import path from `from_dir` to `to_file`, POSIX style.
fn relative_path(from_dir: &Path, to_file: &Path) -> String {
    let from_abs = from_dir
        .canonicalize()
        .unwrap_or_else(|_| from_dir.to_path_buf());
    let to_abs = to_file
        .canonicalize()
        .unwrap_or_else(|_| to_file.to_path_buf());
    let from_parts: Vec<_> = from_abs.components().collect();
    let to_parts: Vec<_> = to_abs.components().collect();
    let mut common = 0;
    while common < from_parts.len() && common < to_parts.len() && from_parts[common] == to_parts[common]
    {
        common += 1;
    }
    let mut parts: Vec<String> = Vec::new();
    for _ in common..from_parts.len() {
        parts.push("..".into());
    }
    for c in &to_parts[common..] {
        parts.push(c.as_os_str().to_string_lossy().to_string());
    }
    let joined = parts.join("/");
    if joined.starts_with("..") {
        joined
    } else {
        format!("./{joined}")
    }
}

fn diff_removed(old: &Lockfile, new: &Lockfile) -> Vec<String> {
    old.packages
        .iter()
        .filter(|p| new.get(&p.name).is_none())
        .map(|p| format!("{}@{}", p.name, p.version))
        .collect()
}

fn report_lock_diff(ui: &Ui, old: &Lockfile, new: &Lockfile) {
    for p in &new.packages {
        match old.get(&p.name) {
            None => ui.info(&format!("+ {}@{} ({})", p.name, p.version, p.registry)),
            Some(prev) if prev.version != p.version => {
                ui.info(&format!("~ {}: {} -> {}", p.name, prev.version, p.version))
            }
            _ => {}
        }
    }
    for name in diff_removed(old, new) {
        ui.info(&format!("- {name}"));
    }
}

/// Typosquat guard: warn when a newly added package name is confusingly
/// similar to an existing registry package.
fn typosquat_check(ctx: &mut Ctx, ui: &Ui, added: &[String]) -> Result<(), PkgError> {
    if added.is_empty() {
        return Ok(());
    }
    // best effort: no registry configured → skip silently
    let Ok(url) = ctx.registry_url(None) else {
        return Ok(());
    };
    let names = match ctx.registry_for(&url) {
        Ok(r) => r.all_names().unwrap_or_default(),
        Err(_) => return Ok(()),
    };
    for a in added {
        for existing in &names {
            if existing == a {
                continue;
            }
            if edit_distance(a, existing) <= typo_threshold(a) {
                ui.warn(&format!(
                    "`{a}` looks similar to existing package `{existing}` — \
                     double-check the spelling (typosquat protection)"
                ));
            }
        }
    }
    Ok(())
}

/// Similarity threshold scaled by name length: short names are noisy at
/// distance 2 (`ax` vs `ao`), so only names of 8+ chars admit it.
fn typo_threshold(name: &str) -> usize {
    if name.chars().count() >= 8 {
        2
    } else {
        1
    }
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_distance_basics() {
        assert_eq!(edit_distance("http", "http"), 0);
        assert_eq!(edit_distance("http", "htp"), 1);
        assert_eq!(edit_distance("htpps", "http"), 2);
        assert!(edit_distance("totally-different", "http") > 2);
    }

    #[test]
    fn relative_paths() {
        let tmp = std::env::temp_dir().join(format!("aoxn-rel-{}", std::process::id()));
        let from = tmp.join("app").join("aox_modules").join("lib");
        let to = tmp.join("packages").join("lib").join("src").join("main.ax");
        std::fs::create_dir_all(&from).unwrap();
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(&to, "").unwrap();
        let rel = relative_path(&from, &to);
        assert!(rel.starts_with("../"), "got {rel}");
        assert!(rel.ends_with("main.ax"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ---- requirements fingerprint ----------------------------------------

    fn pkg(name: &str, deps: &[(&str, &str)], dev: &[(&str, &str)]) -> (PathBuf, Manifest) {
        let mut m = Manifest::synthetic();
        m.name = name.to_string();
        for (n, r) in deps {
            m.dependencies.insert(
                n.to_string(),
                DependencySpec::Registry { req: r.to_string(), registry: None },
            );
        }
        for (n, r) in dev {
            m.dev_dependencies.insert(
                n.to_string(),
                DependencySpec::Registry { req: r.to_string(), registry: None },
            );
        }
        (PathBuf::from(format!("C:/somewhere/{name}")), m)
    }

    #[test]
    fn fingerprint_is_owner_qualified_and_path_free() {
        let a = pkg("app", &[("http", "^1")], &[]);
        let b = pkg("app", &[("http", "^1")], &[]);
        let fa = requirements_fingerprint(std::slice::from_ref(&a));
        let fb = requirements_fingerprint(std::slice::from_ref(&b));
        assert_eq!(fa, fb, "same manifests must fingerprint identically");
        assert_eq!(fa.get("app/http").map(String::as_str), Some("^1@"));
        assert!(
            !fa.values().any(|v| v.contains("somewhere")),
            "the fingerprint must not embed a filesystem path: {fa:?}"
        );
    }

    #[test]
    fn two_workspace_members_can_pin_the_same_dep_differently() {
        let a = pkg("app", &[("http", "^1")], &[]);
        let b = pkg("cli", &[("http", "^2")], &[]);
        let f = requirements_fingerprint(&[a, b]);
        assert_eq!(f.len(), 2, "one entry per (owner, dep): {f:?}");
        assert_eq!(f.get("app/http").map(String::as_str), Some("^1@"));
        assert_eq!(f.get("cli/http").map(String::as_str), Some("^2@"));
    }

    #[test]
    fn dev_dependencies_participate_in_the_fingerprint() {
        let a = pkg("app", &[("http", "^1")], &[("harness", "^2")]);
        let f = requirements_fingerprint(std::slice::from_ref(&a));
        assert!(f.contains_key("app/harness"), "{f:?}");
    }

    // ---- engines ---------------------------------------------------------

    fn sel(min: Option<&str>) -> Selected {
        Selected {
            version: "1.0.0".into(),
            registry: "r".into(),
            checksum: "c".into(),
            tarball_sha256: None,
            dependencies: BTreeMap::new(),
            deprecated: None,
            yanked: false,
            min_aoxn: min.map(String::from),
        }
    }

    fn resolution_of(pairs: Vec<(&str, Option<&str>)>) -> Resolution {
        Resolution {
            packages: pairs
                .into_iter()
                .map(|(n, m)| (n.to_string(), sel(m)))
                .collect(),
        }
    }

    #[test]
    fn a_package_requiring_a_newer_compiler_is_rejected() {
        let res = resolution_of(vec![("modern", Some("999.0.0"))]);
        let err = check_engines(&res).unwrap_err();
        match err {
            PkgError::Engines { detail, .. } => {
                assert!(detail.contains("modern@1.0.0"), "{detail}");
                assert!(detail.contains("999.0.0"), "{detail}");
            }
            other => panic!("expected Engines, got {other:?}"),
        }
    }

    #[test]
    fn packages_within_the_running_compiler_pass() {
        let res = resolution_of(vec![("ok", Some(">=0.0.1")), ("unbounded", None)]);
        check_engines(&res).unwrap();
    }

    #[test]
    fn every_offender_is_reported_not_just_the_first() {
        let res = resolution_of(vec![("a", Some("999.0.0")), ("b", Some("888.0.0"))]);
        let err = check_engines(&res).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains('a') && msg.contains('b'), "{msg}");
    }

    // ---- prod / dev closure ----------------------------------------------

    fn res_with_edges(pairs: Vec<(&str, Vec<&str>)>) -> Resolution {
        Resolution {
            packages: pairs
                .into_iter()
                .map(|(n, deps)| {
                    let mut s = sel(None);
                    s.dependencies = deps
                        .into_iter()
                        .map(|d| (d.to_string(), "^1".to_string()))
                        .collect();
                    (n.to_string(), s)
                })
                .collect(),
        }
    }

    #[test]
    fn prod_closure_excludes_dev_only_subtrees() {
        let app = pkg("app", &[("http", "^1")], &[("harness", "^2")]);
        let res = res_with_edges(vec![
            ("http", vec!["zlib"]),
            ("harness", vec!["fixture"]),
            ("zlib", vec![]),
            ("fixture", vec![]),
        ]);
        let prod = prod_closure(std::slice::from_ref(&app), &res, &[]);
        assert!(prod.contains("http") && prod.contains("zlib"));
        assert!(!prod.contains("harness"), "harness is dev-only");
        assert!(
            !prod.contains("fixture"),
            "fixture is reachable only through the dev tool"
        );
    }

    #[test]
    fn a_package_used_by_both_scopes_counts_as_prod() {
        let app = pkg("app", &[("http", "^1")], &[("http", "^1")]);
        let res = res_with_edges(vec![("http", vec![])]);
        let prod = prod_closure(std::slice::from_ref(&app), &res, &[]);
        assert!(prod.contains("http"), "a prod dependency is never dev-only");
    }

    #[test]
    fn prod_closure_terminates_on_a_cycle() {
        let app = pkg("app", &[("a", "^1")], &[]);
        let res = res_with_edges(vec![("a", vec!["b"]), ("b", vec!["a"])]);
        let prod = prod_closure(std::slice::from_ref(&app), &res, &[]);
        assert_eq!(prod.len(), 2);
    }

    #[test]
    fn overrides_are_unioned_across_workspace_members() {
        let mut a = pkg("app", &[], &[]).1;
        a.overrides.insert("http".into(), "1.4.2".into());
        let mut b = pkg("cli", &[], &[]).1;
        b.overrides.insert("zlib".into(), "1.2.0".into());
        let ov = collect_overrides(&[(PathBuf::new(), a), (PathBuf::new(), b)]);
        assert_eq!(ov.get("http").map(String::as_str), Some("1.4.2"));
        assert_eq!(ov.get("zlib").map(String::as_str), Some("1.2.0"));
    }

    #[test]
    fn report_serializes_for_json_consumers() {
        let r = InstallReport {
            resolved: 3,
            installed: vec!["http".into()],
            removed: vec![],
            reused_cache: 2,
            downloaded: 1,
            skipped_dev: vec!["harness".into()],
        };
        let v = r.to_json();
        assert_eq!(v["resolved"], 3);
        assert_eq!(v["downloaded"], 1);
        assert_eq!(v["skipped_dev"][0], "harness");
    }

    // ---- install_registry_dep: keep-or-rebuild ----------------------------

    const SRC_AX: &str = "def util_f() -> int:\n    return 7\n";

    /// A real package the fixture packs into a real tarball. `main` keys util.ax,
    /// so the loader shim (index.ax) IS involved: every preserve-verdict below
    /// must survive our own adding of that file.
    fn pkg_src(parent: &Path) -> Result<PathBuf, PkgError> {
        let src = parent.join("src");
        std::fs::create_dir_all(&src)?;
        std::fs::write(
            src.join("aoxn.json"),
            r#"{ "name": "util", "version": "1.0.0", "main": "util.ax" }"#,
        )?;
        std::fs::write(src.join("util.ax"), SRC_AX)?;
        Ok(src)
    }

    /// Pack + hash + cache: the exact shape `install_registry_dep` consumes.
    fn cached_package(parent: &Path, cache: &Cache) -> Result<String, PkgError> {
        let src = pkg_src(parent)?;
        let tgz = parent.join("util.tar.gz");
        tarball::pack(&src, "util", &tgz)?;
        let ck = tarball::manifest_hash(&src)?;
        cache.store_tar(&tgz, &ck)?;
        Ok(ck)
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "aoxn-keep-{}-{tag}-{nanos}",
            std::process::id()
        ))
    }

    /// (rel path, mtime ms) for every file under dir. The mtimes are the
    /// removal marker: a rebuilt tree has fresh mtimes; a skipped tree has
    /// exactly the ones it entered with.
    fn tree_state(dir: &Path) -> BTreeMap<String, u128> {
        let mut out = BTreeMap::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap_or_else(|e| panic!("read {}: {e}", d.display())) {
                let e = e.unwrap();
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    let m = std::fs::metadata(&p)
                        .unwrap()
                        .modified()
                        .unwrap()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or(0);
                    let k = p
                        .strip_prefix(dir)
                        .unwrap()
                        .to_string_lossy()
                        .to_string()
                        .replace('\\', "/");
                    out.insert(k, m);
                }
            }
        }
        out
    }

    #[test]
    fn a_correct_install_is_not_rebuilt_the_second_time() {
        let tmp = temp_dir("warm");
        let cache = Cache { root: tmp.join("cache") };
        cache.create_dirs().unwrap();
        let ck = cached_package(&tmp, &cache).unwrap();
        let modules = tmp.join("modules");

        install_registry_dep(&cache, &modules, "util", "1.0.0", &ck).unwrap();
        let before = tree_state(&modules);
        std::thread::sleep(std::time::Duration::from_millis(120));
        install_registry_dep(&cache, &modules, "util", "1.0.0", &ck).unwrap();
        let after = tree_state(&modules);

        assert_eq!(before, after, "a warm install must not rewrite a correct tree");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn a_hand_edited_installed_file_is_restored_not_trusted() {
        // The preserve fast path must not become a trust fast path: an
        // edited file mismatches the checksum, and the verify is the same
        // tree read that install has always done. This is the pin for the
        // security property the always-rebuild used to provide.
        let tmp = temp_dir("edit");
        let cache = Cache { root: tmp.join("cache") };
        cache.create_dirs().unwrap();
        let ck = cached_package(&tmp, &cache).unwrap();
        let modules = tmp.join("modules");

        install_registry_dep(&cache, &modules, "util", "1.0.0", &ck).unwrap();
        std::fs::write(modules.join("util").join("util.ax"), "def hacked() -> int:\n    return 0\n").unwrap();
        install_registry_dep(&cache, &modules, "util", "1.0.0", &ck).unwrap();

        let restored = std::fs::read_to_string(modules.join("util").join("util.ax")).unwrap();
        assert_eq!(restored, SRC_AX, "the tampered file must be put back");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn a_deleted_shim_is_healed_without_a_rebuild() {
        // `index.ax` is EXCLUDED from the warm compare (it is our shim, not
        // the release's content), so deleting it leaves the hash intact.
        // The keep path regenerates the shim from the manifest; the rest of
        // the tree must not have been touched, which the mtimes prove.
        let tmp = temp_dir("entry");
        let cache = Cache { root: tmp.join("cache") };
        cache.create_dirs().unwrap();
        let ck = cached_package(&tmp, &cache).unwrap();
        let modules = tmp.join("modules");

        install_registry_dep(&cache, &modules, "util", "1.0.0", &ck).unwrap();
        let shim = modules.join("util").join("index.ax");
        assert!(shim.exists(), "main=util.ax must have produced the shim");
        let before = tree_state(&modules);
        // simulate the damage: the shim disappears entirely
        std::fs::remove_file(&shim).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(120));
        install_registry_dep(&cache, &modules, "util", "1.0.0", &ck).unwrap();

        assert!(shim.exists(), "a deleted shim must be regenerated");
        let body = std::fs::read_to_string(&shim).unwrap();
        assert!(body.contains("util.ax"), "the shim re-derives the entry: {body}");
        let healed = tree_state(&modules);
        // every file EXCEPT the shim keeps its mtime: no rebuild happened
        let untouched = healed.iter().all(|(k, v)| {
            k == "util/index.ax" || before.get(k) == Some(v)
        });
        assert!(untouched, "the heal must not rebuild the tree: {before:?} -> {healed:?}");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn dot_entries_are_tool_state_and_survive_the_prune() {
        // Package names admit a-z / 0-9 / `-` / `_` only, so a dot entry in
        // aox_modules/ can never be a package. It belongs to tools and is
        // left alone; a plain (non-dot) stale entry keeps being swept.
        let tmp = temp_dir("prune");
        let cache = Cache { root: tmp.join("cache") };
        cache.create_dirs().unwrap();
        let ck = cached_package(&tmp, &cache).unwrap();
        // materialize joins `dir/aox_modules` itself; the planted state goes
        // THERE so the sweep actually sees it.
        let modules = tmp.join("aox_modules");
        std::fs::create_dir_all(&modules).unwrap();
        std::fs::create_dir_all(modules.join("gone")).unwrap();
        std::fs::write(modules.join("gone").join("leftover.ax"), "stale").unwrap();
        std::fs::write(modules.join(".ide-marker"), "tool state").unwrap();

        let mut m = Manifest::synthetic();
        m.dependencies.insert(
            "util".into(),
            DependencySpec::Registry { req: "^1".into(), registry: None },
        );
        let mut sel = sel(None);
        sel.checksum = ck.clone();
        let res = Resolution {
            packages: [("util".to_string(), sel)].into_iter().collect(),
        };
        // the lock gates what materialize installs: without the util entry,
        // the dep is silently left alone (`lock.get(dep)` is the gate)
        let lock = Lockfile {
            requirements: BTreeMap::new(),
            packages: vec![LockedPackage {
                name: "util".into(),
                version: "1.0.0".into(),
                registry: "r".into(),
                checksum: ck,
                tarball_sha256: None,
                dev: false,
                dependencies: vec![],
            }],
            ..Lockfile::empty()
        };

        let (installed, removed, _) = materialize(
            &tmp, &m, &res, &lock, &[], &cache, DepScope::All,
            &crate::ui::plain(),
        )
        .unwrap();

        assert_eq!(installed, vec!["util"], "the dep installs");
        assert!(modules.join("util").exists());
        assert!(
            modules.join(".ide-marker").exists(),
            "dot state must survive the sweep"
        );
        assert_eq!(removed, vec!["gone"], "only real stale names are swept");
        assert!(!modules.join("gone").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
