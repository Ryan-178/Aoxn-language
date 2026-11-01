# Contributing to Aoxn

Thanks for taking a look at Aoxn. This repository **is** the compiler: a
Python-syntax, statically typed language that lowers to ISO C and compiles
through clang to native code, written in Rust with **zero external crate
dependencies in the compiler library** (`src/`). The CLI's package manager
has a separate dependency set. Bug reports,
spec corrections, tests, stdlib work, performance measurements, and docs are all
useful — you do not have to write compiler internals to help.

By participating you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).
Please report security vulnerabilities privately per [SECURITY.md](SECURITY.md)
— not in a public issue.

- [Ways to contribute](#ways-to-contribute)
- [Getting started](#getting-started)
- [Build and test](#build-and-test)
- [Repository layout](#repository-layout)
- [Changing the language](#changing-the-language)
- [Code conventions](#code-conventions)
- [Tests](#tests)
- [Commits and pull requests](#commits-and-pull-requests)
- [AI-assisted contributions](#ai-assisted-contributions)

## Ways to contribute

| Kind | Start here |
|---|---|
| Compiler bug (wrong output, crash, bad diagnostic) | [Bug report](https://github.com/AlonechatWorkspace/Aoxn-language/issues/new?template=bug_report.yml) |
| Feature or tooling request | [Feature request](https://github.com/AlonechatWorkspace/Aoxn-language/issues/new?template=feature_request.yml) |
| Grammar or semantics change | [Language proposal](https://github.com/AlonechatWorkspace/Aoxn-language/issues/new?template=language_proposal.yml) — discuss before implementing |
| Unclear or missing docs | Fix it directly, or file a bug report and say it is documentation |
| Standard library (`stdlib/stdlib.ax`) | `.ax` source, generics, value semantics — no Rust needed |
| Self-hosting (`selfhost/*.ax`) | The compiler being rewritten in Aoxn; see [`docs/selfhost.md`](docs/selfhost.md) |
| Performance | Measure first. [`docs/optimization-report.md`](docs/optimization-report.md) shows the format: numbers, machine, method, before/after |

## Getting started

Aoxn supports **Windows x86_64 (first-class)** and **Linux x86_64**
(since v0.46.0). macOS is unsupported. Prerequisites:

| Requirement | Notes |
|---|---|
| **Rust** (stable) | Windows uses `x86_64-pc-windows-msvc`; Linux uses the native GNU toolchain |
| **clang** | Required to compile generated C and link programs, not to build the Rust compiler library. Discovery is in `find_clang` in `src/lib.rs`; `AOXN_CLANG` overrides it |
| **MSVC Build Tools** (Windows) | clang auto-detects the host linker/CRT |
| **X11/Xft development libraries + Xvfb** (Linux UI tests) | `apt install clang libx11-dev libxft-dev libfontconfig1-dev xvfb` |

Since v0.29.0 there is **no LLVM-C library dependency**; the C-emitting
backend (`src/codegen_c.rs`) is the only backend. The zero-external-crate
policy applies to the compiler **library**; the CLI also links the local
`aoxn-pkg` crate, which has its own pinned dependencies.
Read [`docs/platform-support.md`](docs/platform-support.md) before changing
platform assumptions. [CI](.github/workflows/ci.yml) builds, tests and
smoke-tests on `windows-latest` and `ubuntu-latest`; Linux additionally runs
the X11 probe under Xvfb. Windows-only tests remain cfg-gated.
[Release automation](.github/workflows/release.yml), separately, packages
and verifies the Windows installer (install, doctor, run).

```powershell
git clone https://github.com/AlonechatWorkspace/Aoxn-language.git
cd Aoxn-language
cargo build
cargo run -- run examples\hello.ax
```

Working on the compiler itself you do not need the installer — but the
shipped artifact (`Aoxn-<version>-Setup.exe`, one file with the whole
toolchain inside) is worth exercising before a release, and any install can
be checked at any time:

```powershell
cargo run -- doctor        # install root, stdlib, clang, plus a real compile+run
powershell -ExecutionPolicy Bypass -File dist\package.ps1   # build the Setup.exe
```

See [`docs/install.md`](docs/install.md) for the user-facing guide.

## Build and test

```powershell
cargo build                                    # build the `aoxn` compiler
cargo test                                     # compiler tests (platform-gated where needed)
cargo test --test pipeline recursion_fib       # a single test
cargo run -- run examples\hello.ax             # compile + run a program
cargo run -- build examples\fib.ax -o fib.exe  # emit a standalone executable
cargo run -- c examples\fib.ax                 # print the generated C text
cargo run -- run bad.ax --json                 # diagnostics as JSON
cargo run -- run selfhost\lex_demo.ax          # the Aoxn-written lexer
cargo run -- run selfhost\driver_self_demo.ax  # the fixed point: Aoxn compiles the compiler
```

For package-manager tests also run `cargo test -p aoxn-pkg --locked`.
`run_pkg_tests.sh` is a historical Smart App Control workaround, not a
reliable way to refresh the executable hash or bypass SAC; if policy blocks
a binary, diagnose the policy failure rather than assuming the script fixes it.
IDE tests are separate: `cargo test --manifest-path ide/src-tauri/Cargo.toml`,
`pnpm --dir ide test`, and `pnpm --dir ide typecheck`.

`cargo test` compiles `.ax` sources to executables and runs them, so a full
suite run is slower than a typical Rust crate. While iterating on something big,
`--O1` can reduce clang compile time; measure on the actual workload instead
of treating the historical LLVM-pipeline measurements as a current guarantee.
O3 stays the default; performance claims need a measured `clang -O3` baseline.

Debugging switches (all environment variables, all verified in the source):

| Variable | Effect |
|---|---|
| `AOXN_DUMP_C=1` | dump the generated C text to stderr |
| `AOXN_TIME=1` | per-stage wall clock (lex / parse / resolve / typecheck / codegen / link) |
| `AOXN_TC_TRACE=1` | per-function typecheck markers on stderr |
| `AOXN_CPU=native` | same as `--cpu native` (host SIMD; off by default so output stays reproducible) |
| `AOXN_NO_CACHE=1`, `AOXN_CACHE_DIR=<dir>` | disable / relocate the shared `run` / `build` executable cache |
| `AOXN_CLANG` | override the clang path used for the C compile and final link |

## Repository layout

| Path | Contents |
|---|---|
| `src/lexer.rs` | tokens, line/col, `NEWLINE`/`INDENT`/`DEDENT` (Python-style layout) |
| `src/parser.rs`, `src/ast.rs` | recursive-descent parser → AST |
| `src/typecheck.rs` | strict type rules, `FnSig` table, generic monomorphization |
| `src/codegen_c.rs` | the C-emitting backend: ISO C99 text → `clang -c` → object file (the only backend since v0.29.0) |
| `src/lib.rs`, `src/files.rs` | import loading, diagnostics, clang link |
| `src/main.rs` | CLI: `build` / `run` / `c`, `--json`, `-l`/`-L` |
| `stdlib/stdlib.ax` | standard library, written in Aoxn |
| `examples/*.ax` | demo programs and benchmarks |
| `selfhost/*.ax` | the compiler rewritten in Aoxn (lexer → parser → typecheck → loader → codegen → driver) |
| `tests/pipeline.rs` | the end-to-end suite |
| `docs/spec.md` | language specification and roadmap |

## Changing the language

When a change touches the grammar or the semantics, it is only done when all of
this is done in the same change:

1. **`docs/spec.md`** — the normative spec is updated (it is the contract).
2. **`src/parser.rs`** and **`src/typecheck.rs`** — parse it, then check it.
3. **`src/codegen_c.rs`** — emit it.
4. **`tests/pipeline.rs`** — a test that would fail before the change.
5. **`selfhost/*.ax`** — mirror changes used on the selfhost path, or document
   explicitly when a new feature remains Rust-only.
   Keep the supported selfhost subset explicit: stage-2 is not a full mirror
   of current language features. The fixed point compares C and objects from
   two builds of the **same selfhost emitter**, masking Windows COFF timestamps;
   direct emitter parity is a separate common-subset test. See
   [`docs/selfhost.md`](docs/selfhost.md) and
   [`docs/c-compatibility.md`](docs/c-compatibility.md). Compile-time folds
   such as `target_os()` must agree across stages. clang is required; these
   native tests can skip when it is absent.
6. **`CHANGELOG.md`** — a version bump always comes with an entry.

House rules that are deliberately strict (proposals to relax them are language
proposals, not drive-by patches):

- **No implicit `int`/`float` conversions**, no re-typing a bound name, `bool`
  conditions only, all paths must return, no unreachable code.
- **No external crates in the compiler library.** `aoxn-pkg` has its own
  dependency set pinned by `Cargo.lock`;
  the generated C is the integration surface with the outside world.
- **Codegen rests on the C mapping.** Structs map to C structs, arrays are
  wrapped in single-field structs (`typedef struct { T data[N]; }`) so they copy
  by value like the spec says, and no C standard header is ever included — the
  runtime surface uses clang `__builtin_*` forms plus the program's own
  `extern def` declarations. Before touching emission, read the module comment
  in `src/codegen_c.rs`; the observable rules are in
  [`docs/spec.md`](docs/spec.md).
- **Compiler failures surface as `internal` diagnostics, never panics.**
- Array indexing is unchecked and raw memory builtins are unsafe **by design**;
  that is documented behavior, not a bug to fix in passing.

## Code conventions

- Rust: `cargo fmt` before committing; keep the warning count clean.
- Aoxn: 4-space indentation, `#` comments (`//` is integer division), `.ax`
  extension, and no statement-terminating semicolons (`;` appears only inside
  `[T; N]` array types).
- **Never write a UTF-8 BOM into a `.ax` file.** Aoxn sources are read byte-wise,
  so `EF BB BF` is an "unexpected character" at 1:1. PowerShell 5.1's
  `-Encoding UTF8` adds one — prefer editor writes over shell heredocs, and if
  you do rewrite a `.ax` file from a script, strip the BOM.
- Prefer ASCII in test fixtures and fixture paths: the self-hosted loader opens
  files with narrow `fopen`, so non-ASCII paths fail there even though the Rust
  compiler handles them.
- Delete throwaway probe files (scratch `.ps1` / `.ax` / logs) before committing
  so they never land in history.

## Tests

`tests/pipeline.rs` is the main language end-to-end suite: it compiles `.ax` source to a
native executable and checks its stdout / exit code. Add a test whenever you
change observable behavior, and give it a name that reads like the invariant it
protects (`optimization_levels_agree_on_program_output`,
`selfhost_driver_links_hello`).

- [`tests/c_emitter_parity.rs`](tests/c_emitter_parity.rs) compares complete
  C bytes on an explicit common subset; known gaps are separate assertions.
  [`tests/c_abi.rs`](tests/c_abi.rs) links typed C helpers at O0/O3 and checks
  internal aggregate value semantics, not public C struct interop.
- Indented Aoxn sources embedded in tests go through the `dedent()` helper —
  strip the common leading whitespace before compiling.
- Prefer the platform helpers in `src/platform.rs` over literals such as
  `.exe` (some older tests still hardcode it — do not add more of the same);
  supported targets are Windows and Linux, not macOS.
- Do not weaken or delete an existing test to make a change fit. If a test
  encodes outdated behavior, change it deliberately and say so in the PR.

## Commits and pull requests

**Commit policy: per-session worktree benches and full commits.** Follow
[`AGENTS.md`](AGENTS.md): work in `D:/ailanguage-bench/<topic>` on a topic
branch, never in the shared main worktree. When a commit is requested, include
the whole **bench** working tree (after removing your throwaway probes):

```powershell
git add -A            # not `git add <file>`, not `git add .`
git commit -m "..."
git status            # verify nothing is left behind
```

This applies to source, tests, docs, `selfhost/` sources, CI config, and any new
file created in the session. Do not sweep another session's work into a commit.
When landing is requested, require a clean tracked main worktree, fast-forward
merge the bench (rebase and retest if needed), push, then remove the landed
bench/branch. Do not commit or push without explicit authorization.
The wiki is frozen; update living `docs/` instead.

Pull requests:

1. One logical change per PR; keep unrelated cleanups in a separate one.
2. Fill in [`.github/PULL_REQUEST_TEMPLATE.md`](.github/PULL_REQUEST_TEMPLATE.md)
   — the summary, the verification commands you ran, and the checklist.
3. Green CI on Windows and Linux is required before
   review. `cargo test` must pass locally too; paste the result if anything is
   platform-specific.
4. Performance claims need numbers and a method (machine, warm/cold, best of N).
   Prefer `--release` for benchmarks: the dev profile is `opt-level = 1`.
5. Expect review to push on the spec, the tests, and the fixed point. That is
   the point of the project.

## AI-assisted contributions

AI agents and automated tooling are welcome and explicitly anticipated — Aoxn's
`--json` diagnostics and strict semantics exist to make machine-generated code
verifiable.

Rules for them:

- **Disclose it** in the PR description ("generated with <tool>, reviewed by me").
- **Verify every claim.** A generated patch must actually build, actually pass
  `cargo test`, and must not invent builtins, benchmarks, or spec text.
  Run the commands; do not report what the tool said would happen.
- **Keep the diff reviewable.** No bulk reformatting, no unrelated rewrite of a
  file your change barely touches, no unreviewed dump of generated files.
- **You own the result.** If a reviewer finds a problem in AI-written code, it
  is your patch, not the tool's.

## Questions

- Language rules: [`docs/spec.md`](docs/spec.md)
- Self-hosting plan and status: [`docs/selfhost.md`](docs/selfhost.md)
- Release history: [`CHANGELOG.md`](CHANGELOG.md)
- Anything else: open a discussion, or write to **zmjsjsg3@163.com**

Security issues go to the same address, but as described in
[SECURITY.md](SECURITY.md).
