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

Download **`Aoxn-0.48.0-Setup.exe`** from the
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

Aoxn's first-class platform is **Windows** — the installer, the release
packaging and the Win32/GDI UI backend live there. **Linux is supported too
(v0.46.0)**: the compiler, the whole test suite and an Xlib/Xft UI backend
run on it — build from source with `cargo build` after `apt install clang
libx11-dev libxft-dev libfontconfig1-dev`. The single external prerequisite
on either platform is **clang** (the compiler lowers to C and hands it
over); on Windows the installer provisions LLVM through `winget` when it is
missing and linking uses the MSVC Build Tools, which clang finds
automatically. Full guide — scripted installs, `-Prefix`, uninstall,
troubleshooting: [`docs/install.md`](docs/install.md); the platform story:
[`docs/platform-support.md`](docs/platform-support.md).

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

### v0.44.0 on the surface — the standard library starts

Thirteen stdlib modules land as independent files, imported **by name**:

```Aoxn
import * from "stdlib/datetime"
import * from "stdlib/hashlib"

def main() -> int:
    now = datetime_now()                     # local wall clock
    print(datetime_iso(now))                 # 2026-10-05 21:14:07
    print(sha256_str("abc"))                 # ba7816bf8f01cfea…
    print(hmac_sha256("key", "msg"))         # RFC 2104
    print(b64_decode_str("aGVsbG8="))        # hello — decode now exists
    print(cal_month(now.year, now.month))    # a text month grid
    return 0
```

The batch: `math` (CRT trig/exp + the `fdiv`/`NaN()`/`is_nan` toolkit the
numeric libraries were waiting for), `time` (QPC monotonic + FILETIME wall
clock), `datetime` (Hinnant civil-date math, hand-parsed `strftime` subset),
`calendar`, `pathlib`, `base64` (encode **and** decode, std + URL-safe),
`hashlib` (SHA-256/SHA-1/MD5, one-shot + incremental, pure Aoxn bitwise),
`hmac`, `os` (filesystem core: all-wide UTF-16 on Windows, plain libc on Linux —
env, cwd, listdir),
`glob` (Python's dotfile rule, sorted output), `json` (the DOM promoted out
of `net/` + file I/O), `bisect`, `heapq`. None of them touches
`stdlib/stdlib.ax`, so the self-host fixed point is untouched. Reference —
per-module APIs, Python deltas and the traps (UCRT symbol collisions, the
zero-extended sentinel): [`docs/stdlib.md`](docs/stdlib.md).

### v0.43.0 on the surface — modules you can name

```Aoxn
# util.ax and shapes.ax both define `area`. That used to be a hard error.
import util
import shapes as s
from util import area            # one name, unqualified (imports are top-level)

def main() -> int:
    print(util.area(2))         # qualify across files — no prefixes needed
    print(s.area(3))            # `as` renames the MODULE, not its members
    print(area(4))              # the name `from` brought in
    return 0
```

```Aoxn
import shapes

def scale(c: shapes.Circle) -> shapes.Circle:   # a type reached through a module
    return shapes.Circle(r=c.r * 2)             # ...and constructed through it

def main() -> int:
    return scale(shapes.Circle(r=2)).r
```

The Python spellings all work and mean what they say in Python:
`from m import *`, `from m import a, b`, `from m import a as b`, `import m`,
`import m as n`. A **named import imports exactly what it lists** — anything
else stays out of scope — and a module binding is a namespace, not a value, so
`x = util` is an error while `util.f()` is not. A specifier can be written bare
and dotted (`import stdlib.net.json`, dots being the `/` subpath separator) or
quoted; the bare form also looks in the importing file's own directory, so
`import util` finds the `util.ax` next to it. Importing two modules that both
define `f` is fine as long as you reach them through their modules —
star-importing both into one scope is a conflict, reported by name.

### v0.42.0 on the surface

```Aoxn
def main() -> int:
    assert(0x1F == 31)          # hex / binary literals — malformed ones name
    assert(0b101 == 5)          #   the offending digit instead of "unknown variable 'x10'"

    i = 0
    while i < argc():           # command-line arguments: count + 0-based get
        print(arg(i))
        i = i + 1

    if argc() == 0:
        exit(2)                 # explicit status, from anywhere

    s = "line\r\n"              # \r and \0 join \n \t \\ \" (six escapes)
    print(len(s))               # 6 — CR and LF are real bytes
    return 0
```

`assert` aborts with the source line (not catchable — `raise` is for that),
`exit(code)` sets the status, `argc()`/`arg(i)` read what `aoxn run app.ax --
a b` forwarded, and `main` itself takes no parameters. The compiler also has a
**warning tier** now: `W001` reports a local that is assigned and never read
without failing the build, `--json` returns `{"ok":…,"errors":[…],"warnings":[…]}`
on success too, and `--clang-arg`/`-g` forwards flags to clang (debug info,
`-fsanitize=undefined`) — with a failed C compile reported as the `cc` stage
rather than `internal`.

## Standard library and the UI toolkit

`stdlib/stdlib.ax` is written in Aoxn itself: generic `sort` /
`binary_search` / aggregation, a growable `Vec`, byte buffers, file IO and
process spawning. Since v0.44.0 the stdlib is a set of **named modules**
under `stdlib/` — `math`, `time`, `datetime`, `calendar`, `pathlib`,
`base64`, `hashlib`, `hmac`, `os`, `glob`, `json`, `bisect`, `heapq` — each
its own file, imported by name, documented in
[`docs/stdlib.md`](docs/stdlib.md) with per-module demos
(`examples/*_demo.ax`) and driver tests (`tests/*.rs`).

Since v0.27.0 the stdlib ships a **Qt-flavored, immediate-mode UI toolkit**
in pure Aoxn on raw FFI; v0.29.3 brought it to **Qt grade**: layout
managers (vbox/hbox/grid with stretch), real text input with caret,
UTF-8-aware editing and **text selection** (Shift+arrows/drag, Ctrl+A/C/X/V
clipboard), a **multi-line editor**, a keyboard focus chain (Tab/Enter/
Space), a **menu bar** (keyboard-navigable and input-modal since v0.45.0:
Up/Down wrap, Enter picks, Esc closes), **tree/table model+view** (heap
`TreeModel`/`TableModel`), **signal-slot events** without function pointers,
and 20+ widgets (buttons, toggles, checkboxes, radio groups, sliders, spin
boxes, text fields, list boxes, floating combo boxes, tabs, group boxes,
scroll areas, tooltips), disabled groups, floating overlays and 16-role
light/dark themes. Widgets are plain functions called every frame and the
application owns all state, which is what fits a language without callbacks
(yet). v0.45.0 was a performance and correctness pass over the machinery;
v0.46.0 added horizontal scroll areas (`ui_scroll_begin_h` /
`ui_scroll_end_h`, a bottom scrollbar that nests with the vertical one)
and a monospace face (`ui_font_mono(c)` / `ui_font_sans(c)` — Consolas
on Windows, the fontconfig `Monospace` alias on Linux); **v0.47.0 fixed
the GDI clip stack, which had never restored anything** — one `ui_textbox`
anywhere in a frame clipped the whole window (the final `BitBlt` included)
to that textbox rect for the rest of the process — and added clip-aware
draw culling, a 64-entry text-measure cache and a DC state cache in the
Windows backend.

**The toolkit is three portable files plus one backend per platform** — a
portable core (`ui.ax`), a platform-neutral widget layer (`ui_draw.ax`),
and ONE of `ui_win.ax` (Win32/GDI) or `ui_x11.ax` (Xlib + Xft, v0.46.0) —
and a program picks it up with one import:

```Aoxn
import * from "stdlib/ui_win"     # Windows: Win32 + GDI (-l user32 -l gdi32)
import * from "stdlib/ui_x11"     # Linux: Xlib + Xft  (-l X11 -l Xft)
```

Widgets never name a platform symbol: they call `plat_*` primitives that
the backend supplies, and the contract tests pin BOTH backends to the same
primitive set, so the widget layer stays testable headlessly and the widget
set is identical everywhere. The X11 backend was removed in v0.30.0 and
rewritten against the v3 contract in v0.46.0 — Xft text, an off-screen
pixmap for double buffering, the X11 selection protocol for the clipboard,
and a per-frame `XQueryKeymap` sweep so held keys read continuously down.

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

```bash
aoxn run examples/ui_probe_x11.ax -l X11 -l Xft   # Linux smoke probe (also run under Xvfb in CI)
```

Details: [`docs/ui.md`](docs/ui.md) · gallery: `examples/ui_gallery.ax` ·
tests: `tests/ui.rs`.

### The API SDKs (v0.41.0)

Two provider SDKs ship in the stdlib over **one shared transport**.
`stdlib/net/` is provider-neutral — base64/percent/UTF-16 codecs, a JSON
DOM with a builder, HTTPS over WinHTTP, SSE — and each SDK adds its own
files on top. Both take their defaults, env variable names and retry policy
from the reference Python clients.

`stdlib/openai/` (v0.40.1) covers chat completions, responses, embeddings,
models and moderations, blocking or streamed:

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

`stdlib/anthropic/` (v0.41.0) covers the Messages API — where the payload
is an array of typed content blocks, not a list of strings — plus token
counting, models, files and message batches, with **tool use** end to end:

```Aoxn
import * from "stdlib/anthropic/client"

def main() -> int:
    c = an_client_env()                 # ANTHROPIC_API_KEY / ANTHROPIC_BASE_URL
    m = an_msgs_new()
    m = an_msgs_push_text(m, "user", "Say hello in one word")
    r = an_messages_create(c, "claude-sonnet-4-5", m, 256, an_opts_new())
    if not an_resp_ok(r):
        print(an_err_msg(r))
        return 1
    print(an_msg_text(r))               # -> Hello!
    print(an_usage_line(r))             # -> tokens 12 in / 25 out
    return 0
```

Three things carry across both SDKs. **Streaming** is a pull loop
(`*_stream` / `*_stream_next` / `*_stream_close`) over the SSE filter;
Anthropic's frames are *named*, and a tool call's arguments arrive only as
JSON text fragments — so `AnAcc` reassembles the stream into a finished
Message that the same accessors read (`an_acc_resp(st.acc)`). **Tools** get
a schema builder rather than hand-escaped JSON, and a full loop: advertise,
receive the `tool_use` block, send the assistant turn back verbatim, put
the `tool_result` in a user-role message, ask again. **Nothing raises**:
`an_resp_ok` / `oa_resp_ok` gate, `an_err_msg` / `oa_err_msg` render
whichever layer failed first into one string, and
`an_status_class(status)` turns the reference's exception taxonomy into
data a caller can switch on.

Link with `-l winhttp` (the one Windows transport that brings TLS without a
TLS stack); the HTTP entry points report a clean "no transport on this
platform" error on Linux rather than failing to link. `an_client_at` /
`oa_client_at` point either SDK at a gateway, a proxy or a loopback mock.

```powershell
cargo run -- run examples\openai_chat.ax -l winhttp      # live demo (needs a key)
cargo run -- run examples\anthropic_chat.ax -l winhttp   # message, stream, tool loop, token count
cargo test --test openai_sdk                             # offline: pure layers + a mock server written in Aoxn
cargo test --test anthropic_sdk
```

Reference: [`docs/openai-sdk.md`](docs/openai-sdk.md) ·
[`docs/anthropic-sdk.md`](docs/anthropic-sdk.md) · examples:
`examples/openai_chat.ax`, `examples/anthropic_chat.ax` · tests:
`tests/openai_sdk.rs`, `tests/anthropic_sdk.rs`.

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
| `stdlib/stdlib.ax` | the standard-library core, written in Aoxn itself |
| `stdlib/{math,time,datetime,calendar,pathlib,base64,hashlib,hmac,os,glob,json,bisect,heapq}.ax` | the v0.44.0 module batch — `docs/stdlib.md` is its reference |
| `stdlib/ui.ax`, `stdlib/ui_draw.ax` | the UI toolkit v3: portable core + platform-neutral widget layer (20+ widgets) |
| `stdlib/ui_win.ax`, `stdlib/ui_x11.ax` | the two `plat_*` backends: Win32/GDI (Windows) and Xlib/Xft (Linux, v0.46.0) |
| `stdlib/net/` | the shared API-client layer (v0.41.0, moved out of `stdlib/openai/`): codecs, JSON DOM + builder, WinHTTP transport, SSE — used by both SDKs |
| `stdlib/openai/` | the OpenAI SDK (v0.40.1): headers, client, resources, accessors, `OaStream` — `docs/openai-sdk.md` |
| `stdlib/anthropic/` | the Anthropic SDK (v0.41.0): content blocks, tools/schemas, client, `AnStream` + `AnAcc` — `docs/anthropic-sdk.md` |
| `docs/openai-sdk.md`, `docs/anthropic-sdk.md` | the two SDK references |
| `docs/stdlib.md` | the standard-library module reference (v0.44.0 batch): per-module APIs, Python-parity deltas, the traps |
| `examples/*.ax` | demo programs (hello, fib, primes, stdlib_demo, ui_demo, ui_gallery, math/time/pathlib/hashlib/os/json/bisect _demo, …) |
| `dist/package.ps1`, `src/setup/` | the single-file installer: the packager and the installer stub it fills |
| `selfhost/` | the compiler rewritten in Aoxn (fixed point reached) |
| `ide/` | the Aoxn IDE: Tauri 2 + Next.js + Monaco workbench (`ide/src-tauri` is its own Cargo workspace) |
| `web/` | web benchmark suite: an HTTP server in Aoxn vs pnpm+Node.js+Next.js |
| `tests/` | end-to-end tests: compile → run → verify output, including the self-hosting fixed point (byte-identical generated C + objects) and the install layout |
| `docs/install.md` | install guide (Windows) |
| `docs/ide.md` | the IDE: architecture, security boundary, development workflow |
| `docs/spec.md` | full language specification |
| `docs/stdlib-todo.md` | the standard-library roadmap: the 31 remaining Python stdlib modules batched P0–P3, plus the 15 third-party libraries (`numpy`, `pandas`, `flask`, `matplotlib`, `pytorch`, …) ordered by dependency chain — each with the language constraints it runs into |
| `docs/language-gaps.md` | why some of those cannot be built: every missing language feature (no references, no `enum`/tagged unions, no tuples, no namespaces, no GC, …) with what it blocks, which files a change touches, its cost, and a ranked order for closing them |
| `wiki/` | bilingual (中文/English) wiki — frozen since v0.29.3; `docs/` is the living documentation |

## Testing & CI

`cargo test` runs the end-to-end suite — **285 tests** in the compiler
workspace (pipeline 154, compiler unit tests 18, installer stub 3, TypeScript
front end 34, UI 11, install layout 6, CSS assets 21, CSS assets v0.36 18,
symbol export 8, OpenAI SDK 2, Anthropic SDK 2, and one driver test per
v0.44.0 stdlib module group: hashlib, datetime, math, pathlib, containers,
os+glob, json file-IO — plus `stdlib_defect_pins`, the v0.48.0 regression
suite for the JSON builder corruption and the quadratic loops) plus the
`aoxn-pkg` crate's 94 via
`bash run_pkg_tests.sh` — **379 in total** — where every pipeline test
compiles `.ax` to an executable, runs it and asserts stdout + exit code. The
suite includes the self-hosting fixed point: the stage-1 and stage-2
compilers must emit byte-identical C and object files for the same program
(the object comparison masks the COFF TimeDateStamp that clang stamps into
every Windows object). The two SDK suites run the REAL WinHTTP path against
a mock API server written in Aoxn on a loopback port, so message creation,
SSE streaming, tool-call reassembly, pagination, `Retry-After` and the error
bodies are all covered with no network and no credentials.
The IDE carries its own suites next to these: 36 Rust tests for the command
layer (`pnpm test:rust` in `ide/`) and 62 node:test cases for the frontend
(`pnpm test`), which do not run in a plain `cargo test` because
`ide/src-tauri` is deliberately excluded from the root workspace.

CI runs **two jobs** on every push. `windows-latest` runs the whole suite,
then packages the single-file installer, installs it into a scratch prefix
and runs `aoxn doctor` plus a stdlib program, so a broken install fails the
build and not the next release. `ubuntu-latest` (v0.46.0) runs the same
suite on Linux — proving the compiler, the dual-platform stdlib and the
fixed point on a second host — and then builds
`examples/ui_probe_x11.ax` with `-l X11 -l Xft` and drives it for 30 real
frames under Xvfb, the link-and-run check for the X11 UI backend. Release
tags publish `Aoxn-<version>-Setup.exe` (see
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

**v0.48.0** · **a stdlib audit: one heap corruption and three quadratic loops,
all from the same two mistakes** · 379 tests green
(285 in the compiler workspace + 94 in `aoxn-pkg`; the IDE adds 36 Rust +
62 frontend tests of its own) ·

**the JSON builder could shrink a parsed node's buffer under you** (v0.48.0) —
the parser adopts a `Vec`'s child buffer but leaves the node's capacity at 0,
and the grow read that as "default 8", doubled to 16, and `realloc`'d a
20-member object's 256-byte block **down** to 128 before writing slot 20. Any
parsed object or array with 16+ members died with `0xC0000005` the first time
a field was added — which is exactly what `jb_set_raw` exists to do, and why
it survived: the SDK tests only ever built fresh nodes. Confirmed
differentially (8 members fine, 20 members access violation). A grow now
floors the capacity at the live child count ·
**and `len()` on a string is `strlen` emitted inline at every use site**,
which made three loops quadratic: `j_at`'s end-of-input test (the JSON parser
ran at ~33 KB/s — a 266 KB response took 7 s), `jp_str`'s buffer size (every
string allocated the whole document: a 330 KB payload with 20 000 strings
peaked at 325.8 MB), and `b64_decode`'s loop bound (1.4 MB did not finish in
600 s). `j_dumps` compounded it — concatenation allocates a fresh buffer per
step and abandons the old one, so 20 000 keys touched **8627.8 MB and 28.4 s**.
All four are now linear: 266 KB parses in 7 ms, the same 20 000-key response
serializes in 282 ms at 10.2 MB, and base64 is 3 ms for 2.8 MB. Also fixed: an
unbounded JSON exponent (`1e999999999` spun for minutes — a reachable DoS on
any untrusted JSON), base64's rejection paths leaked a buffer whose length the
sender chooses, and `os_has_env` read a stale `GetLastError` that Win32 does
not clear on success — so a variable that really existed answered `False`
after any earlier miss ·

**the previous milestone** (v0.47.0) — the UI toolkit finally paints what it draws, and stops paying for what
nobody can see — the GDI clip stack had never restored
anything (`IntersectClipRect` only ever narrows, so the old pop narrowed it
further), which meant **one `ui_textbox` anywhere in a frame left the whole
window clipped to that textbox rect for the rest of the process** —
`BitBlt` included, since a blit copies only what the source clip allows;
`ui_win.ax` now keeps one `SaveDC` handle per level and `RestoreDC`es on
pop, the widget layer keeps its own shadow of that stack (so a pop restores
the parent rect and can also cull draws the clip cannot show), every draw
primitive skips the platform when the clip hides it (**-40 %** on a 3000 px
document in a 200 px viewport), `ui_measure` answers from a 64-entry
content-keyed cache (**96x** on a repeat: 52.8 ms -> 0.55 ms per 20 000
measures), and the Windows backend remembers the DC's selected pen, brush,
font and colors instead of re-selecting them per call (**-6 %** per fill,
~9 % on a 100-widget scene). The new pixel-readback test
(`ui_clip_stack_restores_and_culls`) fails against the previous stdlib with
seven mismatches. Honest number: a heavy scene got **slower** (1.8 -> 3.5
ms/frame) because the old one was not drawing most of the frame ·

**the previous milestone** (v0.46.0) brought Linux back and gave the
toolkit a second backend through the same `plat_*` contract — v0.30.0 had
cut the platform list to Windows alone; v0.46.0 restores Linux for the
compiler (ELF objects, `-lm` linked, no `/STACK` flag), the whole test
suite (a new `ubuntu-latest` CI job) and a **rewritten `stdlib/ui_x11.ax`**:
Xlib for the window and input, Xft for antialiased UTF-8 text, an
off-screen Pixmap for double buffering, the X11 selection protocol for the
clipboard, a per-frame `XQueryKeymap` sweep so held keys read continuously
down instead of flickering between autorepeat pairs, and the close request
travelling through shared-block slot 824 (Aoxn passes `UI` by value, so
`plat_close` cannot set a field the app loop would see). `target_os()` now
folds the OS the compiler was BUILT on — `"windows"` or `"linux"` — and the
self-hosted compiler needs no platform check of its own: its builtin emitter
calls `target_os()`, a value burned in by whatever compiler built it, so
every stage of the fixed-point chain on one machine folds the same host OS
and the fixed point stays byte-identical by construction. The stdlib
branches rather than forks: `os.ax` serves Linux through plain libc
(mkdir/stat/opendir/readdir/getenv/setenv, the glibc struct offsets
documented in the header) while the `W` Win32 calls keep serving Windows —
an unreferenced `extern def` emits no symbol reference, so each platform
links only its own side; `exe_path()` answers through `/proc/self/exe` on
Linux. What stays Windows-only does so honestly: the HTTP transport rides
WinHTTP and the entry points return a clean "no transport on this platform"
error on Linux rather than failing to link, and the single-file installer
remains a Windows artifact. That release also added horizontal scroll areas
(`ui_scroll_begin_h` / `ui_scroll_end_h`) and a monospace face
(`ui_font_mono` / `ui_font_sans`; the clip stack intersects, so vertical and
horizontal scroll areas nest) ·

**before that** (v0.45.0) was a performance and correctness pass
over the UI machinery: a popup-overflow clamp for combobox/menu lists past
32 items, O(n) text positioning (one measure per codepoint accumulated),
an O(1) `tree_has_child` on a per-node child count, the caret x-offset
cache (st 532..535), keyboard-navigable modal menus, and one shared
`sb_widget`; v0.44.1 fixed `os_getenv` truncating values longer than 2048
characters; v0.44.0 landed the first thirteen stdlib modules (`math` `time`
`datetime` `calendar` `pathlib` `base64` `hashlib` `hmac` `os` `glob` `json`
`bisect` `heapq`, imported by name, none touching `stdlib/stdlib.ax` so the
fixed point held); v0.43.0 made modules nameable namespaces
(`import util` + `util.f(...)`, the Python spellings included); v0.42.0
closed the everyday gaps (radix literals, `\r`/`\0`, `assert`/`exit`,
`argc()`/`arg(i)`, the `cc` stage, the warning tier); v0.41.0/v0.40.1
brought the Anthropic and OpenAI SDKs over one shared WinHTTP transport;
v0.40.0 added function pointers, `None`/`T | None`, `raise`/`try`/`except`
and `dict[V]`. The full history lives in
[`CHANGELOG.md`](CHANGELOG.md) ·

standing facts: self-hosting fixed point (byte-identical generated C +
object files) · UI toolkit v3 in the stdlib (Qt-grade: layout managers,
text input, focus chain, 20+ widgets, floating overlays —
`examples/ui_gallery.ax`) · no LLVM dependency: the C-emitting backend is
the only backend (clang compiles it) · TS-M1 W1 complete · package manager
beta (PubGrub, curated registries, npm bridge) · one-file installer
(`Aoxn-<version>-Setup.exe`) that downloads nothing by default.

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
下载 **`Aoxn-0.48.0-Setup.exe`**，双击即可。**这一个 exe 里就带着编译器、标准库、
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

Aoxn 的**第一梯队平台是 Windows**——安装器、发布打包与 Win32/GDI UI 后端都
在那里。**Linux 同样受支持（v0.46.0）**：编译器、完整测试套件与 Xlib/Xft UI
后端都能跑——`apt install clang libx11-dev libxft-dev libfontconfig1-dev`
之后 `cargo build` 从源码构建即可。两个平台上唯一的外部依赖都是 **clang**
（Aoxn 生成 C 后交给它编译链接）；Windows 上安装器会在缺失时用 `winget` 装
LLVM，链接还需要 MSVC Build Tools，clang 会自动探测。静默安装（脚本 / CI，
无需窗口）、自定义目录、卸载与排错见 [`docs/install.md`](docs/install.md)；
平台全景见 [`docs/platform-support.md`](docs/platform-support.md)。


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

### v0.44.0 在库面上加了什么——标准库开张

十三个标准库模块以独立文件落地，**按名字导入**：

```Aoxn
import * from "stdlib/datetime"
import * from "stdlib/hashlib"

def main() -> int:
    now = datetime_now()                     # 本地墙上时间
    print(datetime_iso(now))                 # 2026-10-05 21:14:07
    print(sha256_str("abc"))                 # ba7816bf8f01cfea…
    print(hmac_sha256("key", "msg"))         # RFC 2104
    print(b64_decode_str("aGVsbG8="))        # hello —— decode 方向补齐了
    print(cal_month(now.year, now.month))    # 文本月历
    return 0
```

本批模块：`math`（CRT 三角/指数 + 数值库等了很久的 `fdiv`/`NaN()`/`is_nan`
工具箱）、`time`（QPC 单调钟 + FILETIME 墙上钟）、`datetime`（Hinnant 民用
历数学、手写 `strftime` 子集）、`calendar`、`pathlib`、`base64`（编码**和**
解码、标准 + URL-safe 两套字母表）、`hashlib`（SHA-256/SHA-1/MD5，一次性 +
增量，纯 Aoxn 位运算）、`hmac`、`os`（文件系统核心：Windows 全宽字符、Linux 直走 libc——
环境变量、工作目录、listdir）、`glob`（Python 的 dotfile 规则、排序输出）、`json`
（DOM 自 `net/` 提升 + 文件 I/O）、`bisect`、`heapq`。没有一个碰
`stdlib/stdlib.ax`，自举固定点不动。逐模块 API、与 Python 的差异以及各种坑
（UCRT 链接符号冲突、零扩展哨兵值）：[`docs/stdlib.md`](docs/stdlib.md)。

### v0.43.0 在语言面上加了什么——可以点名的模块

```Aoxn
# util.ax 和 shapes.ax 都定义了 area。此前这是硬错误。
import util
import shapes as s
from util import area            # 只取一个名字，不加限定（import 只能写在顶层）

def main() -> int:
    print(util.area(2))         # 跨文件限定访问——不再需要前缀
    print(s.area(3))            # as 改的是「模块」的名字，不是它成员的名字
    print(area(4))              # from 带进来的那个名字
    return 0
```

```Aoxn
import shapes

def scale(c: shapes.Circle) -> shapes.Circle:   # 经模块引用的类型
    return shapes.Circle(r=c.r * 2)             # 以及经模块构造

def main() -> int:
    return scale(shapes.Circle(r=2)).r
```

Python 的写法全部可用，且含义与 Python 一致：`from m import *`、
`from m import a, b`、`from m import a as b`、`import m`、`import m as n`。
**具名导入只导入列出的名字**，其余名字不在作用域内；模块绑定是命名空间而非
值，所以 `util.f()` 成立而 `x = util` 是编译错误。模块说明符可写成裸的点号形式
（`import stdlib.net.json`，点即 `/` 子路径分隔符）或加引号；裸写法还会先在
导入文件自己的目录里找，所以 `import util` 能找到旁边的 `util.ax`。两个模块
都定义 `f` 时，只要经各自的模块访问就没问题——把两个都星号导入到同一作用域
则会冲突，并且会指名道姓地报出来。

### v0.42.0 在语言面上加了什么

```Aoxn
def main() -> int:
    assert(0x1F == 31)          # 十六进制 / 二进制字面量；写错时报
    assert(0b101 == 5)          #   「invalid digit」而不是「unknown variable 'x10'」

    i = 0
    while i < argc():           # 命令行参数：个数 + 按下标取
        print(arg(i))
        i = i + 1

    if argc() == 0:
        exit(2)                 # 显式退出码，任何位置

    s = "line\r\n"              # \r 与 \0 加入转义集（共六个）
    print(len(s))               # 6 —— CR/LF 是真实字节
    return 0
```

`assert` 失败即中止并带源行号（不可捕获——那是 `raise` 的事）、`exit(code)`
设退出码、`argc()`/`arg(i)` 读到 `aoxn run app.ax -- a b` 转发的参数、
`main` 本身不带参数。编译器也有了**警告层**：`W001` 报「赋值后从未读取」的
局部变量且不使构建失败；`--json` 在成功时也返回
`{"ok":…,"errors":[…],"warnings":[…]}`；`--clang-arg`/`-g` 透传给 clang
（调试信息、`-fsanitize=undefined`）——C 编译失败现在是 `cc` 阶段，
不再是 `internal`。

## 标准库与 UI 工具箱

`stdlib/stdlib.ax` 用 Aoxn 自己写成：泛型 `sort` / `binary_search` / 聚合、
可增长 `Vec`、字节缓冲、文件 IO 与进程调用。自 v0.44.0 起标准库是一组
**具名模块**（`stdlib/` 下）：`math`、`time`、`datetime`、`calendar`、
`pathlib`、`base64`、`hashlib`、`hmac`、`os`、`glob`、`json`、`bisect`、
`heapq`——各自独立成文件、按名字导入，参考文档在
[`docs/stdlib.md`](docs/stdlib.md)，配套 `examples/*_demo.ax` 示例与
`tests/*.rs` 驱动测试。

自 v0.27.0 起标准库带有 **Qt 风格的立即模式 UI 工具箱**（纯 Aoxn + 原始
FFI），v0.29.3 升到 **Qt 级**：布局管理器（vbox/hbox/grid +
千分比拉伸）、真文本输入（光标、UTF-8 感知编辑、**文本选区** Shift+方向键/
拖拽、Ctrl+A/C/X/V 剪贴板）、**多行编辑器**、键盘焦点链（Tab/Enter/Space）、
**菜单栏**（v0.45.0 起支持键盘导航且对输入模态：↑/↓ 循环移动、Enter 选中、
Esc 关闭）、**树/表格模型视图**（堆 `TreeModel`/`TableModel`）、无函数指针的
**信号槽事件**、20 余控件（按钮、切换钮、复选框、单选组、滑条、微调框、
文本框、列表框、浮层下拉框、标签页、分组框、滚动区、工具提示）、禁用态、
浮层覆盖与 16 色亮/暗主题。控件是每帧调用的普通函数，状态由应用自己
持有——这正是"暂无回调"的语言所能承载的形态（用法示例见上方英文区）。
v0.45.0 是对内部机制的一次性能与正确性整备；v0.46.0 新增横向滚动区域
（`ui_scroll_begin_h` / `ui_scroll_end_h`，底部滚动条，与竖向可嵌套）
与等宽字体（`ui_font_mono(c)` / `ui_font_sans(c)`——Windows 上是
Consolas，Linux 上是 fontconfig 的 `Monospace` 别名）；**v0.47.0 修好了
GDI 裁剪栈——它从来没能恢复过**：一帧里只要出现一个 `ui_textbox`，整个
窗口（含最后的 `BitBlt`）从那一刻起就被裁到那个 textbox 的矩形里，直到
进程结束；同时加入按裁剪区剔除绘制、64 项文本测量缓存，以及 Windows
后端的 DC 状态缓存。

**工具箱由三个可移植文件加每平台一个后端组成**：可移植内核（`ui.ax`）、与
平台无关的控件层（`ui_draw.ax`），以及 `ui_win.ax`（Win32/GDI）或
`ui_x11.ax`（Xlib + Xft，v0.46.0）二选一。一行 import 即可引入：

```Aoxn
import * from "stdlib/ui_win"     # Windows: Win32 + GDI（-l user32 -l gdi32）
import * from "stdlib/ui_x11"     # Linux: Xlib + Xft（-l X11 -l Xft）
```

控件层从不出现任何平台符号：它只调用后端提供的 `plat_*` 原语，契约测试把
**两个后端**钉在同一套原语集上，因此这一层可以无窗口地测试，控件集合在
两个平台上完全一致。X11 后端在 v0.30.0 被移除、v0.46.0 按 v3 契约重写——
Xft 文本、离屏 Pixmap 双缓冲、X11 selection 协议剪贴板，以及每帧一次的
`XQueryKeymap` 扫描（按住的键不再在自动重复的 release+press 对之间闪烁）。

```powershell
cargo run -- run examples\ui_gallery.ax -l user32 -l gdi32   # 全控件画廊
cargo run -- run examples\ui_demo.ax -l user32 -l gdi32      # 入门示例
```

```bash
aoxn run examples/ui_probe_x11.ax -l X11 -l Xft   # Linux 冒烟探针（CI 里还会在 Xvfb 下跑）
```


细节见 [`docs/ui.md`](docs/ui.md)；画廊 `examples/ui_gallery.ax`；测试
`tests/ui.rs`。

### API SDK（v0.41.0）

标准库里有两套厂商 SDK，共用**同一层传输**。`stdlib/net/` 与厂商无关——
base64/百分号编码/UTF-16 编解码、带构建器的 JSON DOM、WinHTTP 上的 HTTPS、
SSE——各 SDK 在其上追加自己的文件。两者的默认值、环境变量名与重试策略都取自
参考 Python 客户端。

`stdlib/openai/`（v0.40.1）覆盖 chat completions、responses、embeddings、
models 与 moderations，阻塞与流式皆可：

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

`stdlib/anthropic/`（v0.41.0）覆盖 Messages API——那里的载荷是**带类型的内容块
数组**，不是字符串列表——外加 token 计数、models、files 与消息批处理，并且
**工具调用**是完整闭环的：

```Aoxn
import * from "stdlib/anthropic/client"

def main() -> int:
    c = an_client_env()                 # ANTHROPIC_API_KEY / ANTHROPIC_BASE_URL
    m = an_msgs_new()
    m = an_msgs_push_text(m, "user", "Say hello in one word")
    r = an_messages_create(c, "claude-sonnet-4-5", m, 256, an_opts_new())
    if not an_resp_ok(r):
        print(an_err_msg(r))
        return 1
    print(an_msg_text(r))               # -> Hello!
    print(an_usage_line(r))             # -> tokens 12 in / 25 out
    return 0
```

三件事是两套 SDK 共有的。**流式**都是拉取循环（`*_stream` /
`*_stream_next` / `*_stream_close`）跑在 SSE 过滤器上；Anthropic 的帧是
**带名字**的，而工具调用的参数只以 JSON 文本片段的形式抵达——于是 `AnAcc`
把整条流重新组装成一条完整的 Message，同一组访问器照样读它
（`an_acc_resp(st.acc)`）。**工具**用 schema 构建器而不是手写转义 JSON，
并且是完整闭环：声明工具、收到 `tool_use` 块、把 assistant 轮次原样回传、
把 `tool_result` 放进 user 角色的消息、再问一次。**永不 raise**：
`an_resp_ok` / `oa_resp_ok` 把关，`an_err_msg` / `oa_err_msg` 把最先失败的
那一层渲染成一个字符串，`an_status_class(status)` 则把参考实现的异常体系
变成调用者可 switch 的数据。

链接时加 `-l winhttp`（唯一自带 TLS 的传输）；HTTP 入口在 Linux 上会返回
干净的「本平台暂无传输层」错误而不是链接失败。`an_client_at` /
`oa_client_at` 可把任一 SDK 指向网关、代理或环回 mock。

```powershell
cargo run -- run examples\openai_chat.ax -l winhttp      # 在线示例（需 key）
cargo run -- run examples\anthropic_chat.ax -l winhttp   # 消息、流式、工具闭环、token 计数
cargo test --test openai_sdk                             # 离线：纯层 + 用 Aoxn 写的 mock 服务器
cargo test --test anthropic_sdk
```

参考：[`docs/openai-sdk.md`](docs/openai-sdk.md) ·
[`docs/anthropic-sdk.md`](docs/anthropic-sdk.md) · 示例
`examples/openai_chat.ax`、`examples/anthropic_chat.ax` · 测试
`tests/openai_sdk.rs`、`tests/anthropic_sdk.rs`。

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
`src/ts/` TS 前端（TS-M1 W1 完成）、`stdlib/` 标准库核心 + v0.44.0 模块批
（`docs/stdlib.md`）+ UI v3 +
**OpenAI SDK**（`stdlib/openai/`，v0.40.1）、`ide/` Aoxn IDE
（`ide/src-tauri` 是独立 Cargo 工作区）、`dist/` 打包
脚本、`selfhost/` 自举、`web/` Web 基准、`tests/` 端到端测试（含 C 文本
固定点与安装布局）、`docs/stdlib.md` 标准库模块参考、`docs/ide.md` IDE 参考、
`docs/spec.md` 语言规范、
`docs/openai-sdk.md`、`docs/anthropic-sdk.md` SDK 参考、
`wiki/` 双语 wiki——**已冻结**。`docs/` 才是活文档。）

## 测试与 CI

`cargo test` 跑端到端测试套件——编译器工作区 **285 个**（pipeline 154、编译器
单元测试 18、安装器 stub 3、TypeScript 前端 34、UI 11、安装布局 6、CSS 资产
21、CSS 资产 v0.36 18、符号导出 8、OpenAI SDK 2、Anthropic SDK 2，以及
v0.44.0 标准库模块组各一个驱动测试：hashlib、datetime、math、pathlib、
containers、os+glob、json 文件 I/O——外加 `stdlib_defect_pins`，即 v0.48.0
为 JSON 构建器堆损坏与三处二次方循环加的回归套件），另有 `aoxn-pkg` crate 的
94 个经 `bash run_pkg_tests.sh` 运行——**合计 379 个**——每个 pipeline 测试都是
.ax → 可执行文件 → 运行 → 断言 stdout 与退出码。其中含自举固定点：
stage-1 与 stage-2 编译器对同一程序必须产出逐字节一致的 C 文本与目标文件
（目标文件比较会屏蔽 clang 写入每个 Windows 目标文件的 COFF 时间戳）。
两套 SDK 测试都会把**真实的 WinHTTP 路径**打到一台用 Aoxn 写成、跑在环回
端口上的 mock API 服务器上，因此消息创建、SSE 流式、工具调用重组、分页、
`Retry-After` 与各类错误响应都无需联网、无需凭据即被覆盖。IDE 自带两套
独立测试：命令层的 36 个 Rust 测试（`ide/` 下 `pnpm test:rust`）与前端的
62 个 node:test 用例（`pnpm test`）；由于 `ide/src-tauri` 有意排除在根
工作区之外，它们不会在 `cargo test` 里运行。

CI 每次 push 跑**两个任务**。`windows-latest` 跑全套件，然后打出单文件
安装器、安装到临时目录并跑 `aoxn doctor` 与一个标准库程序——安装坏了会在
构建时失败，而不是等到发版。`ubuntu-latest`（v0.46.0）在 Linux 上跑同一套
件——证明编译器、双平台标准库与自举固定点在第二台宿主上同样成立——随后用
`-l X11 -l Xft` 构建 `examples/ui_probe_x11.ax` 并在 Xvfb 下驱动 30 个真实
帧，作为 X11 UI 后端的链接与运行检查。打 tag 发布
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

**v0.48.0** · **一次标准库审计：一处堆损坏与三处二次方循环，都源于同样两个错误**
· 379 测试全绿
（编译器工作区 285 + `aoxn-pkg` 94；IDE 另有 36 个 Rust + 62 个前端测试）·

**JSON 构建器可能在你脚下把已解析节点的缓冲区改小**（v0.48.0）——解析器会
接管 `Vec` 的子缓冲区，却把节点的容量槽留成 0，而扩容逻辑把 0 读成「默认 8」，
翻倍到 16，于是把一个 20 成员对象的 256 字节块**向下** `realloc` 到 128 字节，
再往第 20 槽写。任何**成员数 ≥16** 的已解析对象或数组，第一次加字段就会以
`0xC0000005` 崩溃——而那恰恰是 `jb_set_raw` 的用途，也正是它长期没被发现的原因：
SDK 测试只构建过全新的节点。差分实验坐实（8 个成员正常，20 个成员越界）。
现在扩容一律以**存活子节点数**为下限 ·
**并且 `len()` 作用在 string 上就是 `strlen`，且在每个使用点内联发射**，
这让三处循环变成二次方：`j_at` 的输入结束判断（JSON 解析器只跑到 ~33 KB/s，
266 KB 的响应要 7 秒）、`jp_str` 的缓冲区大小（每个串都按整篇文档分配：
330 KB、2 万个串的输入峰值 325.8 MB）、`b64_decode` 的循环上界（1.4 MB 在
600 秒内都没跑完）。`j_dumps` 又叠加了一层——字符串拼接每步新分配一块并丢弃
旧的，于是 2 万个键的响应吃掉 **8627.8 MB 和 28.4 秒**。四处现已全部线性：
266 KB 解析 7 ms，同一份 2 万键响应序列化 282 ms / 10.2 MB，base64 处理
2.8 MB 是 3 ms。同时修掉：JSON 指数无上界（`1e999999999` 会空转数分钟，
对任何不受信的 JSON 都是可达 DoS）、base64 的拒绝路径泄漏了长度由发送方
决定的缓冲区、以及 `os_has_env` 读了陈旧的 `GetLastError`——Win32 成功时并不
清除它，于是任何一次先前的查找未命中都会让**确实存在**的变量被答成 `False` ·

**上一个里程碑**（v0.47.0）——UI 工具箱终于画出了它要画的东西，也不再为没人能看见的东西付费
——GDI 的裁剪栈从来没能恢复过（`IntersectClipRect` 只会收窄，
所以旧的 pop 只会收得更窄），于是**一帧里只要出现一个 `ui_textbox`，整个
窗口从那一刻起就被裁到那个 textbox 的矩形里，直到进程结束**——`BitBlt`
也一样中招，因为 blit 只拷贝源 DC 裁剪区允许的部分。`ui_win.ax` 现在每层
保存一个 `SaveDC` 句柄、pop 时 `RestoreDC`；控件层同时保留自己那份裁剪栈
影子（pop 于是能真的还原父矩形，也能顺手剔掉裁剪区看不见的绘制——**-40 %**，
场景是 3000 px 文档塞进 200 px 视口）；`ui_measure` 由 64 项按内容哈希
索引的缓存回答（重复测量 **96×**：每 2 万次 52.8 ms → 0.55 ms）；Windows
后端还会记住 DC 已选中的画笔、画刷、字体与颜色，不再每次重选（每次填充
**-6 %**，100 控件场景整体约 **-9 %**）。新增的像素回读测试
`ui_clip_stack_restores_and_culls` 在旧标准库上会报七处不符。诚实数字：
重场景反而**变慢**了（1.8 → 3.5 ms/帧），因为旧的那个根本没画出大部分
帧 ·

**上一个里程碑**（v0.46.0）让 Linux 回归，并经同一套 `plat_*` 契约给工具箱
加上第二个后端——v0.30.0 曾把平台清单砍到只剩 Windows；v0.46.0 把
Linux 带回编译器（ELF 目标文件、自动链 `-lm`、不再传 `/STACK` 标志）、
整套测试（新增 `ubuntu-latest` CI 任务）与**重写的 `stdlib/ui_x11.ax`**：
Xlib 负责窗口与输入、Xft 负责反锯齿 UTF-8 文本、离屏 Pixmap 双缓冲、
X11 selection 协议剪贴板、每帧一次 `XQueryKeymap` 扫描（按住的键不再在
自动重复的 release+press 对之间闪烁），关闭请求经共享块 824 槽传递
（Aoxn 按值传 `UI`，`plat_close` 无法把字段写回应用循环）。`target_os()`
现在折叠的是**编译器构建时**的宿主 OS——`"windows"` 或 `"linux"`——而
自举编译器不需要自己的平台检测：它的内建发射器直接调用 `target_os()`，
这个值由构建它的编译器烧定，因此同一台机器上固定点链条的每一级折叠的是
同一个宿主 OS，固定点按构造逐字节成立。标准库**分支而非分叉**：`os.ax`
在 Linux 上走纯 libc（mkdir/stat/opendir/readdir/getenv/setenv，glibc
结构体偏移量在文件头注明），`W` 系列 Win32 调用继续服务 Windows——
未被引用的 `extern def` 不产生符号引用，所以每个平台只链接自己那一侧；
Linux 上 `exe_path()` 经 `/proc/self/exe` 回答。保持 Windows-only 的部分
以诚实的方式守着：HTTP 传输层骑在 WinHTTP 上，Linux 上入口返回干净的
「本平台暂无传输层」错误而不是链接失败；单文件安装器仍是 Windows 工件。
同一版本还加入横向滚动区域（`ui_scroll_begin_h` / `ui_scroll_end_h`，
裁剪栈取交集，故竖向与横向可嵌套）与等宽字体（`ui_font_mono` /
`ui_font_sans`）·

**再之前**（v0.45.0）是对 UI 机制的性能与正确性整备：浮层列表超过
32 项的溢出钳制、O(n) 文本定位、凭每节点子计数做到 O(1) 的
`tree_has_child`、光标 x 偏移缓存（st 532..535）、可键盘导航的模态菜单、
合并为一个的 `sb_widget`；v0.44.1 修复 `os_getenv` 截断超过 2048 字符的
值；v0.44.0 落地标准库前十三个模块（按名字导入，没有一个碰
`stdlib/stdlib.ax`，固定点不动）；v0.43.0 让模块成为可点名的命名空间；
v0.42.0 关闭日常缺口（进制字面量、`\r`/`\0`、`assert`/`exit`、
`argc()`/`arg(i)`、`cc` 阶段、警告层）；v0.41.0/v0.40.1 在共享的 WinHTTP
传输上落地 Anthropic 与 OpenAI 两套 SDK；v0.40.0 加入函数指针、
`None`/`T | None`、`raise`/`try`/`except` 与 `dict[V]`。完整历史见
[`CHANGELOG.md`](CHANGELOG.md) ·

长期事实：自举固定点（生成的 C + 目标文件逐字节一致）· 标准库内置
UI 工具箱 v3（Qt 级：布局管理器、文本输入、焦点链、20 余控件、浮层覆盖
——`examples/ui_gallery.ax`）· 零 LLVM 依赖：C 发射后端是唯一后端
（clang 编译生成物）· TS-M1 W1 收官 · 包管理器 beta（PubGrub、策展
registry、npm 桥接）· 单文件安装器（`Aoxn-<version>-Setup.exe`，默认
不下载任何东西）。

安装步骤见 [`docs/install.md`](docs/install.md)，完整语言规范见
[`docs/spec.md`](docs/spec.md)，发布历史见
[`CHANGELOG.md`](CHANGELOG.md)。

## 许可证

Apache-2.0 —— 见 [`LICENSE`](LICENSE)。
