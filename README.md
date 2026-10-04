# Aoxn

<img src="icons/mark.svg" width="96" alt="Aoxn" align="right">

**Aoxn** is an AI-native, statically typed, ahead-of-time compiled programming
language. Python-style syntax on the surface, C++-class native performance
underneath: programs lower to ISO C and compile with clang (O3) straight to
machine code — the compiler itself carries no LLVM dependency.

```Aoxn
def sort[T, N](arr: [T; N]) -> [T; N]:      # generic, monomorphized
    result = arr
    for i in range(N):
        for j in range(N - 1 - i):
            if result[j] > result[j + 1]:
                t = result[j]
                result[j] = result[j + 1]
                result[j + 1] = t
    return result

def main() -> int:
    nums = [5, 3, 8, 1]
    print(f"sorted: {sort(nums)[0]}")   # sorted: 1
    print(sort(["pear", "apple"])[0])   # apple
    return 0
```

---

# English

## Why Aoxn

- **Python-style syntax** — indentation blocks, `def` / `elif` / `pass`,
  `#` comments, `x = 5` type inference, `and` / `or` / `not`, `[0] * n`,
  augmented assignment (`x += 1`, `arr[0] += 10`, `p.f += 1`), `//`,
  unary `+`, and chained comparison (`0 <= x < n`)
- **Native speed** — generated C compiled by `clang -O3`; measured at parity
  with hand-written C (benchmarks below)
- **Generics** — `def sort[T, N](arr: [T; N])` monomorphized per call site;
  no boxing, no runtime overhead
- **AI-native tooling** — every diagnostic can be emitted as structured JSON
  (`--json`), the generated C is dumpable (`aoxn c`), and the semantics are
  deliberately strict and deterministic so AI-generated code is verifiable
- **Value semantics** — arrays, structs, and strings copy by value; no hidden
  references; array indexing is unchecked, C-style

## Quick start

### Install (one file, Windows)

Download **`Aoxn-0.40.1-Setup.exe`** from the
[releases page](https://github.com/AlonechatWorkspace/Aoxn-language/releases)
and double-click it. That single executable carries the compiler, the
standard library, the UI toolkit and the examples — nothing else to download,
no archive to unpack:

```console
Aoxn Setup
  [======                ]  Installing…
    23 files installed
  ==> Adding aoxn to your PATH
  ==> Checking the C toolchain (clang)
    clang: C:\Program Files\LLVM\bin\clang.exe
  ==> Checking the installation with 'aoxn doctor'
    [ok  ] install root C:\Users\you\AppData\Local\aoxn
    [ok  ] stdlib       C:\Users\you\AppData\Local\aoxn\lib\stdlib
    [ok  ] smoke test   ok  (aoxn-doctor-ok)
  [Finish]
```

Unpack-free, registry-light: everything lands in `%LOCALAPPDATA%\aoxn`
(`bin\aoxn.exe`, `lib\stdlib\`, `examples\`), `aoxn` goes on your PATH, and the
**Finish** button only lights up once the toolchain really works — the
installer compiles and runs a test program as its own check. Open a new
terminal afterwards, then:

```powershell
aoxn run examples\hello.ax
hello, Aoxn
aoxn doctor          # verify any install, any time
aoxn doctor --json   # same report, for scripts and agents
```

Aoxn targets **Windows** — one platform, one installer, one CI job, one UI
backend (Win32/GDI). The single external prerequisite is **clang** (the
compiler lowers to C and hands it over); the installer provisions LLVM through
`winget` when it is missing, and linking uses the MSVC Build Tools, which
clang finds automatically. Full guide — scripted installs, `-Prefix`,
uninstall, troubleshooting: [`docs/install.md`](docs/install.md).

With an install in place the standard library resolves **by name** from any
directory — no path back into the toolchain:

```Aoxn
import * from "stdlib"

def main() -> int:
    print(isqrt(99))
    return 0
```

### From source

Prerequisites: Rust (msvc host), clang, MSVC Build Tools. Since v0.29.0 the
compiler carries no LLVM dependency — `cargo build` needs only the Rust
toolchain, and compiling programs needs only clang. See
[`docs/platform-support.md`](docs/platform-support.md).

```powershell
cargo build
cargo run -- run examples\hello.ax
```

Release artifacts are attached to the
[releases page](https://github.com/AlonechatWorkspace/Aoxn-language/releases):
one `Aoxn-<version>-Setup.exe` per release.

Emit a standalone native executable:

```powershell
cargo run -- build examples\fib.ax -o fib.exe
.\fib.exe
```

More: `cargo run -- c examples\fib.ax` prints the generated C text (the
compiler's only backend since v0.29.0 — the LLVM backend was removed, see
[`docs/llvm-independence-report.md`](docs/llvm-independence-report.md));
`cargo run -- check examples\fib.ax` type-checks a file and prints only the
diagnostics (no C text, no clang — this is what the IDE's Check button
drives); `cargo run -- run examples\primes.ax --json` emits machine-readable
diagnostics; `--cpu native` targets the host CPU (AVX2 & co.) for maximum
speed — the default generic CPU keeps compiled output reproducible across
machines.

Since v0.35.0 the compiler can also describe what a program declares,
which is what the IDE's outline, "go to definition" and symbol search read:

```powershell
cargo run -- symbols examples\ui_gallery.ax            # a readable table
cargo run -- symbols examples\ui_gallery.ax --json     # for editors and agents
```

It walks the AST after import resolution, so the listing covers the entry
file **and every module it imports** — each `def` and `struct` with its
parameter types, return type, `extern` marking and a source range. It stops
after the parser: no typecheck, no codegen, no clang, which is what makes
it cheap enough for an editor to re-run on every file switch.

Optimization levels, for when compile time matters more than runtime speed
(the level selects the clang `-O` used to compile the generated C):

```powershell
cargo run -- run examples\fib.ax --O1     # clang -O1: faster compile,
                                          # O3 stays the default
cargo run -- build examples\fib.ax --O0   # clang -O0
```

`aoxn run` and `aoxn build` cache their output by program content hash +
compiler + options (`target/cache`), so re-running or re-building unchanged
sources skips compile and link entirely (measured ~1.1s → ~85ms for
`examples/hello.ax`). Set `AOXN_NO_CACHE=1` to disable it or
`AOXN_CACHE_DIR=<dir>` to relocate it.

## Language tour

```Aoxn
# arrays, structs, strings — all value types
struct Particle:
    x: float
    y: float
    mass: float

def energy(p: Particle) -> float:
    return (p.x * p.x + p.y * p.y) * p.mass

def main() -> int:
    ps: [Particle; 1000] = [Particle(x=1.5, y=0.5, mass=2.0)] * 1000

    total = 0.0
    i = 0
    while i < len(ps):
        total = total + energy(ps[i])
        i = i + 1

    print(total)
    print("a" < "b")     # true — strings compare byte-wise
    return 0
```

Types are strict and every expression is checked: `int`/`float` never mix
implicitly, conditions must be `bool`, array indexing is unchecked (C-style),
and every function must return a value on all paths. Strictness is a feature:
the guarantees are simple enough for a machine to reason about.

## Standard library and the UI toolkit

`stdlib/stdlib.ax` is written in Aoxn itself: generic `sort` /
`binary_search` / aggregation, a growable `Vec`, byte buffers, file IO and
process spawning.

Since v0.27.0 the stdlib ships a **Qt-flavored, immediate-mode UI toolkit**
in pure Aoxn on raw FFI; v0.29.3 brought it to **Qt grade**: layout
managers (vbox/hbox/grid with stretch), real text input with caret,
UTF-8-aware editing and **text selection** (Shift+arrows/drag, Ctrl+A/C/X/V
clipboard), a **multi-line editor**, a keyboard focus chain (Tab/Enter/
Space), a **menu bar**, **tree/table model+view** (heap `TreeModel`/
`TableModel`), **signal-slot events** without function pointers, and 20+
widgets (buttons, toggles, checkboxes, radio groups, sliders, spin boxes,
text fields, list boxes, floating combo boxes, tabs, group boxes, scroll
areas, tooltips), disabled groups, floating overlays and 16-role light/dark
themes. Widgets are plain functions called every frame and the application
owns all state, which is what fits a language without callbacks (yet).

**The toolkit is three files** — a portable core (`ui.ax`), a
platform-neutral widget layer (`ui_draw.ax`) and the Win32/GDI backend
(`ui_win.ax`) — and a program picks it up with one import:

```Aoxn
import * from "stdlib/ui_win"
```

Widgets never name a Win32 symbol: they call `plat_*` primitives that the
backend supplies, so the widget layer stays testable headlessly. Aoxn is a
Windows-only language (v0.30.0), so `ui_win.ax` is the only backend that
ships; the X11 backend and its per-OS selection table went with the other
platforms.

```Aoxn
import * from "stdlib/ui_win"

def main() -> int:
    c = ui_init("Hello", 640, 480)
    name = "world"
    while c.open:
        c = ui_frame(c)
        if c.open:
            ui_title(c, 20, 12, "Hello")
            te = ui_textbox(c, 20, 56, 240, 28, name)   # type here
            name = te.text
            ui_label(c, 20, 96, "hi " + name)
            if ui_button(c, 20, 130, 120, 34, "Close"):
                ui_close(c)
            ui_present(c)
    ui_fini(c)
    return 0
```

```powershell
cargo run -- run examples\ui_gallery.ax -l user32 -l gdi32   # full widget gallery
cargo run -- run examples\ui_demo.ax -l user32 -l gdi32      # getting started
```

Details: [`docs/ui.md`](docs/ui.md) · gallery: `examples/ui_gallery.ax` ·
tests: `tests/ui.rs`.

### The OpenAI SDK (v0.40.1)

`stdlib/openai/` gives the language a real API client in five plain-Aoxn
modules: JSON with a proper DOM, HTTPS over WinHTTP, SSE streaming, and the
core OpenAI resources — with the reference Python SDK's defaults (the same
base URL, env variable names, timeouts and retry budget):

```Aoxn
import * from "stdlib/openai/client"

def main() -> int:
    c = oa_client_env()                 # OPENAI_API_KEY, OPENAI_BASE_URL, ...
    roles = vec_new()
    contents = vec_new()
    roles = vec_push_str(roles, "user")
    contents = vec_push_str(contents, "Say hello in one word")
    r = oa_chat_create(c, "gpt-4o-mini", roles, contents)
    if not oa_resp_ok(r):
        print(oa_err_msg(r))
        return 1
    print(oa_chat_text(r))
    return 0
```

Streaming is the same shape with a pull loop: `oa_chat_stream` opens the
request, `oa_stream_next` yields one SSE `data:` payload at a time (the
`[DONE]` sentinel and connection close both end it), `oa_stream_text`
extracts `choices[0].delta.content`, `oa_stream_close` releases the
handles. Resources: `chat.completions`, `responses`, `embeddings`,
`models`, `moderations` — blocking and streaming; `oa_request` +
the JSON DOM cover anything the typed helpers do not. Errors never raise:
`oa_resp_ok` gates, `oa_err_msg` renders whichever layer failed first.
Link with `-l winhttp` (the one transport that brings TLS without a TLS
stack).

```powershell
cargo run -- run examples\openai_chat.ax -l winhttp   # live demo (needs a key)
cargo test --test openai_sdk                          # offline: pure layers + a mock OpenAI server written in Aoxn
```

Reference: [`docs/openai-sdk.md`](docs/openai-sdk.md) ·
example: `examples/openai_chat.ax` · tests: `tests/openai_sdk.rs`.

## The IDE

Aoxn ships an official editor — `ide/`, a native desktop app built on
Tauri 2 with a Next.js + Monaco workbench and a small Rust command layer:

- **Explorer** with a project tree (build/dependency directories skipped)
  and real **New file / New folder** — names are validated in the dialog,
  nesting works in one step (`src/util.ax`), and nothing can escape the
  opened folder or clobber an existing entry.
- **Monaco with an Aoxn grammar** (`#` comments, `f""` interpolation,
  keywords/types/builtins), one model per tab so undo stays per-file.
- **Diagnostics become editor markers**: driven by the compiler's
  **`--json` report** since v0.35.0, so every marker's file, line, column
  and message is the compiler's own field rather than something recovered
  from its text; the raw output still reaches the panel verbatim. Clicking
  a line in the output panel jumps to the offending line, and **saving an
  `.ax` file auto-checks it** so the squiggles follow the edits.
- **Outline, go to definition and symbol search** (v0.35.0) — all three read
  `aoxn symbols --json`, a new compiler command that exports the
  declarations of a file and **everything it imports** from the AST the
  compiler just built. The outline lists the current file's `def`s and
  `struct`s in source order; Ctrl+click jumps to a declaration across an
  `import`; Ctrl+Shift+O searches by name with the origin `file:line`
  beside each hit. Nothing here is a regex over source text — a parameter
  is never mistaken for a declaration, and an `extern def` is recognisable
  as having no body to jump into.
- **Check / Build / Run** (F7 / F6 / F5) drive the real `aoxn` binary with
  the arguments a user would type — nothing about the build is
  reimplemented, so a build inside the IDE cannot disagree with a build in
  a terminal. Check runs `aoxn check` (diagnostics only, no C dump).
- **Packages** (v0.33.0) — a sidebar view over the package manager: the
  workspace's `aoxn.json` with its dependencies and what is installed in
  `aox_modules/`, and buttons for the whitelisted verbs — Init, Add, Install,
  Update, Outdated, Tree, Audit, Why, Remove. Every command runs the real
  `aoxn pkg` in the opened folder and streams its output verbatim;
  outward-facing or global commands (publish, yank, cache, trust bootstrap,
  npm-import) are refused at the Rust gate, not hidden in the UI. `aoxn
  doctor` lives here too: the status bar's "compiler / clang not found"
  button runs it into the output panel.
- **Resolved versions in the packages panel** (v0.35.0) — `aoxn pkg list
  --json` and `aoxn pkg outdated --json`, so the panel shows what the
  lockfile actually installed (`2.1.0`, not the manifest's `^2`) with its
  scope and source registry, and marks anything with a newer version. A
  registry that cannot be reached is reported, never shown as "up to
  date".
- **Quick open** — Ctrl+P files, Ctrl+Shift+P commands, Ctrl+Shift+O symbols.

The workspace is a boundary, not a suggestion: every path arriving from the
webview is canonicalised and refused if it resolves outside the opened
folder (compared component-wise, so `C:\proj-evil` does not pass a
`C:\proj` check), there is no read-any-path command and no shell plugin.

```powershell
cd ide
pnpm install
pnpm ide:dev        # the native app, hot reload
pnpm dev            # browser preview against an in-memory fixture —
                    # layout work without a native rebuild
pnpm test           # frontend tests (node:test)
pnpm test:rust      # the Rust command layer
```

Details: [`docs/ide.md`](docs/ide.md).

## Performance

Same-algorithm comparisons against `clang -O3` on the same machine (warm
runs, best of 3):

| Benchmark | Scale | Aoxn | clang C++ |
|---|---|---|---|
| Loop sum | 2×10⁸ iterations | ~15 ms | ~23 ms |
| Array fill + scan | 2×10⁸ reads | 86 ms | 99 ms |
| Struct copies (by value) | 7.5×10⁷ copies | 237 ms | 206 ms |

Native code is native code — Aoxn sits within noise of clang.

Web servers too: the [`web/`](web/README.md) suite ships an HTTP/1.1 server
written in Aoxn and benchmarks it against the pnpm + Node.js + Next.js stack
on identical routes — it matches plain Node.js throughput at ~1/50 the p50
latency and serves 26-54x more requests than Next.js, from a single 173 KB
binary with a 5 MB RSS. Numbers: [docs/web-benchmark.md](docs/web-benchmark.md).
Since 2026-10-02 the server also speaks static files (W2): a preloaded
in-memory file table with ETag / `If-None-Match` → 304 / single-range → 206
/ unsatisfiable → 416 / `Cache-Control`, cross-checked against the Node
reference server by the parity suite.

## Self-hosting

The compiler is rewritten in Aoxn and has reached a **fixed point**: the
Aoxn-written driver compiles the whole self-hosting compiler
(~7k lines: lexer, parser, typecheck with monomorphization, loader,
C-text codegen, driver) into a stage-2 binary whose generated C **and emitted
object files are byte-identical** to the Rust-built compiler's for the same
program. It also compiles the real stdlib and the full `examples/` suite. See
[`docs/selfhost.md`](docs/selfhost.md) and the
[wiki's Self-Hosting page](wiki/Self-Hosting.md).

## Project layout

| Path | Contents |
|---|---|
| `src/` | the compiler: lexer → parser → typecheck → C-text codegen → clang link |
| `src/codegen_c.rs` | the C-emitting backend (the only backend since v0.29.0) |
| `src/paths.rs` | install-layout discovery (`AOXN_HOME` / stdlib / bundled clang) + the `aoxn doctor` report |
| `src/ts/` | the TypeScript front end (TS-M1 W1 complete: S2b type layer + S3 modules) |
| `stdlib/stdlib.ax` | the standard library, written in Aoxn itself |
| `stdlib/ui.ax`, `stdlib/ui_draw.ax`, `stdlib/ui_win.ax` | the UI toolkit v3: portable core + platform-neutral widget layer (20+ widgets) + the Win32/GDI backend |
| `stdlib/openai/` | the OpenAI SDK (v0.40.1): JSON DOM, WinHTTP transport, SSE, client — `docs/openai-sdk.md` |
| `examples/*.ax` | demo programs (hello, fib, primes, stdlib_demo, ui_demo, ui_gallery, …) |
| `dist/package.ps1`, `src/setup/` | the single-file installer: the packager and the installer stub it fills |
| `selfhost/` | the compiler rewritten in Aoxn (fixed point reached) |
| `ide/` | the Aoxn IDE: Tauri 2 + Next.js + Monaco workbench (`ide/src-tauri` is its own Cargo workspace) |
| `web/` | web benchmark suite: an HTTP server in Aoxn vs pnpm+Node.js+Next.js |
| `tests/` | end-to-end tests: compile → run → verify output, including the self-hosting fixed point (byte-identical generated C + objects) and the install layout |
| `docs/install.md` | install guide (Windows) |
| `docs/ide.md` | the IDE: architecture, security boundary, development workflow |
| `docs/spec.md` | full language specification |
| `docs/stdlib-todo.md` | the standard-library roadmap: the remaining Python modules, batched P0–P3, with the language constraints each one runs into |
| `wiki/` | bilingual (中文/English) wiki — frozen since v0.29.3; `docs/` is the living documentation |

## Testing & CI

`cargo test` runs the end-to-end suite — 244 tests in the compiler workspace
(pipeline 126, compiler unit tests 17, TypeScript front end 34, UI 9, install
layout 6, CSS assets 21, CSS assets v0.36 18, symbol export 8, OpenAI SDK 2,
installer 3)
plus the `aoxn-pkg`
crate's 94 via `bash run_pkg_tests.sh`, 338 in

total — where every
pipeline test compiles
`.ax` to an executable, runs it and asserts stdout + exit code. The suite includes the
self-hosting fixed point: the stage-1 and stage-2 compilers must emit
byte-identical C and object files for the same program (the object comparison
masks the COFF TimeDateStamp that clang stamps into every Windows object).
The IDE carries its own suites next to these: 36 Rust tests for the command
layer (`pnpm test:rust` in `ide/`) and 62 node:test cases for the frontend
(`pnpm test`), which do not run in a plain `cargo test` because
`ide/src-tauri` is deliberately excluded from the root workspace.
CI runs the whole suite on windows-latest on every push — Aoxn is a
Windows-only language (v0.30.0) — and the same job then packages the
single-file installer, installs it into a scratch prefix and runs
`aoxn doctor` plus a stdlib program, so a broken install fails the build and
not the next release. Release tags publish `Aoxn-<version>-Setup.exe` (see
[`.github/workflows/release.yml`](.github/workflows/release.yml)).

## Package management

`aoxn pkg` (and the direct aliases `aoxn init | add | remove | install |
update | outdated | list | freeze | tree | why | publish | yank | audit |
trust | cache | npm-import`) manages dependencies through `aoxn.json` +
`aoxn.lock` into `aox_modules/`, resolving via PubGrub against directory,
git, or read-only HTTP registries (beta; see
[`crates/aoxn-pkg`](crates/aoxn-pkg) and
[`docs/pkg-manager.md`](docs/pkg-manager.md)). Since v0.29.1 a bare package
import resolves its entry through the package's `aoxn.json` — `main`,
`exports` (incl. `pkg/sub` subpaths), and `types` — so an installed package
whose entry is not `index.ax` is importable:

```Aoxn
import * from "http"          # → aox_modules/http/aoxn.json `main`/`exports["."]`
import * from "http/client"   # → exports["./client"]
```

A package without a manifest falls back to the legacy `aox_modules/<name>`
directory probe (`<name>.ax` / `index.ax`).

**v0.32.0** brings the everyday features the manager was missing, measured
against pip and pnpm:

```bash
aoxn add -D harness        # devDependencies — tooling that must not ship
aoxn install --prod        # runtime only; dev-only packages are pruned
aoxn install --jobs 16     # parallel tarball downloads (default 8)
aoxn install --json        # the install report as data
aoxn list                  # what is installed: name/version/scope/source
aoxn freeze                # `name==version`, for CI baselines
aoxn audit --fix           # bump to the patched version an advisory names
```

Dev and prod resolve into **one** lockfile with a per-package `dev` flag, so
a CI job that only ships runtime code still reproduces from a lockfile that
also pins the test tooling. `"overrides": {"http": "1.4.2"}` forces a
requirement wherever the graph mentions that package.

A registry may additionally be **curated** — one repository carrying the
packages, a `trust.json` review record per package, and `advisories/`:

```bash
aoxn trust bootstrap https://github.com/AlonechatWorkspace/Aoxn-trusted-third-party-package
aoxn trust check http --tier audited   # CI gate
```

Install warns about packages a curated registry has no reviewed record for,
and says nothing for registries that make no trust claims. The trust index
is a curation signal, not a signature — the cryptographic anchor is still
the manifest hash pinned in `aoxn.lock`. Layout and schema:
[`docs/trusted-registry.md`](docs/trusted-registry.md).

The npm bridge (`aoxn npm-import`, the npm CLI as transport) imports Aoxn
packages published to any npm-compatible registry into `vendor/<name>/` as
path dependencies; plain JavaScript packages are rejected.

## CSS assets

A `.css` file you `import` is a **build asset, not source**. It is bundled
(`@import`s inlined in place), minified, fingerprinted, and embedded into the
binary — no output directory, no file layout to agree on:

```Aoxn
import * from "./style.css"

def main() -> int:
    print(styles())               # the whole bundle
    print(styles_fingerprint())   # "18603d4d4686e2cc.css"
    return 0
```

Minification removes comments and redundant whitespace, and deliberately
nothing else — no selector merging, no reordering — so the emitted CSS is a
pure function of the source.

**CSS Modules.** A `*.module.css` file has its class names scoped (seeded by
the file's own path) and generates an accessor. Its rules stay out of the
global bundle, which is what makes the scoping meaningful:

```Aoxn
import * from "./page.module.css"
    print(page_class("title"))     # -> "title_87b780ec"
```

The rewriter is context-aware: a `.` inside a string, inside an `@media`
prelude, or inside a declaration value is not a selector and is left alone.

**Tailwind** enters as pre-generated CSS, not as a dependency — it is a
plain-JavaScript npm package, which `aoxn npm-import` rejects by design, and
shelling out to its CLI would make Node a build prerequisite. Either run
`npx tailwindcss -o generated.css` and import the result, or let the compiler
generate a documented utility subset:

```sh
aoxn build app.ts --tailwind
```

It scans `class`/`className` attributes only — prose is never mined — and
names any utility it does not cover instead of dropping it silently.

**Serving assets from disk.** `--emit-assets <dir>` writes the fingerprinted
bundle, each stylesheet and every `url()` target beside the executable, with
`url()`s rewritten to the emitted names. A program finds them without knowing
any build-time path:

```Aoxn
print(asset_path(styles_fingerprint()))   # …\assets\82b4fb25….css
```

Full reference, including limits: [`docs/css-assets.md`](docs/css-assets.md).

## Status

**v0.40.1** · **Windows only** · 338 tests green
(pipeline 126 + lib 17 + assets 21 + assets_v36 18 + symbols 8 + TS 34 + UI 9 +
OpenAI SDK 2 + install 6 + setup 3 + aoxn-pkg 94; the IDE adds 36 Rust + 62
frontend tests of its own) ·
**the OpenAI SDK lands in the stdlib** (v0.40.1) — `stdlib/openai/` in five
plain-Aoxn modules (codec / JSON DOM / WinHTTP transport / SSE / client)
gives the language a real API client: chat completions, responses,
embeddings, models and moderations, blocking or streamed token-by-token,
with the reference Python SDK's defaults and env variable names; tested
offline end-to-end against a mock OpenAI server written in Aoxn itself ·
**the language grows its fourth data axis** (v0.40.0) — **function pointers**
(a bare function name in value position is its address, `fn(int) -> int`
annotations, indirect calls, `as` casts between `int` and fn-ptr for COM
vtable slots and Win32 callbacks), **`None` + `T | None` nullability** (the
only union form; `is None` / `is not None` narrow per branch in checker and
codegen alike; print and operators reject a nullable until narrowed), real
**`raise` / `try` / `except`** (the v0.39.0 "no exceptions" decision reversed
by user instruction — `raise <string>` unwinds to the nearest handler or out
of the function; the `Err`-value channel stays for failures a caller
inspects), and **`dict[V]`** — string-keyed maps with insertion-order
iteration whose missing keys raise. The dict is a **heap handle** (the
`TableModel` / `FileTable` shape): the first spelling passed the 4-word
struct by value and was unsound twice over — a callee's growth was invisible
to the caller, and the stale `len` then walked off the reallocated buffer
(correct at `-O0`, a segfault at `-O1`+; reproduced with clang on the bare
generated C). Copies now share the dict, so set/del through any of them are
seen by all ·
**the self-hosting heap corruption is root-caused and fixed (v0.39.2)** — the
struct
cycle detector threaded its DFS path as a `Vec` by value, pushed onto it and
recurred; the callee's `realloc` freed the buffer the caller still held, and
the next sibling branch walked freed memory. It was hidden by an 8-slot
initial capacity that made the first eight pushes of any path free. The fix
is write-back (`struct Cycle{hit, path}`), and `vec_push` delegates its
growth again ·
**the IDE editor caught up with the language** — the bitwise/shift operators
and the `err_*` / `out_*` / `vec_*` stdlib vocabulary now highlight in the
IDE, and the installer fixes that landed between releases have their entry ·
**two things a language needs before it can hold real data** — a way to report
failure, and a way to hold "however many" of something. A failing call returns
an `Err{code, message}` by value; because Aoxn returns one value and structs
copy on return, the payload comes back through a heap out-slot the caller owns.
And `[T; N]` being compile-time fixed, the stdlib now ships `Vec` with real
capacity management, strings as first-class elements, and `VecVec` — a vector
of vectors, the shape a document format needs. Neither required touching the
compiler. Plus the six operators Aoxn was missing — `&`, `|`, `^`, `~`, `<<`,
`>>` — int-only, like `%`, with C's precedence (in which `==` binds *tighter*
than `&`, so write `(a & b) == c`); they exist for the crypto/TLS/HTTP2 work:
**the CSS pipeline is finished**: `--emit-assets <dir>` writes fingerprinted CSS
and every `url()` target beside the executable (with `url()` rewritten to the
emitted name), `styles.title` gives typed class access, `exe_dir()` /
`asset_path()` let a relocatable program find its own assets, CSS-in-Aoxn
builds inline styles, and `--tailwind` generates a documented utility subset —
plus a fix for a TS import form that never worked
([`docs/css-assets.md`](docs/css-assets.md)) ·
**the IDE drives the package manager**: a Packages sidebar view over
`aoxn.json` / `aox_modules/` with the whitelisted `aoxn pkg` verbs (Init,
Add, Install, Update, Outdated, Tree, Audit, Why, Remove) and an `aoxn
doctor` self-check on the status bar ·
**package manager measured against pip/pnpm**: devDependencies with
`install --prod`, `overrides`, curated registries (a trust index and an
advisory database in one repository), `aoxn list`/`freeze`, parallel
downloads, and enforced minimum compiler versions ·
**the Aoxn IDE** (`ide/`): a Tauri 2 + Next.js + Monaco workbench with an
explorer (real new file/folder), per-tab undo, diagnostics as editor markers,
auto-check on save, and Check/Build/Run driving the real `aoxn` —
`aoxn check` type-checks with diagnostics only ·
**one-file install**: `Aoxn-<version>-Setup.exe` carries the compiler,
stdlib, UI toolkit and examples. Its window follows the Python installer — a
mark, a headline, one large **Install Now** button, then a progress stage and
a done page — and **nothing touches your disk until you click it**. It
downloads nothing: it unpacks, adds `aoxn` to PATH, and self-checks with
`aoxn doctor` (`-InstallClang` opts into fetching LLVM) ·
self-hosting fixed point (byte-identical generated
C + object files) · **UI toolkit v3 in the stdlib** (Qt-grade: layout
managers, text input, focus chain, 20+ widgets, floating overlays —
`examples/ui_gallery.ax`) · no LLVM dependency: the C-emitting backend is the
only backend (clang compiles it) · TS-M1 W1 complete (S2b type layer + S3
modules; the bare `import "path"` form is gone — `import * from "path"`) ·
v0.29.7: Python surface-syntax parity, batch 1 (`+= -= *= /= %=`, `//`, unary
`+`, chained comparison - all parse-time desugarings, in both the Rust and the
self-hosted compiler) · package manager W2: manifest entry resolution
(`main` / `exports` / `types`), a read-only HTTP registry backend, and the npm
bridge (`aoxn npm-import`).

See [`docs/install.md`](docs/install.md) to install,
[`docs/spec.md`](docs/spec.md) for the complete language specification and
[`CHANGELOG.md`](CHANGELOG.md) for the release history.

## License

Apache-2.0 — see [`LICENSE`](LICENSE).

---
---

# 中文

**Aoxn** 是一门 AI 原生、静态类型、提前编译的编程语言：表面是 Python 式语法，
底下是 C++ 级别的原生性能——程序降级为 ISO C 后由 clang（O3）直接编译成机器码，
编译器自身不携带任何 LLVM 依赖。

## 为什么选 Aoxn

- **Python 式语法** —— 缩进块、`def` / `elif` / `pass`、`#` 注释、
  `x = 5` 类型推断、`and` / `or` / `not`、`[0] * n`、增强赋值
  （`x += 1`、`arr[0] += 10`、`p.f += 1`）、`//`、一元 `+`、链式比较
  （`0 <= x < n`）
- **原生速度** —— 生成的 C 交给 `clang -O3` 编译，实测与手写 C 同级（基准见下）
- **泛型** —— `def sort[T, N](arr: [T; N])` 按调用点单态化，无装箱、无运行时开销
- **AI 原生工具链** —— 诊断可输出结构化 JSON（`--json`）、生成的 C 文本可
  导出（`aoxn c`），语义刻意保持严格与确定，AI 生成的代码可被机器验证
- **值语义** —— 数组、结构体、字符串按值复制，没有隐藏引用；数组索引不检查（C 风格）

## 快速上手

### 一个 exe 装全部（Windows）

从 [releases 页面](https://github.com/AlonechatWorkspace/Aoxn-language/releases)
下载 **`Aoxn-0.40.1-Setup.exe`**，双击即可。**这一个 exe 里就带着编译器、标准库、
UI 工具箱和示例程序**——不用再下载别的，也不用自己解压：

```console
Aoxn Setup
  [======                ]  Installing…
    23 files installed
  ==> Adding aoxn to your PATH
  ==> Checking the C toolchain (clang)
    clang: C:\Program Files\LLVM\bin\clang.exe
  ==> Checking the installation with 'aoxn doctor'
    [ok  ] install root C:\Users\you\AppData\Local\aoxn
    [ok  ] smoke test   ok  (aoxn-doctor-ok)
  [Finish]
```

全部落在 `%LOCALAPPDATA%\aoxn`（`bin\aoxn.exe`、`lib\stdlib\`、`examples\`），
`aoxn` 进 PATH；按钮变成 **Finish** 之前，安装程序会**真的编译并运行一个程序**
自检，通过了才算装好。装完开一个新终端：

```powershell
aoxn run examples\hello.ax
hello, Aoxn
aoxn doctor          # 随时自检
aoxn doctor --json   # 同样的信息，JSON 输出
```

Aoxn **只支持 Windows**：一个平台、一个安装包、一条 CI、一个 UI 后端
（Win32/GDI）。唯一的外部依赖是 **clang**（Aoxn 生成 C 后交给它编译链接），
安装程序会在缺失时用 `winget` 装 LLVM；Windows 链接还需要 MSVC Build Tools，
clang 会自动探测。静默安装（脚本 / CI，无需窗口）、自定义目录、卸载与排错见
[`docs/install.md`](docs/install.md)。


装好后任何目录都能按名字引用标准库，不需要写工具链里的路径：

```Aoxn
import * from "stdlib"

def main() -> int:
    print(isqrt(99))
    return 0
```

### 从源码构建

前置：Rust（msvc 主机）、clang、MSVC Build Tools。自 v0.29.0 起编译器不再依赖
LLVM——`cargo build` 只需要 Rust 工具链，编译程序只需要 clang。见
[`docs/platform-support.md`](docs/platform-support.md)。

```powershell
cargo build
cargo run -- run examples\hello.ax
```

三平台的预编译二进制发布在
[releases 页面](https://github.com/AlonechatWorkspace/Aoxn-language/releases)。

编译出独立的原生可执行文件：

```powershell
cargo run -- build examples\fib.ax -o fib.exe
.\fib.exe
```

更多：`cargo run -- c examples\fib.ax` 打印生成的 C 文本（v0.29.0 起这是唯一
后端——LLVM 后端已移除，见
[`docs/llvm-independence-report.md`](docs/llvm-independence-report.md)）；
`cargo run -- check examples\fib.ax` 只做类型检查、只输出诊断（不打印 C、
不调 clang——IDE 的 Check 按钮驱动的就是它）；
`--json` 输出机器可读诊断；`--cpu native` 针对宿主 CPU（AVX2 等）极致提速——
默认的通用 CPU 保证编译产物跨机器可复现。

编译速度优先时用优化级别（级别选择编译生成 C 所用的 clang `-O`）：

```powershell
cargo run -- run examples\fib.ax --O1     # clang -O1：编译更快；默认仍是 O3
cargo run -- build examples\fib.ax --O0   # clang -O0
```

`aoxn run` 与 `aoxn build` 按"程序内容哈希 + 编译器 + 选项"缓存产物
（`target/cache`）：重复运行/构建未改动的源码完全跳过编译与链接（实测
`examples/hello.ax` 约 1.1s → 85ms）。`AOXN_NO_CACHE=1` 关闭，`AOXN_CACHE_DIR`
迁移缓存目录。

## 语言速览

（代码示例同上方英文区：`Particle` 结构体、值语义、字符串逐字节比较。）

类型严格、每个表达式都检查：`int`/`float` 永不隐式互转，条件必须是 `bool`，
数组索引不检查（C 风格），函数所有路径必须返回。严格是特性：保证简单到机器
可以推理。

## 标准库与 UI 工具箱

`stdlib/stdlib.ax` 用 Aoxn 自己写成：泛型 `sort` / `binary_search` / 聚合、
可增长 `Vec`、字节缓冲、文件 IO 与进程调用。

自 v0.27.0 起标准库带有 **Qt 风格的立即模式 UI 工具箱**（纯 Aoxn + 原始
FFI），v0.29.3 升到 **Qt 级**：布局管理器（vbox/hbox/grid +
千分比拉伸）、真文本输入（光标、UTF-8 感知编辑、**文本选区** Shift+方向键/
拖拽、Ctrl+A/C/X/V 剪贴板）、**多行编辑器**、键盘焦点链（Tab/Enter/Space）、
**菜单栏**、**树/表格模型视图**（堆 `TreeModel`/`TableModel`）、无函数指针的
**信号槽事件**、20 余控件（按钮、切换钮、复选框、单选组、滑条、微调框、
文本框、列表框、浮层下拉框、标签页、分组框、滚动区、工具提示）、禁用态、
浮层覆盖与 16 色亮/暗主题。控件是每帧调用的普通函数，状态由应用自己
持有——这正是"暂无回调"的语言所能承载的形态（用法示例见上方英文区）。

**工具箱由三个文件组成**：可移植内核（`ui.ax`）、与平台无关的控件层
（`ui_draw.ax`）、以及 Win32/GDI 后端（`ui_win.ax`）。一行 import 即可引入：

```Aoxn
import * from "stdlib/ui_win"
```

控件层从不出现任何 Win32 符号：它只调用后端提供的 `plat_*` 原语，因此这一层
可以无窗口地测试。Aoxn 只支持 Windows（v0.30.0），所以发布的只有 `ui_win.ax`
这一个后端；X11 后端与那张按系统选后端的表格已随其它平台一起移除。

```powershell
cargo run -- run examples\ui_gallery.ax -l user32 -l gdi32   # 全控件画廊
cargo run -- run examples\ui_demo.ax -l user32 -l gdi32      # 入门示例
```


细节见 [`docs/ui.md`](docs/ui.md)；画廊 `examples/ui_gallery.ax`；测试
`tests/ui.rs`。

### OpenAI SDK（v0.40.1）

`stdlib/openai/` 用五个纯 Aoxn 模块给语言带来了真正的 API 客户端：带 DOM 的
JSON、WinHTTP 上的 HTTPS、SSE 流式，以及 OpenAI 核心资源——默认值与参考
Python SDK 一致（base URL、环境变量名、超时、重试预算都相同）：

```Aoxn
import * from "stdlib/openai/client"

def main() -> int:
    c = oa_client_env()                 # OPENAI_API_KEY、OPENAI_BASE_URL …
    roles = vec_new()
    contents = vec_new()
    roles = vec_push_str(roles, "user")
    contents = vec_push_str(contents, "Say hello in one word")
    r = oa_chat_create(c, "gpt-4o-mini", roles, contents)
    if not oa_resp_ok(r):
        print(oa_err_msg(r))
        return 1
    print(oa_chat_text(r))
    return 0
```

流式是同一形态的拉取循环：`oa_chat_stream` 打开请求，`oa_stream_next` 每次
吐出一个 SSE `data` 载荷（`[DONE]` 哨兵与连接关闭都会结束它），
`oa_stream_text` 抽取 `choices[0].delta.content`，`oa_stream_close` 释放
句柄。资源：`chat.completions`、`responses`、`embeddings`、`models`、
`moderations`——阻塞与流式；类型化辅助没覆盖到的用 `oa_request` + JSON DOM
兜底。错误永不 raise：`oa_resp_ok` 把关，`oa_err_msg` 把最先失败的那一层
渲染成一个字符串。链接时加 `-l winhttp`（唯一自带 TLS 的传输）。

```powershell
cargo run -- run examples\openai_chat.ax -l winhttp   # 在线示例（需 key）
cargo test --test openai_sdk                          # 离线：纯层 + 用 Aoxn 写的 mock OpenAI 服务器
```

参考：[`docs/openai-sdk.md`](docs/openai-sdk.md) ·
示例 `examples/openai_chat.ax` · 测试 `tests/openai_sdk.rs`。

## Aoxn IDE

Aoxn 自带官方编辑器——`ide/`，一个 Tauri 2 外壳 + Next.js + Monaco 工作台 +
一层很薄的 Rust 命令层的原生桌面应用：

- **资源管理器**：项目树（自动跳过构建/依赖目录），带真正的**新建文件 /
  新建文件夹**——名字在对话框里就地校验，支持一步建出嵌套路径
  （`src/util.ax`），任何试图逃出打开目录或覆盖已有条目的名字都会被拒绝。
- **Monaco + Aoxn 语法**（`#` 注释、`f""` 插值、关键字/类型/内建着色），
  每个标签页一个 model，撤销栈按文件隔离。
- **诊断即编辑器标记**：前端解析编译器输出，点击输出面板里的诊断行直接跳到
  出错行；**保存 `.ax` 文件自动检查**，红波浪线跟着编辑走。
- **Check / Build / Run**（F7 / F6 / F5）驱动真实的 `aoxn` 二进制，参数与
  用户在终端敲的完全一致——IDE 不重新实现任何构建逻辑，因此在 IDE 里构建
  和在终端里构建不可能出现两套行为。Check 走 `aoxn check`（只出诊断，不刷 C）。
- **包管理**（v0.33.0）——侧栏里的 Packages 视图：展示工作区 `aoxn.json` 的
  依赖与 `aox_modules/` 里已安装的包，白名单动词做成按钮——Init、Add、
  Install、Update、Outdated、Tree、Audit、Why、Remove。每条命令都在打开的
  目录里运行真实的 `aoxn pkg` 并原样输出；对外或全局性的命令（publish、
  yank、cache、trust bootstrap、npm-import）在 Rust 闸门处直接拒绝，而不是
  只在界面上隐藏。`aoxn doctor` 也在这里：状态栏的"编译器 / clang 未找到"
  按钮会把它跑进输出面板。
- **快速打开**：Ctrl+P 文件、Ctrl+Shift+P 命令。

工作区是边界不是建议：webview 传来的每个路径都会先规范化，解析后落在打开
目录之外的一律拒绝（按路径分量比较——`C:\proj-evil` 骗不过 `C:\proj` 的
检查）；没有"读任意路径"的命令，也没有 shell 插件。

```powershell
cd ide
pnpm install
pnpm ide:dev        # 原生应用，热更新
pnpm dev            # 浏览器预览（内存 fixture）——改布局不用等原生重编译
pnpm test           # 前端测试（node:test）
pnpm test:rust      # Rust 命令层测试
```

详见 [`docs/ide.md`](docs/ide.md)。

## 性能

同算法与 `clang -O3` 同机对比（预热后 3 次取最优）：

| 基准 | 规模 | Aoxn | clang C++ |
|---|---|---|---|
| 循环求和 | 2×10⁸ 次迭代 | ~15 ms | ~23 ms |
| 数组填充 + 扫描 | 2×10⁸ 次读 | 86 ms | 99 ms |
| 结构体复制（按值） | 7.5×10⁷ 次 | 237 ms | 206 ms |

原生代码就是原生代码——Aoxn 与 clang 在噪声范围内持平。

Web 服务同样能打：[`web/`](web/README.md) 套件用 Aoxn 写了 HTTP/1.1 服务器，
在相同路由上对阵 pnpm + Node.js + Next.js——吞吐打平纯 Node.js、p50 延迟约为
其 1/50，比 Next.js 多服务 26–54 倍请求，单个 173 KB 二进制、5 MB 内存。
数据见 [docs/web-benchmark.md](docs/web-benchmark.md)。
自 2026-10-02 起该服务器还支持静态文件服务（W2）：启动时预载的内存文件表，
带 ETag / `If-None-Match` → 304 / 单区间 `Range` → 206 / 不可满足 → 416 /
`Cache-Control`，由 parity 套件对 Aoxn/Node 两台服务器做语义对比。

## 自举

编译器已用 Aoxn 重写并达到**固定点**：Aoxn 写的 driver 编译整个自举编译器
（约 7k 行：词法、语法、带单态化的类型检查、装载器、C 文本代码生成、驱动）
得到 stage-2，产物（生成的 C 与目标文件）和 Rust 侧构建的编译器**逐字节一致**，
并能编译真 stdlib 与全部 `examples/`。详见 [`docs/selfhost.md`](docs/selfhost.md)
与 [wiki 的自举页](wiki/Self-Hosting.md)。

## 项目布局

（表格同上方英文区：`src/` 编译器、`src/codegen_c.rs` 唯一的 C 发射后端
（v0.29.0 起）、`src/paths.rs` 安装布局发现、`src/setup/` 单文件安装器、
`src/ts/` TS 前端（TS-M1 W1 完成）、`stdlib/` 标准库 + UI v3 +
**OpenAI SDK**（`stdlib/openai/`，v0.40.1）、`ide/` Aoxn IDE
（`ide/src-tauri` 是独立 Cargo 工作区）、`dist/` 打包
脚本、`selfhost/` 自举、`web/` Web 基准、`tests/` 端到端测试（含 C 文本
固定点与安装布局）、`docs/ide.md` IDE 参考、`docs/spec.md` 语言规范、
`docs/openai-sdk.md` SDK 参考、
`wiki/` 双语 wiki——**已冻结**。`docs/` 才是活文档。）

## 测试与 CI

`cargo test` 跑端到端测试套件——编译器工作区 244 个（pipeline 126、编译器单元
测试 17、TypeScript 前端 34、UI 9、安装布局 6、CSS 资产 21、CSS 资产 v0.36 18、
符号导出 8、OpenAI SDK 2、安装器 3），另有 `aoxn-pkg` crate 的 94 个经
`bash run_pkg_tests.sh` 运行，合计 338 个——每个 pipeline 测试都是 .ax → 可执行

文件 → 运行 → 断言 stdout 与退出码。
其中含自举固定点：stage-1 与 stage-2 编译器对同一程序必须产出逐字节一致的
C 文本与目标文件（目标文件比较会屏蔽 clang 写入每个 Windows 目标文件的
COFF 时间戳）。IDE 自带两套独立测试：命令层的 36 个 Rust 测试（`ide/` 下
`pnpm test:rust`）与前端的 62 个 node:test 用例（`pnpm test`）；由于
`ide/src-tauri` 有意排除在根工作区之外，它们不会在 `cargo test` 里运行。
Aoxn 只支持 Windows（v0.30.0），每次 push 在 windows-latest
跑全套件；同一个任务随后打出单文件安装器、安装到临时目录并跑 `aoxn doctor`
与一个标准库程序——安装坏了会在构建时失败，而不是等到发版。打 tag 还会发布
`Aoxn-<version>-Setup.exe`（见
[`.github/workflows/release.yml`](.github/workflows/release.yml)）。

## 包管理

`aoxn pkg`（及直接别名 `aoxn init | add | remove | install | update |
outdated | list | freeze | tree | why | publish | yank | audit | trust |
cache | npm-import`）通过 `aoxn.json` + `aoxn.lock` 把依赖装进 `aox_modules/`，
用 PubGrub 对目录、git 或只读 HTTP registry 做解析（beta；见
[`crates/aoxn-pkg`](crates/aoxn-pkg) 与
[`docs/pkg-manager.md`](docs/pkg-manager.md)）。自 v0.29.1 起，裸包
导入按包内 `aoxn.json` 的 `main` / `exports`（含 `pkg/sub` 子路径）/ `types`
解析入口，装进来的包即便入口不叫 `index.ax` 也能 import：

```Aoxn
import * from "http"          # → aox_modules/http/aoxn.json 的 main/exports["."]
import * from "http/client"   # → exports["./client"]
```

没有 manifest 的包回退到旧的 `aox_modules/<name>` 目录探针（`<name>.ax` /
`index.ax`）。

**v0.32.0** 补齐了对照 pip / pnpm 时最缺的日常能力：

```bash
aoxn add -D harness        # devDependencies——不该进产物的工具依赖
aoxn install --prod        # 只装运行时，dev-only 包会被裁掉
aoxn install --jobs 16     # 并行下载 tarball（默认 8）
aoxn install --json        # 把安装报告变成机器可读的数据
aoxn list                  # 装了什么：名字/版本/scope/来源
aoxn freeze                # `name==version`，给 CI 基线用
aoxn audit --fix           # 升到公告给出的已修复版本
```

dev 与 prod 解析进**同一份**锁文件，靠每个包的 `dev` 标记区分——这样只发运行时
产物的 CI 任务，仍然能从一份也钉住了测试工具链的锁文件复现。
`"overrides": {"http": "1.4.2"}` 则是在依赖图任何位置强制该包的版本要求。

registry 还可以是**策展**的——一个仓库同时装着包、`trust.json` 评审记录和
`advisories/`：

```bash
aoxn trust bootstrap https://github.com/AlonechatWorkspace/Aoxn-trusted-third-party-package
aoxn trust check http --tier audited   # CI 门禁
```

安装时会对策展 registry 中没有评审记录的包给出告警；对不做信任声明的 registry
则保持沉默。信任清单是策展信号，不是签名——真正的密码学锚点仍是 `aoxn.lock` 里
钉住的 manifest 哈希。目录结构与 schema 见
[`docs/trusted-registry.md`](docs/trusted-registry.md)。

npm 桥接（`aoxn npm-import`，以 npm CLI 为传输层）把发布到任意 npm 兼容 registry
的 Aoxn 包导入 `vendor/<name>/` 并登记为路径依赖；纯 JavaScript 包会被明确拒绝。

## CSS 资产

`import` 进来的 `.css` 是**构建资产，不是源码**：它会被打包（`@import` 就地
内联）、压缩、指纹化后嵌入二进制——不需要输出目录，也不需要约定文件布局：

```Aoxn
import * from "./style.css"

def main() -> int:
    print(styles())               # 整份产物
    print(styles_fingerprint())   # "18603d4d4686e2cc.css"
    return 0
```

压缩只去注释与冗余空白，刻意不做别的——不合并选择器、不重排——因此产物 CSS
是源文件的纯函数。

**CSS Modules。** `*.module.css` 的类名会被作用域化（以文件自身路径为种子），
并生成一个取用函数；它的规则不进全局包，这正是作用域化有意义的前提：

```Aoxn
import * from "./page.module.css"
    print(page_class("title"))     # -> "title_87b780ec"
```

改写器识别上下文：字符串里、`@media` 前导里、声明值里的 `.` 都不是选择器，
原样保留。

**Tailwind** 以预生成产物接入，而非作为依赖——它是纯 JavaScript 的 npm 包，
`aoxn npm-import` 按设计拒绝这类包；而调用它的 CLI 会把 Node 变成构建前置。
可以自行运行 `npx tailwindcss -o generated.css` 后 import 结果，也可以让编译器
生成一份有文档的工具类子集：

```sh
aoxn build app.ts --tailwind
```

它只扫描 `class`/`className` 属性（绝不从散文中挖掘类名），并且对不覆盖的
工具类**指名报告**，而不是静默丢弃。

**从磁盘提供资产。** `--emit-assets <dir>` 把指纹化的产物包、每张样式表和全部
`url()` 目标写到可执行文件旁，并把 `url()` 改写成产物名。程序无需知道任何
构建期路径即可找到它们：

```Aoxn
print(asset_path(styles_fingerprint()))   # …\assets\82b4fb25….css
```

完整说明与限制见 [`docs/css-assets.md`](docs/css-assets.md)。

## 现状

**v0.40.1** · **只支持 Windows** · 338 测试全绿
（pipeline 126 + lib 17 + assets 21 + assets_v36 18 + symbols 8 + TS 34 + UI 9 +
OpenAI SDK 2 + 安装布局 6 + setup 3 + aoxn-pkg 94；IDE 另有 36 个 Rust + 62 个
前端测试）·
**OpenAI SDK 进驻标准库**（v0.40.1）——`stdlib/openai/` 五个纯 Aoxn 模块
（codec / JSON DOM / WinHTTP 传输 / SSE / client）给语言带来真正的 API 客户端：
chat completions、responses、embeddings、models、moderations，阻塞或逐 token
流式，默认值与环境变量名与参考 Python SDK 一致；离线端到端测试由一个用 Aoxn
写成的 mock OpenAI 服务器完成 ·
**语言长出第四条数据轴**（v0.40.0）——**函数指针**（值位置的裸函数名就是它的
地址，`fn(int) -> int` 标注、间接调用、`as` 在 `int` 与函数指针之间转换——
COM vtable 槽位与 Win32 回调的逃生门）、**`None` + `T | None` 可空**（唯一的
union 形式；`is None` / `is not None` 在 checker 与 codegen 两侧都按分支收窄；
收窄之前 print 与运算符都拒绝可空值）、真正的 **`raise` / `try` / `except`**
（v0.39.0「无异常」的决策被用户指令推翻——`raise <string>` 退到最近的
handler 或退出函数；`Err` 值通道保留给调用方要**检视**的失败），以及
**`dict[V]`**——字符串键字典，按插入序迭代，缺失键直接 raise。字典是**堆
句柄**（`TableModel` / `FileTable` 同款形状）：最初按值传 4 词结构体的写法
有两重不健全——被调方的增长对调用方不可见，过期的 `len` 又会走出被
realloc 过的缓冲（`-O0` 正确、`-O1` 起段错误；用 clang 直接编同一份生成 C
复现过）。现在拷贝共享同一个字典，set/del 透过任一副本都彼此可见 ·
**自举堆损坏已查明根因并修复（v0.39.2）**——struct 环检测器把 DFS 路径按值传递、push
之后递归，被调方的 `realloc` 释放了调用方仍持有的缓冲，下一个兄弟分支就读
到了已释放内存；它之所以潜伏，是因为内联增长一次性预留 8 槽、路径前 8 次
push 都不触发 realloc。修复方式是写回（`struct Cycle{hit, path}`），
`vec_push` 也重新把增长委托回 `vec_reserve` ·
**IDE 编辑器追上了语言**——位运算/移位操作符与 `err_*` / `out_*` / `vec_*`
标准库词汇现在在 IDE 里正确着色，两次版本之间落地的安装器修复也有了归属的
版本条目 ·
**一个语言在能装下真实数据之前必须先有的两样东西**——报告失败的方式，以及装
「不定多少个」东西的方式。失败的调用按值返回一个 `Err{code, message}`；因为
Aoxn 只有一个返回值且 struct 返回时拷贝，载荷要通过调用方自己持有的堆
out-slot 回来。而 `[T; N]` 是编译期定长的，所以 stdlib 现在提供带真正容量管理
的 `Vec`、作为一等元素的字符串，以及 `VecVec`（向量的向量，正是文档格式需要
的形状）。两者都没碰编译器。另外还补上了 Aoxn 一直缺的六个运算符——`&`、`|`、`^`、`~`、`<<`、`>>`，
只接受 int（和 `%` 一样），优先级与 C 一致（C 里 `==` 比 `&` **更紧**，
所以要写 `(a & b) == c`）；它们是为密码学/TLS/HTTP2 那条线准备的：
**CSS 管线收官**：`--emit-assets <dir>` 把指纹化的 CSS 与全部 `url()` 目标写到
可执行文件旁（并把 `url()` 改写为产物名），`styles.title` 提供类型化的类名
访问，`exe_dir()` / `asset_path()` 让可搬移的程序找到自己的资产，
CSS-in-Aoxn 负责行内样式，`--tailwind` 生成有文档的工具类子集——另修复一个
一直不可用的 TS import 形式（[`docs/css-assets.md`](docs/css-assets.md)）·
**IDE 深度接入包管理**：Packages 侧栏视图（`aoxn.json` / `aox_modules/` +
白名单化的 `aoxn pkg` 动词 Init/Add/Install/Update/Outdated/Tree/Audit/Why/
Remove）与状态栏的 `aoxn doctor` 自检 ·
**包管理对标 pip/pnpm**：devDependencies 配 `install --prod`、`overrides`、
策展 registry（信任清单 + 公告库同仓）、`aoxn list`/`freeze`、并行下载、
强制最低编译器版本 ·
**Aoxn IDE**（`ide/`）：Tauri 2 + Next.js + Monaco 工作台——资源管理器
（可新建文件/文件夹）、按标签页隔离的撤销、诊断即编辑器标记、保存自动检查、
Check/Build/Run 驱动真实的 `aoxn`；`aoxn check` 只做检查只出诊断 ·
**一个 exe 装全部**：`Aoxn-<version>-Setup.exe` 内含编译器、标准库、UI 工具箱
与示例；窗口参照 Python 官方安装器——一个标识、一句标题、一个大的 **Install Now**
按钮，之后是进度页与完成页——**点击之前磁盘上不会写入任何东西**。它**不下载任何
东西**：只解包、配置 PATH，再用 `aoxn doctor` 自检（`-InstallClang` 可选拉取
LLVM）·
包管理对标 pip/pnpm：devDependencies、overrides、`list`/`freeze`、并行下载、
带信任清单与公告库的策展 registry、强制最低编译器版本 · 自举固定点（生成的
C + 目标文件逐字节一致）· **标准库内置 UI 工具箱 v3**（Qt 级：
布局管理器、文本输入、焦点链、20 余控件、浮层覆盖——`examples/ui_gallery.ax`）·
零 LLVM 依赖：C 发射后端是唯一后端（clang 编译生成物）· TS-M1 W1 收官
（S2b 类型层 + S3 模块系统；旧 `import "path"` 已删除——用 `import * from "path"`）·
v0.29.7：Python 表面语法对标第一批（`+= -= *= /= %=`、`//`、一元 `+`、
链式比较——全部是解析期降级，Rust 侧与自举编译器同时实现）·
包管理器 W2：manifest 入口解析（`main` / `exports` / `types`）、只读 HTTP
registry 后端、npm 桥接（`aoxn npm-import`）。

安装步骤见 [`docs/install.md`](docs/install.md)，完整语言规范见
[`docs/spec.md`](docs/spec.md)，发布历史见
[`CHANGELOG.md`](CHANGELOG.md)。

## 许可证

Apache-2.0 —— 见 [`LICENSE`](LICENSE)。
