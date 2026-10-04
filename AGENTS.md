# AGENTS.md — Aoxn compiler

Aoxn: AI-native compiled language with Python-style syntax (indentation
blocks, `def`, `elif`, `#` comments). Programs lower to ISO C
(`src/codegen_c.rs`) and clang compiles/links them to native code — measured
at parity with `clang -O3`. **v0.29.0 (2026-10-01): the LLVM dependency is
gone**; the C-emitting backend is the only backend (history:
`docs/llvm-independence-report.md`, `CHANGELOG.md`). **v0.30.0: Windows is
the only supported platform** — one installer, one CI job, one UI backend;
macOS/Linux support (the X11 UI backend, the POSIX web socket/server
modules, the POSIX link flags) was removed, not deprecated. This repo IS the
compiler (Rust workspace: root `aoxn` crate + `crates/aoxn-pkg`).
Language sources use the `.ax` extension. Language rules live in
`docs/spec.md` (Windows-only: `docs/platform-support.md`) — keep it in sync with `src/parser.rs` + `src/typecheck.rs`
when the grammar changes. Update `CHANGELOG.md` on every version bump.
**The wiki (`wiki/`) is FROZEN since v0.29.3 (user instruction)** — do not
update wiki pages anymore; `docs/` is the living documentation and stays in
sync. AGENTS.md is a published repo file (tracked since v0.29.3) — update it
in the session bench like any other file.

## Commit policy (user instruction, standing)

**每对话独立工作树 / per-session worktree bench (v2, 2026-09-29 起)**:
every conversation works in its OWN git worktree ("bench") created at the
start of the session. Never commit session work from the shared main
worktree — parallel sessions must not be able to sweep each other's
half-finished files into their commits.

Session start:
1. Create the bench: `git worktree add D:/ailanguage-bench/<slug> -b <slug>`
   where `<slug>` is a short session topic (e.g. `ui`, `ts-m1`; append
   `-2`, `-3` on name collision). The bench is a SIBLING directory of the
   repo (`D:\ailanguage-bench\`), shares the same `.git`, and starts from
   the current `main`. NOTE: in Git Bash pass the path with FORWARD SLASHES
   (`D:/ailanguage-bench/<slug>`) — backslashes get eaten and the worktree
   lands in a mangled path.
2. Do ALL file work inside the bench directory; the main worktree
   (`D:\ailanguage`) stays untouched by the session.

**全量提交 / full commits**: inside the bench, every commit must include
the whole bench working tree — use `git add -A` (not `git add <file>`
cherry-picking, not `git add .`), then `git commit`, then verify with
`git status` that nothing is left behind.

- Applies to all work: source, tests, docs, selfhost Aoxn sources, CI config,
  AGENTS.md updates, and any new files created during the session.
- Before committing, delete throwaway probe artifacts you created (scratch
  `.ps1`/`.ax`/log files) so they do not land in history; do not delete files
  authored by the user.

Session end (merge back + push, 合并后统一推送 — main stays current):
1. In the MAIN worktree require a clean `git status` (tracked files). If
   another session holds uncommitted work there, STOP and let the user
   coordinate — never merge on top of a dirty main.
2. `git merge --ff-only <slug>` in the main worktree (if it cannot
   fast-forward: `git rebase main` in the bench, re-run tests there,
   retry). Then `git push`, keeping
   `github.com/AlonechatWorkspace/Aoxn-language` in sync (remote moved
   from `Ryan-178` on 2026-10-01).
3. Clean up: `git worktree remove D:/ailanguage-bench/<slug>` and
   `git branch -d <slug>`. A session paused mid-work keeps its bench and
   branch until the work lands.

**README/SECURITY 重写 / README & SECURITY rewrite (user instruction,
standing)**: after every conversation that changes the project, REWRITE
`README.md` and `SECURITY.md` against the new reality — same commit as the
work, before the session-end merge. Do not patch them incrementally;
regenerate the status/version/test-count sections from the actual tree
(CHANGELOG top entry, `cargo test` totals, current commands) so they can
never drift half a version behind (the old "v0.7 · 70/70 tests" README is
the cautionary tale). Both files are bilingual (English section first,
中文 section second) — update BOTH halves.

**Wiki 冻结 / wiki frozen (user instruction, standing, 2026-10-01 起)**:
每次项目结束后**不再修改 `wiki/`** — do NOT touch `wiki/` pages at the end
of (or during) a session anymore. The old wiki-sync rule (update affected
pages in the same commit) is RETIRED; the wiki stays as it is and may
describe older behavior. `docs/` remains living documentation: keep
`docs/spec.md` in sync with the grammar and update the topical docs
(`docs/ui.md`, `docs/ts-m1-spec.md`, …) as before — just never `wiki/`.

## Commands

```powershell
cargo build                                   # build the `aoxn` compiler (Rust only — no LLVM)
cargo test                                    # tests: pipeline + lib + TS + UI (see run_pkg_tests.sh for aoxn-pkg)
bash run_pkg_tests.sh                         # aoxn-pkg tests (Smart-App-Control hash-stamp workaround)
cargo run -- run examples\hello.ax            # compile+run an Aoxn program
cargo run -- run examples\stdlib_demo.ax      # imports ../stdlib/stdlib.ax itself
cargo run -- run selfhost\lex_demo.ax         # the Aoxn-written lexer tokenizing a sample
cargo run -- run selfhost\parse_demo.ax       # the Aoxn-written parser dumping a sample AST
cargo run -- run selfhost\tycheck_demo.ax     # the Aoxn-written checker accepting + rejecting
cargo run -- run selfhost\load_demo.ax        # Aoxn loader: imports + check of examples\stdlib_demo.ax
cargo run -- run selfhost\codegen_demo.ax     # self-hosted C emitter -> stdlib_use.c + .obj (needs clang)
cargo run -- run selfhost\driver_demo.ax      # Aoxn-written compiler: hello.ax -> exe (needs clang on PATH)
cargo run -- run selfhost\driver_stdlib_demo.ax  # Aoxn-written compiler: stdlib_use.ax -> exe
cargo run -- run selfhost\driver_frontend_demo.ax  # Aoxn compiler compiles its own lexer+parser
cargo run -- run selfhost\driver_self_demo.ax  # fixed point: Aoxn compiler compiles the Aoxn compiler
cargo run -- build examples\fib.ax -o f.exe   # emit a native executable (clang -O3 default)
cargo run -- c examples\fib.ax                # print the generated C (`ir` is a deprecated alias)
cargo run -- check examples\fib.ax            # type-check only: run the pipeline, print diagnostics, no C text (v0.31.1; what the IDE's Check button drives)
cargo run -- symbols examples\fib.ax          # top-level declarations with signatures and positions, incl. every import (v0.35.0; no clang — what the IDE's outline / go-to-definition / symbol search read)
cargo run -- run bad.ax --json                # diagnostics as JSON for agent consumption
cargo run -- run examples\fib.ax --O1         # clang -O1: fast compile (O3 stays the default)
cargo run -- build examples\fib.ax --O0       # clang -O0
cargo run -- run examples\ui_gallery.ax -l user32 -l gdi32   # UI widget gallery
cargo run -- doctor                                         # toolchain self-check
cargo run -- doctor --json                                  # same, machine-readable
cargo run -- version
```

**Installing (v0.30.0, window rewritten in v0.37.0)**: the shipped artifact is ONE
file, `Aoxn-<version>-Setup.exe` — the installer stub (`src/setup/`) with the
whole toolchain appended to it. Double-clicking it opens a native window
(`src/setup/ui.rs`, laid out after the Python installer) that unpacks into
`%LOCALAPPDATA%\aoxn`, puts `aoxn` on PATH and runs `aoxn doctor`; it downloads
nothing unless given `-InstallClang`. `-Console` is the headless mode for
scripts and CI. `dist/package.ps1` builds the exe and
`.github/workflows/release.yml` publishes it on a `v*` tag; CI runs the same
package → install → doctor → run check on every push. clang stays an
external prerequisite (Aoxn emits C).

Package management (`crates/aoxn-pkg`, beta): `Aoxn pkg <cmd>` plus direct
aliases `Aoxn init|add|remove|install|update|outdated|list|freeze|tree|why|
publish|yank|audit|trust|cache|npm-import`. Manifest `aoxn.json`, lockfile
`aoxn.lock`, install dir
`aox_modules/`; registries are directories, git repos, or read-only HTTP mirrors (`http://`, v0.29.4); resolution is
PubGrub; `Cargo.lock` pins aoxn-pkg's own dependencies. v0.29.1: bare
package imports resolve entries via the manifest's `main`/`exports`/`types`
(`src/pkg_manifest.rs`, zero-dep JSON reader). v0.29.4: read-only HTTP
registry backend (`crates/aoxn-pkg/src/registry/http.rs`, TLS-free
`std::net` client; `packages/names.json` feeds the typosquat guard).
v0.29.5: npm bridge `aoxn npm-import` (`npm.rs`) — the npm CLI is the
transport; Aoxn packages published to npm land in `vendor/<name>/` as
path dependencies. v0.32.0 (**measured against pip/pnpm**): devDependencies
+ `install --prod/--dev-only` (ONE lockfile, per-package `dev` flag —
`Lockfile::mark_dev_only`), manifest `overrides` (applied in
`resolve.rs::get_dependencies`), curated registries (`trust.json` +
`advisories/` in one repo: `aoxn trust bootstrap|list|check`, `trust.rs`),
`aoxn list`/`freeze` (`list.rs`), `--json` reports, parallel downloads
(`--jobs`, default 8; each worker holds its own `Registry::fork()` — backends
carry mutable state), and enforced `IndexVersion.aoxn` minimum compiler
versions. Topical docs: `docs/pkg-manager.md` and
`docs/trusted-registry.md`.

**Integrity has TWO digests; do not confuse them**: `checksum` is the
manifest hash over the unpacked file tree (the content anchor, pinned in
`aoxn.lock`, re-verified after extraction); `tarball_sha256` is the digest
of the served tarball bytes (the transport digest — ONLY the HTTP backend may
compare a download against it, and it is optional for pre-v0.32.0 indexes,
in which case the wire check is skipped, never the content one). Until
v0.32.0 the HTTP backend compared the download against `checksum` — two
unrelated quantities — so every real download failed with a bogus integrity
error, and its test was green only because it had written the tarball digest
into the manifest-hash field.

**The requirements fingerprint is `"<owner-pkg>/<dep>" -> "<req>@<registry>"`**:
the owner prefix stops two workspace members from overwriting each other's
entry, and the value must never embed a filesystem path (an absolute dir in
there made moving the project re-resolve forever). `lockfile_version` stays
1 — every field added since is optional with a serde default.

Optimization levels (v0.29.0): `--O0/--O1/--O2/--O3` select the clang `-O`
level used to compile the generated C — the C text itself is level-independent
(test `c_text_is_opt_level_independent`). `--backend c` is accepted for
compatibility (it is the only backend); any other value errors out.
`Aoxn run` AND `Aoxn build` share the exe cache: content hash of entry+all
transitive imports + compiler identity + options in `target/cache` — an
unchanged re-run/re-build skips compile and link (run: ~1.1s → ~85ms).
`AOXN_NO_CACHE=1` disables, `AOXN_CACHE_DIR=<dir>` relocates.

Env (install layout, v0.30.0): `AOXN_HOME=<root>` = toolchain root (default:
the parent of the executable's `bin/`), `AOXN_STDLIB=<dir>` = the stdlib
sources (default: `$AOXN_HOME/lib/stdlib`, falling back to the checkout in
dev builds). A bare `import * from "stdlib"` / `"stdlib/ui"` resolves against
that directory **after** the project-local `aox_modules/<name>` probe —
local packages always win. clang discovery: `AOXN_CLANG` → `PATH` →
`<root>/toolchain/bin/clang` (portable LLVM drop-in) → repo LLVM → system.

Env: `AOXN_DUMP_C=1` dumps generated C to stderr; `AOXN_TIME=1` prints
pipeline phase wall-clock (lex/parse/typecheck/codegen/link);
`AOXN_TC_TRACE=1` prints per-fn typecheck markers; `AOXN_CPU=native` (or
`--cpu native`) targets the host CPU; `AOXN_CLANG=<path>` selects the clang
executable. NOTE: `AOXN_DUMP_IR`, `AOXN_PASSES`, `AOXN_BACKEND`,
`AOXN_CG_TRACE` are DEAD since v0.29.0 (LLVM-era).

Single test: `cargo test --test pipeline recursion_fib`.

## Environment facts (non-obvious, verified)

- **clang is the only external toolchain** (needed to compile the generated C
  and to link). Resolution order: `AOXN_CLANG` env → PATH → repo-local
  `LLVM\bin\clang.exe` → `C:\Program Files\LLVM\bin\clang.exe`. The winget
  "LLVM 23.1.0" install on this machine survives purely as clang's home —
  no LLVM library is probed, linked, or shipped since v0.29.0 (`build.rs`,
  `src/llvm.rs`, `src/codegen.rs` are deleted).
- clang auto-detects MSVC Build Tools, which must be installed (Rust host is
  x86_64-pc-windows-msvc) for the final link. User programs needing C
  libraries pass `-l NAME` / `-L DIR` (forwarded to clang).
- **COFF TimeDateStamp flake trap (fixed in a23e096)**: clang stamps the
  wall-clock time into every Windows object (bytes 4–8 of the COFF header),
  so two compiles of identical C differ by exactly those bytes. The
  fixed-point test masks them (`tests/pipeline.rs`, Windows only). If you see
  an object diff "first diff at Some(4)", it is the timestamp, not a codegen
  bug.
- **Do NOT add external crates to the compiler library `src/`** — zero deps
  there is a security property (see SECURITY.md). `crates/aoxn-pkg` may use
  crates (clap/serde/semver/pubgrub/sha2/hex/flate2/tar/dirs/thiserror),
  pinned by `Cargo.lock`.
- MSVC STL (VS 2022 17.14+) rejects clang 18 ("expected Clang 19.0.0 or
  newer") when compiling C/C++ with headers — irrelevant for Aoxn emission,
  but breaks ad-hoc C++ comparison probes; declare externs manually instead
  of including headers.
- This dev machine's 10–50ms phase timings swing ±2× (AV/indexer): benchmark
  comparisons need interleaved 5× min/median or they lie.
- **`run_pkg_tests.sh` exists because Windows Smart App Control blocks
  freshly built unsigned test binaries after their first runs** — the script
  bumps a marker comment so each test build produces a new binary hash. Use
  it instead of bare `cargo test -p aoxn-pkg` on this machine.
- **The same AV behavior hits the INSTALLER, and it is worse there.**
  `aoxn-setup` unpacks an unsigned `aoxn.exe`; SAC/Defender can quarantine it
  *after* `aoxn doctor` has already run it successfully. The installer used
  to report **Done** and leave a PATH entry pointing at a file that no
  longer existed, so the failure surfaced at the next command as
  PowerShell's `"The term '…\bin\aoxn.exe' is not recognized as a name of a
  cmdlet"` — an error that blames the shell. `confirm_present` in
  `src/setup/main.rs` re-checks the binary after the self-test and fails the
  install with the cause and the remedy; `release.yml` checks the path before
  invoking it. **When touching the installer, keep that check.**
- **The installer downloads NOTHING by default** (v0.37.0). It unpacks, sets
  PATH and runs `aoxn doctor`. It used to `winget install LLVM.LLVM` whenever
  it found no clang, which turned a ten-second install into a multi-minute one
  behind a progress bar that never mentioned the download — indistinguishable
  from hung. `-InstallClang` re-enables it deliberately. **Do not add an
  unprompted download to the install path**; report what is missing instead.
- **The installer window (`src/setup/ui.rs`) is modelled on the Python
  installer** — Welcome / Progress / Done, with ONE primary button that changes
  meaning per phase (Install Now -> Cancel -> Close) rather than being swapped.
  Everything is owner-drawn in GDI. `main.rs` hands the UI a *closure*; the
  worker must not be spawned before the user clicks, or the toolchain unpacks
  into the disk with nothing on screen.
- `WNDCLASSEXW::lpszClassName` must point at memory that outlives the
  statement — Win32 keeps the pointer for the life of the class. The class
  names are leaked UTF-16 buffers (`wide_leaked`), not temporaries.
- `src/setup/main.rs` no longer contains a literal NUL byte (the payload magic
  is `b"AOXNSFX\x00"`, an escape). The file reads as text again, so grep/rg
  work on it. It *did* contain one from v0.30.0 to v0.37.0; if it ever comes
  back, do not "clean it up" blindly — check the magic spelling first.

## Windows tooling gotchas (learned the hard way)

- **PowerShell 5.1 `-Encoding UTF8` writes a BOM.** Aoxn `.ax` sources are
  read byte-wise — a BOM (EF BB BF) is an "unexpected character" at 1:1.
  After any PowerShell rewrite of a `.ax` file, strip BOMs:
  check bytes `[0]==0xEF && [1]==0xBB && [2]==0xBF` and slice them off.
  The Write tool does NOT add a BOM — prefer it over shell heredocs.
- **PowerShell quoting for inline code is a trap** (backticks, `$()`, nested
  quotes). For anything non-trivial, write a `.ps1` via the Write tool and
  execute it. Commit messages: `git commit -F <file>`, never `-m` with
  multiline text.
- **Git Bash eats backslashes in path arguments** — `git worktree add
  D:\ailanguage-bench\foo` creates a mangled directory. Use forward slashes.
- `winget download` of LLVM needs `--accept-source-agreements`; CI installs
  clang the same way (Windows).

## Architecture (pipeline order)

```
.aox source (.ax extension)  — or TypeScript (.ts/.tsx) via src/ts/
  → src/lexer.rs      tokens with line/col/file; emits NEWLINE/INDENT/DEDENT
  → src/parser.rs     recursive-descent AST (src/ast.rs), Python-style layout
  → src/typecheck.rs  strict check + FnSig table + GENERIC monomorphizer
  → src/codegen_c.rs  ISO C text → clang -c → object file (clang -O<n> -w -c)
  → src/lib.rs        load imports (src/files.rs registry) → link via clang → executable
src/main.rs           CLI (build / run / c / pkg...), --json diagnostics, -l/-L link flags
crates/aoxn-pkg       package manager crate (its own dependency set; see above)
```

- Diagnostics: `Diag { stage, file: u32, line, col, message }` in lib.rs;
  stages are `lex | parse | type | internal | link | io`. `file` indexes
  `src/files.rs` (thread_local registry) — resolved to names at print/JSON
  time. Compiler-internal failures must surface as `internal` diags, never
  panics.
- The compiler library `src/` has zero external crate dependencies. The C
  emitter is plain string building — no LLVM handles, no FFI of its own
  beyond what user programs declare.
- Multi-file: `load_program` (lib.rs) resolves `import "..."` recursively —
  include-once per canonical path, cycles rejected via an import stack,
  paths relative to the importing file. String-based APIs (`build_exe`)
  reject imports; only path-based entry points resolve them. A resolved
  `.css` target is diverted to the asset pipeline (`src/assets.rs`) instead
  of a front end; see the CSS assets section below.
- Diagnostics: `Diag { stage, file, line, col, message }` in lib.rs; stages
  are `lex | parse | type | internal | link | io | asset`. `asset` covers
  stylesheet problems and is constructed in `lib.rs`/`assets.rs` directly —
  `codegen_c.rs` is `Result<_, String>` and would flatten it to `internal`.

## C backend invariants (src/codegen_c.rs; mirrored by selfhost/codegen.ax)

- **C gives Aoxn's value semantics natively**: structs map to C structs;
  arrays are wrapped in single-field structs (`typedef struct { T data[N]; }`)
  so assignment/param/return copy by value exactly like C struct semantics.
- Aggregate ABI: aggregates cross function boundaries as pointer + sret
  out-pointer (spelled natively in C), same shape the old LLVM backend used.
- The generated C text is **level-independent** — `--O` only picks the clang
  level. Stage-1 (Rust-built) and stage-2 (Aoxn-built) compilers must emit
  byte-identical C AND byte-identical objects for the same program (the
  fixed point); the only tolerated object difference is the COFF timestamp
  the test masks.
- `target_os()` is a compile-time builtin folding `"windows"` on a supported
  build (`"other"` elsewhere) — **both** compilers must fold the same value
  (`platform::target_os_name()` Rust-side, `target_os()` inside
  `selfhost/codegen.ax`) or the fixed point breaks. It stays a builtin even
  though Windows is the only target: it is part of the language, and source
  that branches on it must keep compiling.
- Reserved-word collisions in emitted C identifiers are handled by the
  keyword table in codegen_c.rs; C runtime use is limited to the small
  builtin name list (`malloc`, `memcpy`, `strlen`, `snprintf`, …).
- platform.rs centralizes exe/obj extensions (`exe_ext`/`obj_ext`), the
  Windows stack-link flag (`-Wl,/STACK:8388608`), and `target_os_name()`.
  Tests must use these helpers, not literals (CI runs the suite on four
  platforms).

## Lexer invariants (Python-style layout)

- The lexer maintains an indent stack and emits `Indent`/`Dedent` tokens;
  the parser consumes `:` Newline Indent stmt* Dedent (or one simple stmt).
- Blank lines and comment-only lines produce **no tokens** (no indent changes).
- Inside parentheses `paren_depth > 0`: newlines and indentation are ignored
  (implicit line joining).
- EOF: flush trailing `Newline`, then pending `Dedent`s, then `Eof`.
- `//` is NOT a comment (it is int division); comments are `#` only.
- Indent/dedent mismatch → lex error "unindent does not match any outer
  indentation level".
- Tests embed indented sources; `dedent()` in tests/pipeline.rs strips the
  common leading whitespace before compiling.

## Language semantics (enforced, keep strict)

- No implicit int/float conversions, no re-typing a bound name, `bool`
  conditions only, all-paths-return, no unreachable code. This strictness is
  a feature (deterministic, AI-verifiable); do not relax it casually.
- Python-style surface, native semantics: `def`/`elif`/`pass`, `x = 5`
  infers, `x: int = 5` checks, re-assignment keeps the type, `and`/`or`/`not`
  + `True`/`False` are aliases of the symbolic operators.
- **Bitwise/shift operators exist (v0.38.0)**: `&`, `|`, `^`, `~`, `<<`,
  `>>`, **int-only** like `%`. They were added for the crypto/TLS/HTTP2 work
  (SHA-256 rounds, the TLS 1.3 key schedule, HPACK varints, UTF-8
  continuation bytes). Two traps, both pinned in `tests/pipeline.rs`: **in C
  `==` binds TIGHTER than `&`** (`a & b == c` is `a & (b == c)` — write
  `(a & b) == c`), and there are **no augmented bitwise forms** — write
  `x = x & y`. Do not "fix" the precedence to what looks intuitive.
- **`vec_push` delegates again (v0.39.2) — the "self-host trap" is
  ROOT-CAUSED, and it was never a compiler bug.** The culprit is
  `struct_cycle` in `selfhost/typecheck.ax`: the cycle detector threaded its
  DFS path as a `Vec` **by value**, pushed onto it, then recursed. The
  callee's push can realloc, and realloc frees the buffer that every other
  copy of the handle still points at — so on return the caller's `path`
  dangled and the next sibling branch walked freed memory (0xC0000374). It
  stayed latent because the inline growth reserved 8 slots up front, so the
  first eight pushes of any path never reallocated; the delegating form
  grows 1, 2, 4, … and reallocates on almost every push, which turned the
  latent bug into a sure crash. The fix is the write-back discipline the
  rest of the self-hosted code already follows: return the live handle with
  the answer (`struct Cycle{hit, path}`) and hand back `vec_pop(mine)`, so
  the caller's ancestor chain is restored but points at the current buffer.
  Returning the *extended* path instead is wrong in the other direction —
  the DFS path must be the ancestor chain, not every node ever visited, or a
  node reached twice through different branches reads as a cycle.
- **The general rule this exposes**: a `Vec` handle must be refreshed from
  its callee whenever that callee may have grown it. A function that takes a
  `Vec`, pushes onto it, and recurses **cannot return `int`** — it has no
  way to hand the handle back. `load_file` gets this right (`ls =
  load_file(ls, …)`); the old cycle detector had no such door.
- **`raise` / `try` / `except` exist (v0.40.0) — the v0.39.0 "no exceptions"
  decision was reversed by user instruction (2026-10-04).** `raise <string>`
  unwinds to the nearest `try` handler (or out of the function; uncaught =
  one report, exit 1). The runtime is two statics (pending flag + message), a
  `goto`, and a slot check after every statement that called a raiser — the
  may-raise set is a fixpoint over call names, and calls through a function
  pointer always count. A raise inside a try BODY is caught by that try; in
  a handler it propagates. `raise` counts as an exit for all-paths-return
  (except in a try body). The v0.39.0 `Err`-value channel STAYS for
  inspected failures; `Diag` stays compile-time. **Known wart, pinned by
  test**: a statement's check runs after the statement, so
  `print(f(x))` with a raising `f` prints the zero return value before the
  hop — fixing it means hoisting raising args out of the call, in
  `codegen_c.rs` AND the selfhost mirror.
- **`dict[V]` is a HEAP HANDLE (v0.40.0), not a value** — `struct
  ax_dict_V*` over parallel key/value arrays, like the UI's `TableModel` and
  the web `FileTable`. The first spelling passed the 4-word struct by value
  and was unsound twice over (`len`/`cap` lived in the copy, so a callee's
  growth/del was invisible; the stale `len` then walked off a reallocated
  buffer — correct at -O0, segfault at -O1+, reproduced with clang on the
  bare C). Copying the handle SHARES the dict; set/del through any copy is
  visible to all. A missing key RAISES `dict key not found` (the reason any
  program with a dict pulls in the error slot); `dict_has` guards reads.
  `{}` needs an annotation (`d: dict[int] = {}`) — and that annotation path
  is special-cased in the Let arms of BOTH the checker and codegen, because
  `check_expr`/`hint` have no expected type to give an empty literal.
- **v0.40.0 also added function pointers and `T | None`**: a bare function
  name in value position is its address (`fn(A,...) -> R` annotations, `as`
  casts between `int` and fn-ptr, `to_int(f)`); `None` + `T | None` is the
  only union form, `is None` / `is not None` narrow per branch (checker AND
  codegen keep the narrowed view), and print/operators reject a nullable
  until narrowed. **The selfhost mirror has NONE of v0.40.0 yet** —
  `selfhost/codegen.ax` emits no dict/None/fn-ptr/raise text, so the fixed
  point only covers programs without them. Porting is future work.
- Arrays `[T; N]` and structs are first-class value types (copy on
  assignment/param/return). Indexing is unchecked (C-style); struct fields by
  name; construction requires every field by name (`Point(x=1, y=2)`).
- Strings are immutable byte sequences: `+` concatenates, all six comparisons
  work byte-wise, `len()` is bytes. They live in struct fields/arrays by
  shared pointer (safe: immutability); concat results leak by design.
- **One array length parameter per function** (`def f[N](a: [T; N], ...)`;
  multiple differently-sized arrays in one signature are still unsupported —
  this shapes API design, e.g. the UI toolkit's table/tree take heap-backed
  model objects instead of parallel arrays).
- When grammar/semantics change: update `docs/spec.md`, parser, typecheck,
  codegen, and add a test in `tests/pipeline.rs` in the same change.

## UI toolkit (stdlib/ui.ax + ui_draw.ax + a backend) — v3 widget set, 3 OS

- **THREE FILES**: `ui.ax` (portable core) + `ui_draw.ax` (the
  platform-NEUTRAL widget layer) + the Win32/GDI backend `ui_win.ax`.
  A program pulls the toolkit in with ONE import line; widget names are the
  same everywhere. `ui_draw.ax` declares no platform externs and never
  tests `target_os()`. (The X11 backend was removed in v0.30.0.)
- **`plat_*` primitive contract** (see the `ui_draw.ax` header): the widget
  layer only ever calls `plat_fill_rect` / `plat_text` / `plat_measure` /
  `plat_clip_push` / `plat_pump` / `plat_init` / … and each backend
  supplies them. THREE tests pin this (`tests/ui.rs`): the widget layer
  must name no Win32/Xlib symbol; both backends must implement the SAME
  `plat_*` set; every `plat_*` called must exist in both. Adding a widget
  that reaches for a platform symbol fails the test run, not one OS.
- **No second windowing backend.** `extern def` can only pass
  int/f64/string/bool, so a backend whose window/draw API takes structs by
  value (AppKit's `NSRect`/`CGRect`, for one) is not expressible, and there
  is no C shim escape hatch — the compiler only emits its own C text
  (`src/codegen_c.rs`) and shells out to clang. Do NOT add a native backend
  for another windowing system; if a future version gains struct-typed
  externs, revisit then.
- Qt-flavored **immediate mode** (no callbacks possible: the language has no
  function pointers/closures). Widgets are per-frame functions; app state
  travels via the write-back idiom (struct in / struct out). Reference:
  `docs/ui.md`; showcase: `examples/ui_gallery.ax`; tests: `tests/ui.rs`.
- **Signal-slot without function pointers**: an integer-channel event bus —
  `ui_connect(c, signal, slot)` + `ui_emit(c, signal, kind, a, b)` queues
  `Ev` records read via `ui_event_count`/`ui_event(i)`; the app dispatches
  one switch on `ev.slot`. Widgets don't hold handlers.
- **Model/view without interfaces**: heap-backed `TableModel`/`TreeModel`
  (pointer structs + get/set fns) feed `ui_table`/`ui_tree`. Models are
  pointers precisely so views can mutate them (no value-copy write-back).
- Text editing: `Sel{anchor, caret}` byte-offset selection, UTF-8 aware;
  `ui_textbox` (single line) + `ui_textedit` (multi-line, line cache in the
  heap block) share pure edit ops in the portable half; clipboard via
  `ui_clip_get/set` (CF_UNICODETEXT). Render text slices with
  `ui_measure_sub`/`ui_draw_text_sub` (frame-arena based — never `str_sub`
  on a frame path).
- **GDI stock pens: `DC_PEN = 19`, `DC_BRUSH = 18`** — `GetStockObject(20)`
  is out of range and fails silently (no outline ever drew; fixed in
  v0.29.2). Keep the decimal-constant discipline: no hex literals, no
  bitwise ops in the language.
- The shared heap block (`st`, 1024 i64 slots) layout is documented in the
  `ui.ax` header comment — extend it there when adding state. Slots
  820/821 = the clip-rect stack, 822/823 = the clipboard buffer,
  532 = the close request consumed by `plat_pump` (see below).
- **GDI `DC_PEN`/`DC_BRUSH` traps (Windows)**: `GetStockObject(20)` is out
  of range and fails silently. Keep the decimal-constant discipline in the
  UI sources: no hex literals there (bitwise/shift operators exist since
  v0.38.0, but the UI toolkit keeps to plain arithmetic on raw constants).
## Self-hosting status (docs/selfhost.md is a historical assessment; wiki/Self-Hosting.md is frozen at v0.29.2, the source is current)

- Stages 1–4 + loader + driver all DONE (`selfhost/`, ~7k lines of Aoxn):
  lexer, parser (arena AST), typecheck (incl. generic monomorphization),
  loader (include-once + cycles), codegen (**C text emitter mirroring
  `codegen_c.rs`** — the LLVM-C FFI port is gone since v0.29.0), driver
  (load → check → emit C → `clang -O3 -w -c` → link).
- **FIXED POINT**: `selfhost/driver_self_demo.ax` — the Aoxn-written driver
  compiles the whole self-hosting compiler into `target/selfhost_stage2.exe`;
  stage-2 then compiles a stdlib program and the generated C **and the
  emitted object** match the Rust-built compiler's byte-for-byte (objects
  modulo the COFF timestamp, see above). Runs on every platform now (skips
  only when clang is missing) — no more `LLVM-C.lib` precondition. Verified
  by `selfhost_driver_self_compiles`. `gen_c_text` (codegen.ax) + `emit_c`
  (driver.ax) are the self-hosted `aoxn c`.
- The Aoxn driver also compiles its own front end
  (`selfhost/driver_frontend_demo.ax`) and the whole `examples/` suite.
  Next rung: porting `main.rs` CLI semantics (argv is still missing from the
  language).
- CRITICAL value-semantics discipline (the v0.11 segfault): a function
  taking `p: PState`/`c: CState` by value MUST return the struct — local
  `p.n_tag = vec_push(...)` mutations are discarded by the caller. All
  selfhost helpers use write-back (`p = parse_expr(p)`) and pass extra
  results through fields (`p.res_node`, `p.res_vec`, `c.res_ok`, `c.r_ty`).
  Never nest mutating calls as arguments; assign each step.
- Porting traps that still matter: call-argument chains walk ARG nodes
  (`node_child(node_next(arg))`), never `node_next` of the first value;
  `and`/`or`/`not` lex as `&&`/`||`/`!` (no separate keyword tokens); an
  extern's return type is the last child (there is no body block); the
  loader must `vec_set(n_next, ch, -1)` before splicing file roots or you
  get a self-cycle; keep the global root in LoadState (`parse_program`
  overwrites `p.root` per file); `range(n)` with ONE argument must move the
  value to `end` before zeroing `start`; `if` merge branches must come from
  the real end block of each path (`LLVMGetInsertBlock`'s C-backend
  equivalent: whatever block the builder is at after the branch body).
- Aoxn-on-Aoxn debugging recipe: runtime crash in generated code → check
  seed values first (`vec_get(empty, -1)` dereferences NULL); compiler crash
  → per-fn stage traces (`AOXN_TC_TRACE`); silent wrong output → dump both
  sides' C (`AOXN_DUMP_C=1`) and diff.

## TypeScript front end (src/ts/) — TS-M1 W1 complete (v0.28.0)

- S0 lexer + S1 parser + S2a generics/arrays/templates + S2b f64 tower,
  unions/narrowing, any/optional/tuples + S3 modules ALL DONE. `load_file`
  dispatches on extension (`.ts`/`.tsx` → `ts::parser`), `aoxn build foo.ts`
  just works. TS lowers into the EXISTING `crate::ast` (console.log→print,
  x.length→len(x), object literal→StructLit via interface annotation).
- Legacy Aoxn `import "path"` is REMOVED (one-shot switch in W1-S3) — use
  `import * from "path"` everywhere, including stdlib/selfhost sources.
- **`import * from "p"` compiled in .ax files but NOT in .ts files until
  v0.36.0** — the TS arm that should have handled the whole-module merge
  returned an error on seeing `*`, so a `.ts` file could import nothing at all
  (its own error text recommended the form it refused). `import * as ns`
  still needs namespace objects and still errors (TS-M2); the two are told
  apart by peeking one token further (`src/ts/parser.rs`, `import_decl_ts`).
- `number` is f64 (double) everywhere since S2b; explicit type args
  `f<T>(x)` stay rejected (ambiguity with `a < b > (c)`).
- Roadmap: see `docs/ts-m1-spec.md` — W2 (pkg + CSS pipeline), W3 (benchmark
  v2), W4 (TS-M2 runtime semantics: objects/closures/GC — the "full TS
  compatibility" gate), W5 (acceptance samples). `crates/aoxn-pkg` (W2's
  package manager) landed as beta in v0.29.0; the CSS pipeline + CSS Modules
  landed in v0.34.0 (docs/css-assets.md).

## CSS assets (src/assets.rs, v0.34.0) — B1 phase 1

- **A `.css` file reached through `import` is an ASSET, never source.**
  `load_file` (src/lib.rs) diverts it after include-once/cycle detection and
  BEFORE the front-end dispatch — otherwise the `is_file()` short-circuit in
  `complete_module_path` lets it through and it dies in the Aoxn lexer with
  `unexpected character '{'`, which says nothing useful.
- **Assets ride into codegen as ordinary `FnDecl`s** whose bodies are
  compile-time string literals, reusing the `TS_RUNTIME_SRC` injection trick
  (src/ts/runtime.rs). So `src/codegen_c.rs` has NO asset dispatch and
  `selfhost/codegen.ax` needs no mirror — the fixed point holds BY
  CONSTRUCTION. Do not "improve" this by adding a codegen builtin: that
  reintroduces the self-hosting mirror obligation.
- **Minification is comments + whitespace ONLY.** No selector merging, no
  reordering, no dropping the trailing `;`, no empty-rule elision. Same
  discipline as `c_text_is_opt_level_independent`: the emitted text must be a
  pure function of the source. A comment counts as whitespace (`a/**/b` must
  not become `ab`).
- **Class rewriting must be context-aware.** A `.` inside a string literal, an
  `@media` prelude, or a declaration value is NOT a class selector. The three
  tests in `tests/assets.rs` that pin this are load-bearing, not decoration.
- **`dependency_files` MUST know about `.css`** (including `@import`ed
  partials) or editing a stylesheet silently serves a stale cached exe.
  `cache_key` (src/main.rs) hashes whatever that returns.
- **Tailwind is PRE-GENERATED CSS, deliberately** — not a dependency. It is a
  plain-JS npm package with no `aoxn.json`, which `aoxn npm-import` rejects by
  design (`plain_js_package_is_rejected`). Shelling out to the Tailwind CLI
  would make Node a build prerequisite; the one-click install avoids that.
  v0.36.0 added a **built-in utility subset** (`src/tailwind.rs`,
  `--tailwind`) after the user explicitly asked for it. It mines ONLY
  `class=`/`className=`/`class:` attributes — the earlier "any string shaped
  like a class list" heuristic was removed because prose passes the same shape
  test. Keep it that way, and keep naming unsupported utilities at build time.
- **`--emit-assets <dir>` writes files** (v0.36.0). Emit beside the OUTPUT
  executable, never into `cache_dir()`: `prune_cache` walks a flat directory
  and only removes 16-hex names, so cache-dir emissions leak forever. A
  relative path resolves against `exe.parent()`. It must run on the cache-hit
  path too (the cache does not know the output dir), which is why
  `--emit-assets` and `--tailwind` are in `cache_key`.
- **Emitted names are compiler-generated and re-checked** by
  `is_safe_emitted_name` before any write. A stylesheet may influence the
  *contents* of an emitted file (via `url()` rewriting) but never its path.
- **Programs find assets at runtime via `exe_dir()`/`asset_path()`**
  (stdlib, `GetModuleFileNameA`), NOT a baked absolute path — baking one
  would make the exe non-relocatable and leak a build path.
- A `url()` target that does not resolve **warns and passes through**; it must
  not fail the build (a CDN font or a file another tool copies in is valid).
- Reference: `docs/css-assets.md`. Tests: `tests/assets.rs`,
  `tests/assets_v36.rs`.

## Web benchmark suite (web/) — facts for future sessions

- Platform direction decided (docs/web-platform-plan.md + docs/ts-m1-spec.md):
  TS front end -> existing pipeline (full TS syntax compatibility goal),
  **independent** package management (own manifest `aoxn.json`/`aoxn.lock`/
  `aox_modules/`; no registry proxy), CSS full compatibility + Tailwind
  toolchain (as pre-generated CSS) + CSS Modules (both landed v0.34.0).
  Acceptance baseline: 3 canonical samples
  (REST API / SSR page / generic util lib) until real team projects arrive.
  Render metrics: Playwright + Chrome FCP/LCP/TTI.
- `web/server_win.ax` (`aoxn build web\server_win.ax -o web\server.exe -l ws2_32`);
  shared `web/serve.ax` + `web/http_buf.ax` + `web/sock_win.ax` (ws2_32).
  (The POSIX server/socket modules were removed in v0.30.0.) Bench targets:
  `web/node-server.mjs` (plain node:http) and `web/next-app/` (Next.js 15 app
  router, `pnpm build`/`pnpm start`). Results: `docs/web-benchmark.md`.
- Observability: `/metrics` (Prometheus text, RED counters per route, bytes,
  connections, duration sum/max ns, uptime, static counters). Metrics block
  layout is documented above `render_metrics` in http_buf.ax (slot 104 =
  clock scratch pointer, 136 bytes total since v0.29.6 — slots 112/120 are
  the static-file counters). `net_now_ns(m)` lives in `web/sock_win.ax` and
  uses QueryPerformanceCounter — **`timespec_get` does NOT link on Windows**
  (clang/MSVC libs), don't retry it.
- **`load_i64` takes ONE argument (an address)** — offsets go on the address
  (`load_i64(ts + 8)`); only `load_u8` takes `(base, off)`.
- **C `int` returns arrive zero-extended in i64** (callee writes EAX): -1
  shows up as 4294967295. The sock modules' `i32()` helper maps it back (a
  true 64-bit -1 passes through). SOCKET/pointer returns are full 64-bit.
- **No `\r` string escape** (`\n`, `\t`, `\\`, `\"` only) — HTTP CRLF is
  written as raw bytes 13/10 (`bb_crlf`).
- Long-running servers must render into byte buffers (`bb_*` in
  `http_buf.ax`): string concat results leak by design, so per-request concat
  would balloon RSS. This is idiomatic (C-style), not a bug.
- HTTP framing: drain loop in `serve.ax` handles several requests per recv
  and partial requests (scan for `\r\n\r\n`, slide remainder with a byte loop
  — overlapping `memcpy` is UB). Dynamic responses go out as ONE segment
  (header rendered in front of the body at fixed offset 512 in `bbuf`) with
  TCP_NODELAY; a split header/body send stalls on Nagle (~ms/request).
  Static file responses (v0.29.6) stream header + body separately — the
  body may exceed `bbuf` and is sent straight from the preloaded file's
  memory.
  `render_body` returns the NEW absolute offset — subtract the body start.
- **Static files (W2, 2026-10-02)**: docroot files are PRELOADED into a
  `FileTable` at startup (`load_static_table` — `read_file` once; requests
  never touch the disk). Targets are matched against the FIXED name table
  (`static_names()`), never turned into paths — traversal is impossible by
  construction. ETag is `"<size>.<hash>"` where hash is a BOUNDED
  polynomial (`content_hash`): **do not switch it to FNV/multiplicative
  hashing** — signed int overflow is UB in the language contract; the Node
  server mirrors the exact arithmetic and parity.mjs pins it. Range
  parsing follows RFC 9110: syntactically invalid / multi-range → 200,
  unsatisfiable → 416, valid single/suffix range → 206; `If-None-Match`
  beats Range (304). Missing docroot files = empty body entries → 404.
- Winsock bind: do **not** add `SO_REUSEADDR` on Windows — it enables
  hijacking of another process's listener. `bench.mjs` has a port pre-flight
  and `web-bench.yml` clears stray listeners instead.
- Tests: `web/loadtest/parity.mjs` (functional: body parity vs Node, 404,
  keep-alive, /metrics) and `web/loadtest/bench.mjs` (oha engine, 3
  interleaved trials, median; raw runs in `last-results.json`).
  `.github/workflows/web-bench.yml` runs parity + a 5s reference bench on
  windows-latest (runner numbers are trend-only).
  oha binary goes in `loadtest/tools/` (gitignored — download cmd in
  `web/README.md`). autocannon is NOT used: single-core client capped at
  ~4k req/s and masked server differences. Even oha caps at ~7k req/s on
  this machine — use latency/in-flight (Little's law) to tell servers apart,
  not raw rps.
- Same-machine benchmark caveat: client + server share the 4C8T i5-1135G7;
  Node p50 jumps 4→9.7ms under 2-client load while Aoxn stays at 0.1ms.

## OpenAI SDK (stdlib/openai/) — v0.40.1

- **FIVE modules, all plain Aoxn** (selfhost-compilable: no dict/None/fn-ptr/
  raise anywhere): `codec.ax` (base64 RFC 4648, percent-encoding, UTF-8 ↔
  UTF-16LE), `json.ax` (the JSON DOM), `http.ax` (WinHTTP transport), `sse.ax`
  (the SSE pull filter), `client.ax` (`OaClient`, resources, accessors).
  Reference: `docs/openai-sdk.md`; tests `tests/openai_sdk.rs` (2 drivers);
  example `examples/openai_chat.ax`. Programs link `-l winhttp`.
- **The JSON DOM is a slab of 40-byte nodes** (kind / i64 / f64 / ptrA / ptrB),
  child buffers are adopted `Vec.data` arrays of slab indices, and the parser
  follows the write-back discipline (`p = jp_value(p)`, extra results through
  `p.res` / `dom.last`). **Empty-dom accessors are guarded** (`slab == 0`
  reads as null) — a failed `j_parse` hands back an empty dom and every
  getter/dumps must tolerate it; the original draft dereferenced NULL there
  and the crash looked like a parser bug.
- **`snprintf` CANNOT be used from an Aoxn extern.** Aoxn externs are
  fixed-arity, but printf-family is variadic: the prologue reads its float
  varargs from the XMM spill slots, which a fixed-arity call site never
  fills — `j_fmt_f(3.25)` printed `2.47e-323` (the bit pattern of 5). Float
  formatting is hand-rolled integer math (`j_fmt_f`, ~%.15g). The same trap
  waits for anyone declaring `printf`/`sprintf`/`fprintf`.
- **No `\r` escape — again.** Header blocks (HTTP CRLF) and SSE CRLF
  fixtures must spell byte 13 via `store_u8` (`oa_crlf()` in http.ax). This
  bit twice in one session (transport, then the test fixture).
- **The JSON serializer failed silently at first**: `j_quote` escaped
  content but never emitted the surrounding `"` — `j_dumps` produced
  `{model:gpt-4o}` (invalid JSON) and the round-trip test crashed on the
  empty-dom NULL above. Round-trip tests (parse → dumps → parse → dumps)
  are the pin that caught both.
- **The end-to-end test needs NO network**: a mock OpenAI server written in
  Aoxn (`web/sock_win.ax`, ws2_32) is spawned on a loopback port (env
  `MOCK_PORT`, `MOCK_COUNT` = requests to serve before exit) and the real
  WinHTTP path walks a chat completion, a full SSE stream and a 401 body.
  **The port-poll consumes a mock slot** — the poll `TcpStream::connect`
  lands as a request (400 response), so `MOCK_COUNT` must exceed the
  client's request count by at least one.
- Client defaults mirror openai-python: base URL `https://api.openai.com/v1`
  (env `OPENAI_BASE_URL`), key from `OPENAI_API_KEY`, org `OPENAI_ORG_ID`,
  project `OPENAI_PROJECT_ID`, 600 s receive / 5 s connect timeouts, 2
  retries after the first attempt (0.5 s, 1 s; `Retry-After` ≤ 120 s wins;
  only transport errors, 429 and 5xx retry). Errors follow the v0.39.0
  Err-value channel: `oa_resp_ok` gates, `oa_err_msg` renders transport /
  HTTP / `error.message` — nothing raises.


## IDE (ide/) — Tauri 2 + Next.js + Monaco workbench (v0.31.0, continued v0.31.1)

- `ide/src-tauri` is EXCLUDED from the root Cargo workspace — a plain
  `cargo test` at the repo root must never compile a Tauri app. Its tests
  run separately: `cargo test --manifest-path ide/src-tauri/Cargo.toml`.
- Frontend tests/typecheck: `pnpm --dir ide test` (node:test on
  `test/*.test.ts`, no framework) and `pnpm --dir ide typecheck`.
- **It drives the real compiler** (`ide_check`/`ide_build`/`ide_run` shell
  out to `aoxn`); Check runs `aoxn check` (v0.31.1) — NOT `aoxn c`, which
  prints the generated C and would flood the output panel.
- **Never round-trip a file path through Monaco's `model.uri`** — the
  wrapper parses the `path` prop into a URI and a Windows path does not
  survive `uri.toString()`; v0.31.0 had Ctrl+S silently no-op in the native
  app because of exactly that. Always pass the doc's path prop through.
- Monaco language + theme are registered in the Editor's `beforeMount`
  (strictly between Monaco's load and the first model's creation); a
  page-level effect raced the mount in v0.31.0 (light-theme flash).
- `fs::canonicalize` on Windows returns `\\?\D:\...` verbatim paths —
  `fsops::pretty` strips the prefix so the tree, the log and the compiler's
  echoed diagnostics all spell plain `D:\...`.
- **The Packages panel is a remote control, not a reimplementation**
  (v0.33.0): `pkg.rs` runs the real `aoxn pkg` from the workspace root, and
  `ALLOWED_SUBCOMMANDS` is the whole surface — publish/yank/cache/trust
  bootstrap/npm-import are refused in Rust, never merely hidden in the UI.
  `default-run = "aoxn"` in the root Cargo.toml is what keeps bare
  `cargo run -- <cmd>` working now that the repo ships two binaries (aoxn +
  aoxn-setup); removing it breaks the CI smoke test and every documented
  command.
- **The IDE's language service is the COMPILER's, not a parser in the IDE**
  (v0.35.0): `aoxn symbols <file> --json` (`src/symbols.rs`) exports the top-
  level `def`s and `struct`s of a file and every file it imports, walking the
  AST `load_program` already built. The outline, Ctrl+click and Ctrl+Shift+O
  all read that one table (`ide/lib/symbols.ts`), so they cannot disagree;
  the ranking and jump rules are pure functions there precisely so they are
  testable without a render. It stops after the PARSER on purpose — an
  editor re-asks on every file switch and clang is far too slow for that.
  **Do not add an editor-side regex outline**: it cannot know where a
  declaration ends, which name is a parameter, or that `extern def` has no
  body.
- **Diagnostics are STRUCTURED first, text second** (v0.35.0): the IDE runs
  `check`/`build`/`run` with `--json`, and `ExecResult.diags` carries the
  parsed report BESIDE the raw `output` — the panel shows the words, the
  markers use the structure. `lib/diagnostics.ts`'s text scanner is the
  fallback for a pre-v0.35.0 compiler or a command that died before
  printing; keep it, and keep it tested. `toolchain::json_document`
  extracts the document with a STRING-AWARE brace scan, because `}` inside
  a diagnostic message is not the end of the JSON.
- **One spelling of every path** (v0.35.0): the loader registers the
  CANONICAL path with the verbatim prefix stripped (`paths::strip_verbatim`),
  because the compiler, the symbol table and the IDE's tree all have to
  agree on how a file is written or cross-file navigation silently breaks.
  `src/setup/main.rs` keeps the same rule for the payload it unpacks.
- `pnpm dev` runs the whole workbench in a browser against an in-memory
  fixture (`lib/bridge.ts`, tree derived by `lib/tree.ts`) — layout work
  without a 12-minute native rebuild.

## Known issues (deliberately unfixed — do not "drive by" fix)

- The self-hosted loader opens files with narrow `fopen` (stdlib `read_file`),
  so non-ASCII paths fail there; the Rust compiler is unaffected. Tests keep
  fixture dirs ASCII.

## Repo layout

- `examples/*.ax` — demo programs (hello, fib, primes, vectors, strings, benchmarks, stdlib_demo, ui_demo, ui_gallery); since v0.30.0 they import the stdlib **by name** (`"stdlib"`, `"stdlib/ui_win"`), so the ones shipped in a release archive run from any directory — `selfhost/load.ax` does NOT do name resolution (repo-bound, relative paths only)
- `web/` — web benchmark suite: HTTP/1.1 server written in Aoxn (FFI sockets)
  vs pnpm+Node.js+Next.js; see `web/README.md` + `docs/web-benchmark.md`
- `stdlib/stdlib.ax` — the standard library, written in Aoxn itself;
  `stdlib/ui.ax` + `stdlib/ui_win.ax` — the Qt-flavored immediate-mode UI
  toolkit v3 (layout engine, text selection/multi-line editing, menus,
  tree/table model+view, signal-slot events)
- `selfhost/` — the compiler rewritten in Aoxn (fixed point reached; C emitter)
- `crates/aoxn-pkg/` — the package manager (`aoxn pkg`, beta)
- `ide/` — the Aoxn IDE (Tauri 2 + Next.js + Monaco); `docs/ide.md` is its
  reference; `ide/src-tauri` is its own Cargo workspace (excluded above)
- `dist/package.ps1` — builds the single-file installer;
  `src/setup/` — the installer stub and its Win32 window
- `docs/selfhost.md` — historical self-hosting assessment (v0.19-era);
  `docs/install.md` — install guide (Windows);
  `docs/platform-support.md` — what "Windows only" means and what a port
  would need;
  `docs/spec.md` — language spec + roadmap;
  `docs/ui.md` — UI toolkit reference (v3);
  `docs/llvm-independence-report.md` — why/how LLVM was removed;
  `docs/ts-m1-spec.md` / `docs/web-platform-plan.md` — TS platform decisions
- `AGENTS.md` — this file: agent/contributor working agreement (published
  since v0.29.3)
- Repo: github.com/AlonechatWorkspace/Aoxn-language · Apache-2.0 · CI: one
  job, `windows-latest` (winget clang). It runs the suite, then packages the
  single-file installer, installs it into a scratch prefix and runs
  `aoxn doctor` — a broken installer fails the build.
- **Windows only (v0.30.0).** Do NOT reintroduce macOS/Linux claims, CI jobs
  or the X11 backend. The wiki is frozen and still describes the old
  multi-platform matrix; `docs/platform-support.md` is the current answer.
