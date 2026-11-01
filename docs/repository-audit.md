# Repository artifact and documentation audit (P2)

Audit date: 2026-11-01. Bench: `D:/ailanguage-bench/c-parity-abi`,
base HEAD `8f90aca`. This is an evidence inventory, not a cleanup operation.
Existing concurrent source/document edits were left alone. This artifact-audit
subtask added only this report, `tools/check_doc_links.py`, and
`tools/test_check_doc_links.py`. It did not change CI, README or ignore rules.
The overall P2 work includes separate CI/documentation changes by the main
agent; the three-file restriction applies to this subtask, not the whole P2.
No historical user files were deleted, moved, regenerated or executed by this
subtask. No commits, pushes or network fetches were performed by this subtask.

## Evidence method and scope

Used `git ls-files`, `git log --all --diff-filter=A --name-only`, per-path
`git log --all`, `git check-ignore -v --no-index`, and case-insensitive
`git grep -I` over tracked text. Supplemented with a read-only UTF-8 text
reference scan over the working tree (including untracked text), pruning
`.git`, `target`, `node_modules`, `wiki`, `LLVM`, `.zcode` and
`third-party-package-in-the-making`; symlinks were not followed. Binary files
were not decoded as text. The new P2 files were excluded from the artifact
reference search to avoid counting this inventory as an earlier consumer.
ZIPs were inspected in memory without extracting. Git provenance means the
first addition visible in local history, not proof of who originally authored
or downloaded an artifact. No remote history was fetched. Ignored/untracked
files in a different worktree are not present automatically in this bench.

## Artifact inventory

| Path | Tracked / ignored / present in this bench | Local provenance and actual references | Disposition |
| --- | --- | --- | --- |
| `axon_stdlib_test.txt` | Tracked, not ignored by current rules, present (15 bytes) | Added `9a4f658` (2026-09-06), v0.9 raw memory/stdlib change. Contents `hello from axon`. No exact-name text consumer found. `tests/pipeline.rs` instead writes `AOXN_stdlib_test.txt`, which `.gitignore` explicitly ignores. `axon` versus `AOXN` is a spelling difference, not just case; do not conflate the two. | Preserve as historical tracked test output; not an active fixture proved by references. |
| `clang_shim.obj` | Not tracked, matches `.gitignore` rule `*.obj`, absent | No addition or other per-path history in local `--all`; no text reference to this name or `clang_shim` found. Object suffix is consistent with a build product, but producer/owner cannot be established from absent bytes/history. | No action. Ignore status does not authorize deleting another worktree's file. |
| `tmp_b1convert.py` | Tracked, not ignored, present | Added `45c07b5` (2026-09-27), full-tree commit/platform measurements. Source rewrites `Diag { ... }` constructions in `src/typecheck.rs` and `src/parser.rs` into helpers. No caller/reference to the script name found. | Historical one-shot migration helper, **not** a read-only utility; inspected but never run. Preserve. |
| `test/web-bench-macos-14.zip` | Tracked, not ignored, present | Added `54cb305` (2026-09-27), bilingual wiki commit. One entry: `last-results.json` (4,924 bytes). No archive-name consumer found. | Preserve historical benchmark archive, not evidence of current macOS support. |
| `test/web-bench-ubuntu-latest.zip` | Tracked, not ignored, present | Same addition; one `last-results.json` (4,965 bytes). No archive-name consumer found. | Preserve. |
| `test/web-bench-windows-latest(1).zip` | Tracked, not ignored, present | Same addition; one `last-results.json` (4,968 bytes). No archive-name consumer found. | Preserve filename and contents. |
| `web/loadtest/last-results.json` | Tracked, not ignored, present | Added `2fb62eb` (2026-09-27), HTTP server/benchmark suite. `web/loadtest/bench.mjs` writes it; `.github/workflows/web-bench.yml` uploads it; `web/README.md`, `docs/web-benchmark.md`, and `AGENTS.md` reference raw results. Snapshot: Aoxn v0.26.3 / Node 22 / Next 15, 10s, 32 connections, 3 trials. | Keep raw measurement evidence; a later benchmark can overwrite this named output, so it is not an immutable/latest-version guarantee. |
| `web/loadtest/ci-results/{macos-14,ubuntu-latest,windows-latest}.json` | All three tracked, not ignored, present | Added `54cb305`. `docs/web-benchmark.md` links the directory in two places. Snapshots: same version labels, 5s, 32 connections, 1 trial. Each JSON parses equal to the corresponding ZIP entry; byte strings are **not** equal, so only semantic duplication is established. | Retain alongside archives; not fresh cross-platform certification. |
| `tmp_time.txt`, `tmp_time_base.txt` | Both tracked, not ignored, present | Added `45c07b5`; no exact-name tracked consumer found. First contains phase timings for `selfhost/driver_self_demo.ax` and old `D:/ailanguage` source paths; second is empty. Exact invocation/toolchain/environment not recorded. | Preserve raw timing evidence; insufficient for a reproducible performance claim. |

The seven non-build working-tree reference lines for the requested names/raw
result locations agree with the tracked scan: `AGENTS.md` (one), workflow
(one), web benchmark doc (three), web README (one), benchmark producer (one).
The separate current `AOXN_stdlib_test.txt` producer/ignore rule is documented
above; it is not a reference to the differently spelled historical file.
Frozen wiki was excluded from this live-reference inventory deliberately.

## Read-only Markdown checker

The commands below assume the repository root as the working directory.
The checker itself also works from other directories when invoked by its
absolute path (its default scan root is the script's repository).

```sh
python -B tools/check_doc_links.py
python -B tools/check_doc_links.py --strict
python -B -m unittest discover -s tools -p test_check_doc_links.py -v
```

`--root PATH` selects another repository. Python standard library and local
Git are the only requirements. The checker reads Markdown and local deletion
history, prints findings, and never writes files, follows links over the
network, executes documented commands, or invokes a build. Use `-B` as above
for tests as well, to avoid interpreter bytecode artifacts.

### Scope and exclusions

- Traversal is over working-tree `.md` files, **including untracked current
  documents**, not just `git ls-files`. Directory pruning happens before
  descent: `.git`, `target`, `node_modules`, `wiki`, `__pycache__`, `.zcode`,
  `LLVM`, `third-party-package-in-the-making`. Symlinks are not followed.
  Other vendor trees are not implicitly excluded; this is intentionally
  disclosed rather than claiming tracked-only coverage.
- Exact historical-document allowlist: `CHANGELOG.md`, `docs/selfhost.md`,
  `docs/optimization-report.md`, `docs/llvm-independence-report.md`. These
  preserve release history, v0.19 assessment, LLVM-era measurements, and the
  migration narrative respectively. **Whole documents are skipped, including
  their current-status prefaces**; this is an explicit coverage limitation,
  not proof those prefaces have valid links. No whole `docs/` exemption.
- Inline Markdown links/images and reference definitions/defined references
  are checked relative to their containing document. URL escapes, angle
  destinations, one-level balanced destination parentheses, titles, query
  strings and fragment suffixes are handled. Existing directories count.
  Fragment anchors themselves are not verified. External schemes, protocol-
  relative URLs, absolute paths and targets outside the root are skipped.
- Code fences, indented code, inline code and HTML comments are not interpreted
  as Markdown links. Bare `import * from "stdlib/time"` is not a link, and no
  generic quoted-string/filesystem guessing is performed. An explicitly
  authored Markdown link to an extensionless target is still a real link.
- A separate conservative regex inspects command-looking lines (including
  code) for selected source/script filename extensions. Root-relative **or
  document-directory-relative** existing files are accepted. It does not
  interpret shell `cd`, substitutions, outputs, options, package scripts or
  arbitrary build tools. Placeholders such as `file.ax` and prose on a line
  containing `cargo run` can be reported. These are **manual-review hints**,
  never confirmed invalid commands. No command is executed.
- This is a small Markdown subset checker, not a full CommonMark/HTML parser;
  raw HTML links, undefined reference labels, deeply nested destinations and
  arbitrary escaped-label syntax are outside its guarantee.

### Classification and exits

| Output category | Meaning |
| --- | --- |
| `missing-path` | Actual relative Markdown destination does not exist and no deletion of that root-relative path is visible in local Git history. |
| `historical-path` | Missing destination (or command token) matches a path deleted somewhere in local `git log --all`. This is historical evidence, not permission to restore it and not proof the document intentionally describes history. |
| `command-drift` | Selected file token on a command-looking line is missing at both candidate locations. Suspected drift or placeholder/context mismatch only. |

Default is warning-only: findings still exit **0**. `--strict` exits **1**
for any finding in any category, including hints/historical paths. A root,
Git-history, read or decoding error exits **2** rather than claiming a clean
scan. Output always gives category counts, scan/exclusion totals and policy.

### Validation

The temporary-fixture unittest suite has 11 tests: live/missing/deleted paths,
local Git deletion discovery, URL/absolute/outside exclusions, encoded names,
directories and fragments, code/comments/import exclusions, explicit
extensionless links, images/titles/parentheses, references, root/document
command context, pruning/exact allowlist and default/strict/error exits.
Tests create and remove their own temporary repository only.

Validation snapshots (counts depend on which concurrent documents are present):

- Initial P2 run before this report: 25 scanned, 4 allowlisted, 0 missing
  paths, 0 historical paths, 23 command hints. Accepting existing
  document-local command paths removed the `web/README.md` `server_win.ax`
  false positive.
- The main agent independently confirmed **11/11 unittest passes** and a
  **25 scanned / 4 allowlisted / 0 missing / 0 historical / 22 command hints**
  snapshot before the final report and README audit link were incorporated.
- P2's subsequent run with this report present: **26 scanned / 4 allowlisted /
  0 missing / 0 historical / 23 command hints**; warning-only exit **0**,
  strict exit **1**, **11/11 tests passed**. The additional hint is this
  report's unittest discovery `-p` filename pattern, not a missing input path.
  The other 22 remain manual-review candidates, not confirmed defects.

These are worktree observations, not a promise of a fixed count across future
edits. After the README audit link and reports were incorporated, the main
agent confirmed the final scan: **26 scanned / 4 allowlisted / 0 missing /
0 historical / 23 command hints**, with **11/11 fixture tests passing**.
No live documentation was rewritten merely to suppress findings.

### Overall P2 validation (main-agent evidence)

The main agent independently confirmed the following Windows/local results:

- Explicit locked root-crate test run passed **349 tests**.
- Locked package suite passed **104 tests**.
- Python checker fixture suite passed **11 tests**.
- YAML and PowerShell syntax checks passed; YAML was independently rechecked
  with BaseLoader.
- The Linux locked workspace/tests cross-check passed. This was a cross-check,
  not a native Linux runtime test run.
- No remote operations or release packaging were performed for this validation.

The root command was `cargo test -p aoxn --locked`. Compiler/package and
syntax results are attributed to the main agent, not to an artifact-audit
rerun. This subtask only reran the lightweight checker and its fixture tests;
it did not repeat compiler/package builds or heavy test suites.
