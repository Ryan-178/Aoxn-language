# C compatibility and emitter parity

## Scope

Aoxn emits ISO C99 and invokes external clang. This is not source compatibility
with C and does not make every Aoxn `extern def` a correct declaration for an
arbitrary C library. There is no C preprocessor, header importer, or automatic
translation of C types. The compiler library remains dependency-free.

This document separates three independent claims:

1. **Emitter parity:** Rust `src/codegen_c.rs` and Aoxn
   `selfhost/codegen.ax` emit exactly the same C for explicitly tested inputs.
2. **Aoxn internal ABI:** generated Aoxn functions pass and return their own
   values correctly, including aggregate copies and indirect calls.
3. **External C ABI:** an Aoxn declaration matches a separately compiled C
   function with the deliberately chosen signature.

Passing one does not establish the other two. A finite fixture corpus is not a
proof of equivalence for every program.

## Mapping to C

| Aoxn type | Generated C representation | Boundary |
|---|---|---|
| `int` | `long long` | Signed 64-bit on the supported targets; not C `int` or LP64 `long` |
| `float` | `double` | No implicit conversion from `int` |
| `bool` | `int` | Do not assume C `_Bool` is interchangeable in arbitrary signatures |
| `string` | `char *` | Immutable by language convention, NUL-terminated bytes |
| struct | Generated C struct | Aoxn copies by value; internal calls use explicit aggregate conventions |
| `[T; N]` | C struct containing `T data[N]` | Length is in the Aoxn type; assignment copies the array |
| `fn(...) -> ...` | C function pointer | Signature must match; not a closure or captured environment |
| `T \| None` | `struct ax_opt_<T> { T value; int has; }` | Passing/returning follows the aggregate convention; `None` is a zero value with `has = 0` |
| `dict[V]` | `struct ax_dict_<V>*` heap handle | Copies SHARE the map; never treat it as a by-value struct from C |
| `None` value | `long long 0` before carrier coercion | Only meaningful inside an `ax_opt_*` carrier |
| raw address | Integer handle, commonly `long long` | Caller owns width, lifetime, alignment and API contract obligations |

Generated aggregate calling conventions are **internal Aoxn ABI**, not a promise
that an arbitrary C struct-by-value function can be declared with `extern def`.
Extern signature checking is not a C-header ABI validator. In particular,
aggregate extern declarations are not uniformly rejected by this revision;
acceptance does not establish arbitrary C struct-by-value interoperability.
Function-pointer
parameters on externs are rejected; APIs accepting callback addresses may be
called using the documented integer-address escape hatch on an appropriate
platform, but this is not general header-level ABI compatibility.

An incorrectly declared C function may compile and link and still have undefined
behavior. A C `int` return is not an Aoxn 64-bit `int` return; negative values
from system APIs may require an API-specific wrapper. Variadic functions are not
supported by fixed-arity `extern def`; do not declare `printf`/`snprintf` as an
ordinary Aoxn extern to format floating-point arguments.

## Intentional language differences

- Conditions require `bool`; integer/float conversions are explicit.
- Structs and fixed arrays copy on assignment, parameter passing and return.
- Strings are immutable bytes, not C-owned mutable arrays or Unicode characters.
  String indexing/slicing clamps bounds; fixed-array indexing remains unchecked.
- Nullable values, exceptions, dictionaries and namespace resolution are language
  constructs, not transparent aliases of C syntax.
- Dict copies share a heap handle. A growable `Vec` must be written back after
  operations that can reallocate. These are different contracts.
- Signed overflow, invalid array indexing, invalid raw-memory access and invalid
  FFI use are not made safe by compiling through C. Compiling/executing is not a
  sandbox. String concatenation intentionally leaks; there is no GC guarantee.
- Expression evaluation order is not generally pinned beyond documented
  short-circuit/control-flow rules; do not infer Python evaluation order from
  Python-style surface syntax.

## Parity boundary

The original `selfhost_driver_self_compiles` fixed-point test builds a driver in
successive compiler stages and compares that driver's C/object products. Both
products are emitted through `selfhost/codegen.ax`. It remains valuable, but it
does not directly compare the Rust emitter with the self-hosted emitter.

Direct parity tests must invoke Rust `compile_paths_to_c` and self-hosted
`emit_c` for the **same input**, compare full bytes without whitespace/sorting
normalization, and retain the stage fixed-point test separately. A driver built
by Rust that executes `selfhost/codegen.ax` is a real comparison of the two
emitter implementations; it is not an additional stage-2 bootstrap proof.

The self-hosted compiler is still a subset. Do not include unsupported syntax in
an allegedly common-subset corpus or silently skip rejected cases. The current
parity corpus now covers `None`/`T | None` (widening, `is`/`is not`, branch
narrowing, mixed array literals), `dict[V]` (runtime helpers, aliasing, iteration,
missing-key raise), function pointers (values, annotations, indirect calls,
int↔fn casts, fn-pointer returns) and `raise`/`try`/`except` (pending slot,
propagation checks, handler re-raise, uncaught exit). Each fixture's C is compared
byte-for-byte and failure-message fixtures are checked separately. Default
parameters, namespace bindings and selective imports remain unimplemented in the
self-hosted front end; selective imports are explicitly refused. String indexing /
slices and several v0.51-era numeric conveniences also remain outside the
self-hosted subset unless a fixture proves otherwise. Embedded NUL escapes and
noncanonical float spellings must not be assumed to produce identical C.
Known-gap regression cases are not parity successes.
`argc()`, `arg(i)`, `exit(code)` and `assert(cond[, message])` use the same
argv-excluding count, zero-based access and conditional C wrapper as Rust; `main`
stays restricted to no parameters and an `int` or `void` return.

The selfhost CLI now implements `help`, `version`, `check`, `c`, `build`, `run`,
`symbols` and `doctor`, with `run` using an argv vector and propagating the child
exit code. Emitters and link/compile invocations use argv, not a shell, and
Windows arguments receive CRT quoting. JSON reports (success and error) go to
stderr; stdout is the C text, symbols, build path or child output. Package
commands, CSS assets / `--emit-assets` / `--tailwind`, namespace/selective
imports, build caching, the full doctor row set, and a Linux-native run this round
remain known gaps; they are documented in `docs/selfhost.md` and are not parity
successes. Use wildcard imports and ASCII fixture paths; the self-hosted loader
still uses narrow `fopen`, and imported symbol file attribution is the entry path
because it keeps no file registry.

## Reproducibility and verification rules

- Optimization flags select clang optimization, not different C source.
- Generated names derive from names, source identities or counters, never heap
  addresses. Reproducibility tests must cross process boundaries.
- C parity is an exact byte assertion, not merely equal stdout or normalized
  text. Report the first differing byte and preserve enough context to diagnose.
- External ABI tests compile a helper with explicit C signatures, then link and
  execute Aoxn callers. System API accidents are not substitutes for this test.
- Aggregate and function-pointer tests check runtime values and copy isolation,
  not just that emitted C compiles.
- Compile-fail tests pin diagnostics for unsupported extern shapes. These do not
  prove detection of arbitrary mismatched declarations in external libraries.
- If clang is unavailable, a reported skipped integration test is not successful
  ABI or emitter validation. Select `AOXN_CLANG` explicitly for validation runs.
  Main CI runs both targets in a named Windows/Linux step with
  `AOXN_TEST_REQUIRE_CLANG=1`, so missing clang fails that gate instead of
  skipping. Without the opt-in variable, local test behavior is unchanged.
- Windows execution and Linux Rust cross-checking are separate evidence. A Rust
  cross-check does not demonstrate Linux native linkage or runtime behavior.

## Implemented regression coverage

`tests/c_emitter_parity.rs` has a full-byte parity test over nineteen comparison
inputs: nullable widening/narrowing and mixed array literals, dict aliasing /
iteration / missing-key helper text, function-pointer values and fn-pointer
returns, raise/try/except propagation, numeric conversions, argv, exit,
assertions, a shared-arena f-string growth regression; plus the original
field-augmented-assignment, reverse-order extern, recursive/control-flow,
struct/array/repetition, strings/float/f-string and stdlib/generic/Vec fixtures.
Selfhost orders extern, struct roots and array typedef roots like the Rust
emitter, while keeping dependency-first definitions and parallel table indices.
The index ordering currently uses insertion sort; it makes no new scaling or
performance claim.

A separate known-gap test pins three current boundaries. Selfhost rejects
indexed augmented assignment with an exact diagnostic and no C output.
Embedded NUL and noncanonical `3.2500` spelling produce precisely asserted C
differences. These are not parity passes, ignored tests, or permissions for
arbitrary drift. When fixed, promote the corresponding fixture into the parity
corpus.

`tests/c_abi.rs` adds four tests. Three native tests run at both O0 and O3:
separately compiled C scalar helpers and 12 mixed arguments, internal struct/array
copy isolation, and indirect scalar calls. Coverage includes ordinary Aoxn and
extern C factories returning function pointers, zero-argument callbacks,
function-pointer parameters and integer-address round trips. The fourth test
checks five illegal extern shapes without clang: extern main, void parameter,
function-pointer parameter, default parameter and generic extern.

These tests exposed invalid C for functions returning function pointers. Return
prototypes and definitions now feed the complete function declarator through
`c_decl`, including externs, rather than prefixing the anonymous return type.
Ordinary scalar declaration formatting is unchanged.

## Validation record

Language/CLI/stdlib batch, 2026-11-07, Windows with clang 23.1.2:
`cargo test -p aoxn` with `AOXN_TEST_REQUIRE_CLANG=1` passed **357/357**
(complete log `target/final-test.log`); package tests through
`bash run_pkg_tests.sh` passed **105/105** (462 combined, no failures or
ignores). Nineteen full-C comparison inputs, 22 rejection cases and three
known-gap cases are distinct categories. The original stage fixed point, all
four ABI tests, both CLI stages, real socket loopback (Rust and selfhost
builds), random cross-process vectors, the Python `csv` differential and the two
stage CLI differential all ran. `cargo check --workspace --tests --target
x86_64-unknown-linux-gnu` passed; no Linux native runtime, IDE or installer
release verification is implied. Reruns are not added to the counts.

The earlier CLI increment record (2026-11-06) remains historical evidence for
that bounded surface:
`cargo test -p aoxn --locked --no-fail-fast` with
`AOXN_TEST_REQUIRE_CLANG=1` passed **350/350**; package tests through
`bash run_pkg_tests.sh --locked` passed **105/105** (455 combined, no failures or
ignores). All four ABI tests, both parity targets, the original stage fixed
point and the new argv-driven two-stage CLI test ran. Equal-C fixtures total 14;
17 builtin rejection cases and five known gaps remain separate categories.
Initial root runs exposed two f-string arena regressions caused by missing
`n_line` subparser transfer; both were fixed before the successful full rerun.
Python checker tests passed 11/11. Linux Rust workspace/test cross-check passed;
no local Linux native runtime, IDE or installer release verification is implied.

CI-gate follow-up, 2026-11-01: the exact explicit command
`cargo test -p aoxn --locked --test c_emitter_parity --test c_abi` passed with
`AOXN_TEST_REQUIRE_CLANG=1` on Windows: **4 ABI + 2 parity tests, 0 failed or
ignored**. Both workflow jobs contain that named step. Local runs without the
variable keep the previous missing-clang skip behavior; the required CI path
uses assertions before either early return.

The earlier compatibility validation below remains the record for the emitter
and ABI implementation itself:

Validated on Windows x86_64 with Rust 1.94.1 and explicitly selected clang
23.1.2 (`AOXN_CLANG=E:/llvm/bin/clang.exe`), on the `c-parity-abi` worktree
based on `8f90aca`. Versions remain compiler 0.55.0 / package crate 0.42.0;
these are uncommitted/unreleased changes, not a new release claim.

- Final `cargo test --no-fail-fast`: **349 passed, 0 failed, 0 ignored**.
  Includes 198 pipeline tests, both new parity tests and all four ABI tests.
  No compiler warnings in this final run. The native ABI cases actually ran at
  O0/O3; no missing-clang early return was used.
- Independent package tests via `bash run_pkg_tests.sh`: **104 passed, 0 failed,
  0 ignored**. Combined root + package count: **453** (do not add reruns).
- Final standalone parity rerun: **2/2**. Six equal-C fixtures and five known-gap
  fixtures are distinct categories, not eleven equal-C fixtures.
- Standalone original `selfhost_driver_self_compiles`: **1/1**; it also passed
  inside the final complete suite. Existing cross-process C determinism tests
  passed in that suite.
- `cargo check --workspace --tests --target x86_64-unknown-linux-gnu`: passed
  against the final test sources. This is Rust conditional-compilation evidence,
  not Linux native C linkage/runtime.
- `git diff --check`: passed. IDE tests, Linux native runtime and full installer
  packaging were not run. No new security audit or performance benchmark is claimed.

An intermediate targeted rerun failed because the initial known-gap table
incorrectly expected field `+=` rejection (expected exit 1, observed 0).
Investigation compared its complete C and found parity; it was promoted to the
normal corpus. Indexed `+=` was separately executed and still rejects with
`missing expression`. The corrected final suite passed; the failed expectation
was not bypassed or marked ignored.

Reproduction (from the bench root, using an existing clang):

```powershell
$env:AOXN_CLANG = "E:/llvm/bin/clang.exe"
$env:AOXN_TEST_REQUIRE_CLANG = "1"
cargo test --no-fail-fast
cargo test --test c_abi --test c_emitter_parity -- --nocapture
cargo test --test pipeline selfhost_driver_self_compiles -- --exact
bash run_pkg_tests.sh
cargo check --workspace --tests --target x86_64-unknown-linux-gnu
```

## Deferred work

No new support is introduced here for C source import, arbitrary struct-by-value
externs, variadic externs, `sizeof`, taking a struct address, general references,
closures or GC. Porting newer Rust-only language features into selfhost is a
separate project. Documentation cleanup outside this compatibility boundary and
changes to the frozen `wiki/` are not part of this work.

## Evidence entry points

- `src/codegen_c.rs`: C representations, declarations and code emission.
- `src/typecheck.rs`: supported extern signatures and function-pointer typing.
- `selfhost/codegen.ax`, `selfhost/driver.ax`: self-host emitter and `emit_c`.
- `tests/pipeline.rs`: existing fixed point, language and determinism tests.
- `docs/spec.md`, `AGENTS.md`: language rules and engineering invariants.
