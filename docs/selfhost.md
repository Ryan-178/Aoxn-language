# Self-Hosting Feasibility Assessment

> **Historical assessment (v0.19-era), not the current implementation guide.**
> The original analysis, estimates and LLVM-C experiment below are preserved
> as historical evidence; paths such as `examples/ffi_llvm.ax` refer to that
> tree, not files required by today's checkout. Since v0.29.0 both compiler
> implementations emit C and use clang, with no LLVM-C library dependency.
>
> **Current evidence:** [`selfhost/driver.ax`](../selfhost/driver.ax) loads,
> checks and emits C via [`selfhost/codegen.ax`](../selfhost/codegen.ax).
> [`selfhost_driver_self_compiles`](../tests/pipeline.rs) builds a driver
> with Rust and with Aoxn, then compares those drivers' generated C and
> objects (masking only Windows COFF timestamps). Both drivers execute the
> **selfhost emitter**: this is cross-stage reproducibility, not a direct
> Rust-emitter/selfhost-emitter equivalence proof.
> [`tests/c_emitter_parity.rs`](../tests/c_emitter_parity.rs) separately
> compares complete C bytes from the two emitters on an explicit common
> subset and pins known gaps separately. Neither test proves all-feature
> equivalence. See [`c-compatibility.md`](c-compatibility.md) for the current
> C/ABI contract and [`platform-support.md`](platform-support.md) for
> Windows/Linux support. **The stage-2 front end now supports `None`, `T | None`,
> `dict[V]`, function pointers and `raise`/`try`/`except`** with C that matches
> `src/codegen_c.rs` byte-for-byte on the parity corpus; the fixed point and the
> direct-emitter comparison still answer different questions. The selfhost CLI
> covers `help/version/check/c/build/run/symbols/doctor`, but package commands,
> CSS assets/Tailwind and namespace/selective imports remain unsupported. The
> production CLI remains Rust. The [wiki](../wiki/Self-Hosting.md) is frozen
> historical material, not the authority for current status.

## Current implementation status (2026-11-07)

**Language features.** `selfhost/lexer.ax`, `parser.ax`, `typecheck.ax` and
`codegen.ax` now implement the v0.40.0 surface:

- `None` / `T | None`: annotation parsing, implicit `T`→`T | None` widening for
  bindings/arguments/struct fields/array elements, `is` / `is not`, branch-local
  narrowing with early-return propagation, array literal joins (`[1, None, 3]`
  is `[int | None; 3]`), and the same nullable operator rejection messages.
  `return` stays strict like the Rust checker (a bare `T` is not widened by a
  return).
- Function pointers: bare function names in value position, `fn(...) -> T`
  annotations, indirect calls, `as` casts int↔fn-ptr, `to_int(fn)`, complete C
  declarators (including function-pointer returns and parameters), and the
  original `extern def` restrictions (no fn-pointer parameters).
- `dict[V]`: annotation-only empty literal, string-keyed shared heap handle,
  read/write/delete/`dict_has`/`len`, insertion-order `for k in d`, alias
  visibility, growth; a missing key raises through the exception slot.
- `raise` / `try` / `except`: one catch-all handler with an optional `as e`
  string binding, pending-error statics, may-raise fixpoint over calls (a
  call through a function pointer counts), per-statement propagation checks,
  raises in handlers propagating outward, and the uncaught report + exit 1.
  Programs without exceptions/dicts keep their previous C byte-for-byte.

The parser also tracks token START line/column and statement positions for
diagnostics; the lexer previously recorded end columns.

**CLI.** `selfhost/cli.ax` implements:

- `help`, `version`, `check`, `c`, `build`, `run`, `symbols`, `doctor`.
- `run` uses a real argv vector (Windows CRT `_spawnvp`, POSIX
  fork/execvp) with CRT quoting on Windows, so empty strings, spaces and shell
  metacharacters survive; the child exit code is propagated. No shell is used.
- Options `-o`, `--O0..--O3`, `-l/-L`, `--cpu`, `--backend c`, `--clang-arg`,
  `-g`, `--cc-warnings`, `--json` reach codegen/link through the same argv
  channel. `AOXN_CLANG`, `AOXN_CPU`, `AOXN_CLANG_ARGS` and
  `AOXN_CC_WARNINGS` provide defaults; unsafe clang arguments are rejected.
- JSON diagnostics on stderr carry `stage`, `severity`, `code`, `file`, `line`,
  `col` and `message`, plus W001 warnings (`{"ok":...,"errors":[...],
  "warnings":[...]}`). stdout stays reserved for the C text, symbols, the build
  path, or the child program.
- `symbols` reports top-level functions/structs (text and JSON) from the
  loader's merged AST. `doctor` checks version/platform/executable/stdlib/clang
  and optionally runs a compile-only smoke test; `--no-smoke` skips it.
- A content-keyed `run` cache hashes the entry, every transitive import, the
  compiler executable and all codegen options. `AOXN_NO_CACHE`,
  `AOXN_CACHE_DIR` work. A dependency edit or an option change misses; an
  unchanged tree hits without rebuilding.
- The loader resolves `stdlib` / `stdlib/<module>` through `AOXN_STDLIB`, then
  `AOXN_HOME/lib/stdlib`, then a cwd-relative `stdlib/`.

**Known gaps (not hidden, not forwarded):**

- Package-manager commands (`pkg` and the direct aliases) are **rejected
  explicitly**; the selfhost CLI never invokes the Rust package manager.
- CSS assets, `--emit-assets` and `--tailwind` are rejected explicitly.
- Namespace bindings (`import m`, `m.f`) and selective imports are not
  implemented by the selfhost loader/parser.
- `build` is not cached (only `run`); the cache has no LRU/pruning.
- Imported symbols are reported against the entry path because the loader keeps
  no file registry; JSON positions are statement/expression approximations, not
  a full diagnostic-span model.
- Doctor lacks the installed-root/examples rows, reports no CPU architecture in
  `platform`, and its smoke test compiles/links but does not capture the child's
  output.
- `-o` is rejected by `run` (Rust accepts and ignores it); `c --json` is
  rejected (Rust emits both report and C).
- Linux: the compiler/CLI logic is portable, and the POSIX socket branch is
  implemented, but this round executed only on Windows with clang 23.1.2. No
  Linux-native result is claimed from this workspace.

**Evidence (Windows, clang 23.1.2):** `tests/c_emitter_parity.rs` compares full
C bytes for the new fixtures (nullable, dict, fn-pointer, raise/try) and the
existing corpus, and pins new rejection messages; `tests/selfhost_cli.rs`
builds stage-1 with Rust and stage-2 with stage-1, then differentially exercises
arguments, options, JSON, symbols, doctor, cache invalidation and output paths;
`selfhost_driver_self_compiles` remains the independent fixed point. stdlib CSV
(`tests/csv.rs`, including a Python differential), random (`tests/random.rs`) and
socket (`tests/socket.rs`, real loopback) also build/run through the selfhost
CLI where applicable. Final Windows run: **357 root + 105 package = 462 passed,
0 failed/ignored** (clang 23.1.2, full log `target/final-test.log`).
`cargo check --workspace --tests --target x86_64-unknown-linux-gnu` passed;
Linux native execution was not run this round.

## Original assessment (historical v0.19-era)

All sections below preserve the original assumptions, estimates and LLVM-era
progress descriptions; their uses of "today", "now" and "remaining" refer to that
historical assessment, not the unreleased batch or current supported surface.

> Can Aoxn be rewritten in Aoxn? Short answer: **yes — and stages 1–3 are
> underway**: the Aoxn lexer, parser and type checker written in Aoxn live in
> `selfhost/` (verified by exact token-stream / AST-dump tests and by
> checking the entire `stdlib.ax`), and generics are monomorphized just like
> in the Rust compiler. The riskiest assumption was validated first by a
> working proof of concept (`examples/ffi_llvm.ax`): an Aoxn program drives
> the LLVM-C API through `extern def` and emits real IR.

## 1. Evidence: the FFI proof of concept

`examples/ffi_llvm.ax` calls 12 LLVM-C functions from plain Aoxn code and
produces:

```llvm
define i64 @answer() {
entry:
  ret i64 42          ; LLVMBuildAdd(20, 22) 鈥?O3 constant-folded
}
```

Key facts established by this experiment:

- **Pointers work as `int`**: LLVM handles (`LLVMContextRef`, `TypeRef`,
  ...) are 8-byte values; Aoxn `int` is i64 鈥?ABI-identical on x86-64. No
  pointer type is needed to *hold* opaque handles.
- **Strings work as `char*`**: Aoxn string literals/heap strings are
  NUL-terminated byte buffers 鈥?exactly what the C API expects.
- **Any C library can be linked**: `-l LLVM-C -L <dir>` flags pass through
  to clang (v0.8).
- Scalable pattern: the entire LLVM-C surface the compiler needs is
  addressable with `extern def` + int/string parameters.

## 2. What the compiler needs vs what Aoxn has

| Requirement (Rust impl today) | Aoxn now | Gap |
|---|---|---|
| Unicode/ASCII scanning, char classes | int arithmetic, comparisons | none |
| Token vector (grow) | fixed arrays only | **dynamic arrays** |
| Token/AST enums (`enum Tok { ... }`) | no sum types | **tagged unions or enums** |
| Structs with payloads | structs | works (tagged-union style) |
| HashMap for symbol tables | none | dynamic arrays + linear/probing |
| String building (lexer) | `+` concat, no indexing | **string indexing / char** |
| Mutating buffers (lexer, codegen strings) | immutable strings | **mutable byte buffers** |
| LLVM-C FFI | `extern def` (int/string) | proven 鉁?|
| Spawn clang for linking | none | `extern def system(...)` or CreateProcess |
| Read source files | none | `fopen/fread` via FFI (needs ptr-as-int) |
| Generics for containers | 鉁?monomorphized | none |
| Modules, file loading | 鉁?imports | none |
| Diagnostics w/ files, JSON output | 鉁?| none |

## 3. Gaps, ranked

### B1 鈥?dynamic arrays (blocker)
A growable `Vec[T]` in the stdlib, built on `malloc`/`realloc` via FFI.
Generics already exist, so `Vec[T]` is a stdlib artifact, not a compiler
feature. Needs: pointer-holding (int is fine), malloc/realloc/free externs,
and **addressability** 鈥?growing a vec must write back its pointer:
`v = vec_push(v, item)` (functional style, value semantics preserved).
Effort: ~150 lines of stdlib. Unblocks token vectors, AST nodes, scopes.

### B2 鈥?sum types or tagged-union ergonomics (blocker for sane AST)
The AST is enums all the way down (`Expr::Binary{op,lhs,rhs}`). Without sum
types, use the C pattern:

```Aoxn
struct Expr:
    tag: int          # EXPR_INT, EXPR_VAR, EXPR_BINARY, ...
    ival: int
    sval: string
    lhs: int          # indices into an arena instead of pointers
    rhs: int
```

Arena allocation (`Vec[Expr]` + integer indices) sidesteps pointers entirely
and is *simpler* than Rust's ownership model for a self-hosted compiler.
Alternative: add real `enum`/`match` to the language (bigger language
change, better long-term). Recommendation: **arena + tagged structs first,
`match` syntax later**.

### B3 鈥?string indexing / mutable byte buffers (major)
The lexer needs `s[i]`, `s[i] = c`, char鈫攊nt. Options:
- stdlib functions over malloc'd buffers (`buf_get`, `buf_set`, `buf_len`)
  using int-as-pointer FFI 鈥?no language change;
- or first-class `char` type + indexing (language change, on roadmap anyway).
Recommendation: buffer functions in stdlib now, `char` later.

### B4 鈥?file I/O + process spawn (major, small)
`fopen/fread/fclose` and `system`/`CreateProcess` via `extern def` 鈥?all
pointer-as-int friendly. ~40 lines of stdlib. Unblocks reading sources and
invoking clang.

### M1 鈥?unsigned/byte types (minor)
i64 covers char-classification and sizes; `u8` is a nicety, not a need.

### M2 鈥?compile-time evaluation (not needed)
The Rust compiler does no comptime; self-hosting doesn't require it.

## 4. Bootstrap plan

```
stage 0  axonc.rust     current compiler (the crutch)
stage 1  axonc.axon     compiler written in Aoxn
         axon build axonc.ax -l LLVM-C -L <llvm>\lib   → stage1.exe
stage 2  stage1.exe compiles axonc.ax   → stage2.exe
         verify: stage1 and stage2 produce identical IR for the test suite
stage 3  CI keeps the fixed point green: stage2 builds axonc.ax, outputs match
```

Progress: **stages 1–3 are complete** — the lexer (`selfhost/lexer.ax`), the
parser (`selfhost/parser.ax`) and the type checker (`selfhost/typecheck.ax`,
with generic monomorphization) run green in CI, including a full check of
`stdlib.ax`. Multi-file imports are resolved in Aoxn too (`selfhost/load.ax`).
Stage 4: `selfhost/codegen.ax` emits native objects for int/float/bool/string
and struct programs through LLVM-C (print/len/str, f-strings, value-semantics
structs), and `selfhost/driver.ax` closes the loop — source file -> executable
via clang. Remaining: arrays, then broader codegen coverage.

The Rust compiler remains the bootstrap crutch until stage 2 is stable, then
becomes a test oracle only. The self-hosted compiler does NOT need to link
LLVM-C via the C API forever — it can reuse the same emission path
(`LLVMTargetMachineEmitToFile`) since it talks to the same DLL.

## 5. Scale estimate

| Component | Rust today | Aoxn estimate |
|---|---|---|
| lexer.rs | ~700 | ~900 (tagged structs + buffer fns) |
| parser.rs | ~640 | ~800 |
| typecheck.rs | ~1450 | ~1800 (monomorphizer included) |
| codegen.rs | ~1100 | ~1400 (LLVM-C FFI calls) |
| main/lib (CLI, link) | ~350 | ~400 |
| **total** | **~4200 Rust** | **~5300 Aoxn** |

At the current velocity (one feature-milestone per session, each with full
test coverage), the plan is:

1. **v0.9**: Vec[T] + buffer/string utils + file IO + process spawn (stdlib
   work, no language changes except maybe `char`).
2. **v1.0**: arena-tagged-union discipline proven by writing a REAL tool in
   Aoxn that parses Aoxn syntax (the stage-1 lexer only) 鈥?the dress
   rehearsal.
3. **v1.1鈥搗1.3**: parser 鈫?typecheck 鈫?codegen in Aoxn, ported
   component-by-component, each keeping the 75-test pipeline green from both
   compilers.
4. **v1.4**: fixed-point CI (stage2 == stage1), self-hosting achieved.

## 6. Risks

- **Speed of the Aoxn compiler compiled by itself**: the pipeline is
  dominated by LLVM O3 (a C library), not by our Rust code 鈥?the risk is
  small; the interpreter-free design helps.
- **Value semantics copying large AST nodes**: arena + indices (B2) avoids
  big copies by design 鈥?this is why the arena is the recommended pattern.
- **Debugging without a debugger story**: Aoxn diagnostics must be excellent
  (they are, `--json`) because stage-1 bugs will surface as Aoxn-side
  panics; mitigate with a `assert(cond, msg)` builtin early.
- **Language churn during porting**: freeze language changes once stage-1
  work starts; the compiler being ported must not chase a moving target.

## 7. Verdict

Feasible. The two genuinely new compiler features are dynamic arrays (stdlib)
and a disciplined arena/tagged-union style (no language change required);
everything else the pipeline needs 鈥?FFI, generics, modules, file-aware
diagnostics 鈥?already exists and the LLVM-C path is proven end-to-end by a
working example in the repository.

