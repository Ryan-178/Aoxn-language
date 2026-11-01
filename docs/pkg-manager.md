# Aoxn Package Manager (`aoxn pkg`)

*Status: beta. Ships since v0.29.0; manifest entry resolution since v0.29.1;
read-only HTTP registry backend since v0.29.4; npm import bridge since
v0.29.5; dev dependencies, overrides, curated registries, `list`/`freeze`,
parallel downloads and compiler-version enforcement since v0.32.0.*

The package manager gives Aoxn **independent** infrastructure (the W2
goal): its own manifest, its own lockfile, its own registry protocol —
with npm as a *bridge target*, not a foundation. The crate lives in
[`crates/aoxn-pkg`](../crates/aoxn-pkg) and keeps a deliberately
build-script-free dependency set (clap, serde, semver, pubgrub, sha2,
flate2+tar, dirs, thiserror).

## Quick start

```bash
aoxn init                 # aoxn.json + hello world
aoxn add http@^2          # add a registry dependency, resolve + install
aoxn add -D harness       # add tooling that must not ship
aoxn tree                 # what is installed, and why
aoxn list                 # what is installed, as a table
aoxn freeze               # as `name==version` lines (pip freeze)
aoxn npm-import left-pad  # bridge: import an npm-hosted Aoxn package
aoxn publish              # ship this package to the registry
```

Every `pkg` subcommand is also a direct alias of `aoxn` (`aoxn install`,
`aoxn outdated`, `aoxn trust`, …).

## The three artifacts

| artifact | role |
|---|---|
| `aoxn.json` | the manifest: identity, entry points, dependencies, overrides, registries |
| `aoxn.lock` | the lockfile: resolved versions + manifest hashes + prod/dev split, shared across the workspace |
| `aox_modules/<name>/` | the *fully managed* install dir: recreated on every install, never edited by hand |

### Manifest (`aoxn.json`)

```json
{
  "name": "my-app",
  "version": "0.1.0",
  "main": "src/main.ax",
  "exports": { ".": "src/lib.ax", "./client": "src/client.ax" },
  "types": "src/types.ax",
  "dependencies": {
    "http": "^1.2",
    "json": { "version": "^1", "registry": "main" },
    "my-utils": { "path": "../my-utils" }
  },
  "devDependencies": { "test-harness": "^2" },
  "overrides": { "http": "1.4.2" },
  "registries": {
    "default": { "url": "https://github.com/you/aoxn-registry" },
    "mirror":  { "url": "http://mirror.lan:8080/aoxn", "kind": "http" }
  },
  "workspace": { "members": ["packages/*"] }
}
```

- `main` / `exports` / `types` (v0.29.1): the compiler resolves bare
  package imports (`import * from "http"`, `import * from "http/client"`)
  against the installed package's `aoxn.json` — `exports["."]` is the root
  entry, `exports["./client"]` the `http/client` subpath. Packages without
  a manifest fall back to the legacy `index.ax` probe. The reader is
  `src/pkg_manifest.rs`, a zero-dependency JSON parser (the compiler's
  `src/` crate stays free of external crates).
- dependencies are a semver requirement string (registry dep against the
  default registry) or an object with `version` + `registry` override, or
  a `path` (local/workspace member — resolved live, no registry).
- `devDependencies` (v0.32.0): tooling that must not reach a production
  install. `aoxn add -D <pkg>`.
- `overrides` (v0.32.0): force a requirement on a package **wherever it
  appears in the graph** — pnpm's `overrides` / pip's constraints file in
  one line. A bare version is a caret range (`"1.4.2"` means `^1.4.2`);
  write `"=1.4.2"` to pin exactly.
- `registries` entries: a git URL (default), a directory (`kind: "dir"`),
  or an HTTP mirror (`http://` URL, `kind: "http"` optional). Priority for
  un-hinted deps: `default` key → first declared → `$AOXN_REGISTRY` →
  global config.

### Lockfile (`aoxn.lock`)

Records every resolved package's version, manifest hash, source registry
and whether it is dev-only, plus a requirements fingerprint of the
manifests. `install` skips re-resolution when the fingerprint matches
(works offline); `--frozen` (CI) fails instead of re-resolving. One
lockfile covers the whole workspace.

The fingerprint is keyed `"<owner-pkg>/<dep>" -> "<req>@<registry>"`. Two
details matter:

- **the owner prefix** — two workspace members may pin the same dependency
  at different ranges, and a bare dependency-name key let the second
  overwrite the first;
- **no filesystem path** — the previous format embedded the declaring
  manifest's absolute directory, so moving or renaming the project forced
  a re-resolve every time.

`lockfile_version` stays **1**: every field added since is optional with a
serde default, so a lockfile written by an older aoxn still loads. Its
packages simply read as not-dev-only. The first install after upgrading
re-resolves once, because the fingerprint's *shape* changed.

### `aox_modules/` materialization

- registry deps: the tarball is extracted to `aox_modules/<name>/`; if the
  entry is not a root-level `index.ax`, a one-line shim `index.ax`
  (`import * from "./<entry>"`) is generated — the loader merges names
  transitively, so the shim is transparent.
- path deps: `aox_modules/<name>/` holds only a shim pointing at the real
  source — edits are live.
- everything in `aox_modules/` is managed; stale entries are pruned on
  every install. Do not commit it; do not edit it.

## Dev vs production dependencies

Dev and prod live in **one** lockfile. The split is a `dev` flag per
package, computed as "not reachable from any production root" by walking
the dependency graph; `--prod` simply materializes the other half.

```bash
aoxn add -D test-harness       # record tooling as a devDependency
aoxn install                   # everything (default)
aoxn install --prod            # runtime only; dev-only packages are pruned
aoxn install --dev-only        # just the dev tooling
aoxn update -D test-harness    # update a devDependency
aoxn list --prod               # table, production packages only
aoxn freeze --prod             # `name==version`, production only
```

A package named in **both** tables counts as production — the prod
declaration wins, so `--prod` never breaks a build that genuinely needs it.
One lockfile covering both scopes (rather than pip's two requirement files
or pnpm's second lockfile) means a CI job that only ships runtime code
still reproduces from a lockfile that also pins the test tooling.

## Resolution

Single-stage [PubGrub](https://github.com/pubgrub-rs/pubgrub) over the
whole dependency graph with **soft preference**: a locked version is
preferred, otherwise the newest non-yanked version. Conflicts print a full
derivation with `aoxn <cmd> --explain`. Manifest `overrides` replace every
requirement the graph places on that package. Newly added packages run
through a **typosquat guard**: a name close to an existing registry package
requires explicit confirmation.

## Registries

All three backends serve the same tree layout:

```text
packages/<name>/index.json       # PackageIndex: version -> {checksum, deps, yanked, …}
packages/<name>/<version>.tar.gz
packages/names.json              # full name list (HTTP backend only; feeds the typosquat guard)
```

| backend | read | write | notes |
|---|---|---|---|
| **git** (default) | shallow clone into the global cache | commit + tag `pkg/<name>/v<ver>` + push (retry loop for concurrent publishes) | zero infrastructure; offline installs read the last snapshot |
| **dir** | direct file access | direct file access | air-gapped CI, tests |
| **http** (v0.29.4) | plain HTTP/1.1 GET | **read-only** | a static file server or mirror is enough; publish through the git/dir backend that owns the tree |

Publishing is idempotent (same name+version+checksum ⇒ success) and
versions must be strictly increasing per package. `aoxn yank <pkg> <ver>`
marks a version unresolvable for *fresh* resolution; existing lockfiles
keep working. A publish also records the publishing compiler as the
package's **minimum** version (`IndexVersion.aoxn`), which `install` now
enforces — a package needing language or stdlib features your compiler
lacks is refused up front instead of failing mid-compile.

### Two digests, two jobs (v0.32.0)

The index carries both, and they are different quantities:

| field | what it hashes | who checks it | when |
|---|---|---|---|
| `checksum` | the **manifest hash** over the unpacked file tree | install | after extraction |
| `tarball_sha256` | the **tarball bytes** as served | the HTTP backend | at download |

Until v0.32.0 the HTTP backend compared `sha256(downloaded bytes)` against
`checksum` — two unrelated digests — so *every* HTTP download failed with a
bogus integrity error. The transport digest is optional (registries
published before v0.32.0 carry none): when it is absent the wire check is
skipped rather than failing a good download, and integrity is not
weakened, because the post-extraction manifest-hash check always runs.

### The HTTP backend's constraints (by design)

The client is hand-rolled on `std::net::TcpStream` because the crate's
dependency set must stay free of build scripts — which rules out every
TLS-capable HTTP crate. Consequences: only `http://` URLs are accepted
(`https://` fails early with an explanation; put a local reverse proxy in
front of a remote registry), requests use `Connection: close`, and both
`Content-Length` and chunked bodies are supported.

## Curated registries: trust + advisories

A registry may also carry `trust.json` (a review record per package) and
`advisories/*.json` (the vulnerability database `aoxn audit` checks
against). One repository, three jobs, one command to wire up:

```bash
aoxn trust bootstrap https://github.com/AlonechatWorkspace/Aoxn-trusted-third-party-package
aoxn trust list
aoxn trust check http --tier audited   # CI gate, exit 1 when unreviewed
aoxn audit --audit-level high --json
```

Install warns about packages a curated registry has no reviewed record
for; a registry with no `trust.json` makes no claims and stays silent.
`aoxn audit` falls back to the default registry's `advisories/` when no
advisory source is configured, and `--fix` raises manifest constraints to a
patched version — but only where that *tightens* a constraint that already
admits it, never by widening a range someone wrote on purpose.

Full layout and schema: [`docs/trusted-registry.md`](trusted-registry.md).

## Inspecting what is installed

```bash
aoxn list               # Package / Version / Scope / Source table
aoxn list --json
aoxn tree --depth 2     # shape: who depends on whom
aoxn why http           # every requirement path to a package
aoxn outdated           # newer versions available
aoxn freeze             # `name==version`, sorted — CI baselines
```

`list` reports what is installed, `tree` shows shape, `why` explains a
choice, `outdated` compares against the registry, `freeze` emits the
machine-diffable form.

## Concurrency

Tarballs download in parallel, `jobs` at a time per registry:

```bash
aoxn install --jobs 16
AOXN_JOBS=4 aoxn install
aoxn install --jobs 1      # strictly serial
```

Default 8. Each worker gets its own forked registry handle (the backends
carry mutable state and are not shared across threads), and progress is
reported once per fetch phase rather than per package — the per-package
step line rewrites one terminal row, which several threads would fight
over.

## npm bridge (`aoxn npm-import`)

A **one-shot import tool**, not a proxy (decided 2026-10-02): the npm CLI
is the transport — `npm view` for metadata, `npm pack` for the tarball —
so auth, https and private registries come from the user's `.npmrc` for
free, and this crate never needs TLS.

```bash
aoxn npm-import my-pkg@1.2.0         # one package
aoxn npm-import --from package.json  # every dep of an npm package.json
```

Only **Aoxn packages** can be imported: the npm tarball must contain an
`aoxn.json` in its package root. Plain JavaScript packages are rejected
explicitly (Aoxn cannot link JavaScript). An imported package lands in
`vendor/<name>/` and is recorded in `aoxn.json` as a path dependency
(`{"path": "vendor/<name>"}`) — reusing the entire install pipeline with
zero new concepts. `vendor/` is excluded from packing. The npm tarball is
verified against `dist.integrity` (sha512) before it is unpacked.
`AOXN_NPM` overrides the npm binary (default: `npm.cmd` on Windows).

## Cache (`aoxn cache`)

Content-addressed under `$AOXN_HOME` (default `~/.aoxn`):
`tarballs/<manifest-hash>.tar.gz` and per-registry snapshots
(`registries/<sha8(url)>/`). Safe to share read-only between CI workers.
`aoxn cache gc` drops unreferenced tarballs (TTL-protected), `prune` drops
registry snapshots, `clean` deletes everything.

## Security model

- **Integrity = manifest hash**, not tarball bytes:
  `sha256(Σ "<rel>\0<sha256(file)>\n" over sorted files)`, computed at
  publish, pinned in `aoxn.lock`, re-verified over the extracted tree at
  install — the bytes you run are exactly what the checksum describes,
  immune to tar/gzip quirks and re-packing.
- **Transport digest** (`tarball_sha256`, v0.32.0) additionally lets the
  HTTP backend reject a corrupted transfer before it reaches the cache.
- tarball extraction refuses any entry that would write outside its destination:
  `..` traversal, a remainder that is absolute **on this host** (`C:/x` on
  Windows, `/x` on POSIX — the guard follows the host because `Path::join` does),
  and every hard link or symlink. A refused archive writes nothing at all: the
  checks run before any directory or file is created (supply-chain hard stop).
- typosquat guard on newly added packages.
- `aoxn audit` checks the lockfile against an advisory database — the
  registry's own `advisories/`, or one configured globally.
- `aoxn trust` surfaces a curated registry's review records; it never
  blocks an install on its own.
- declared minimum compiler versions are enforced at install.
- yanked versions never enter fresh resolution; lockfiles pinning them
  keep working (migration, not breakage).

## Testing

`bash run_pkg_tests.sh` (94 tests; it exists because Windows Smart App
Control blocks freshly built unsigned test binaries after their first
runs — the script bumps a marker so each build gets a fresh hash). The
HTTP backend is tested against an in-process `TcpListener` static server,
including both transport-digest paths — verified when the index publishes
one, skipped when it does not. The npm bridge is tested against
npm-layout tarballs packed in-process (no network).

## Known gaps

Deliberately not implemented yet: signatures and provenance attestations
(`IndexVersion.attestations` is reserved), peer dependencies,
optional/feature-gated dependencies, two versions of one package in a
graph, and `workspace:*` protocol specs (use `{"path": "../member"}`).
