# Aoxn

<img src="icons/mark.svg" width="96" alt="Aoxn" align="right">

**Aoxn** is an AI-native, statically typed, ahead-of-time compiled programming
language. Python-style syntax on the surface, C++-class native performance
underneath: programs lower to ISO C and compile with clang (O3) straight to
machine code 鈥?the compiler itself carries no LLVM dependency.

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

- **Python-style syntax** 鈥?indentation blocks, `def` / `elif` / `pass`,
  `#` comments, `x = 5` type inference, `and` / `or` / `not`, `[0] * n`,
  augmented assignment (`x += 1`, `arr[0] += 10`, `p.f += 1`), `//`,
  unary `+`, and chained comparison (`0 <= x < n`)
- **Native speed** 鈥?generated C compiled by `clang -O3`; measured at parity
  with hand-written C (benchmarks below)
- **Generics** 鈥?`def sort[T, N](arr: [T; N])` monomorphized per call site;
  no boxing, no runtime overhead
- **AI-native tooling** 鈥?every diagnostic can be emitted as structured JSON
  (`--json`), the generated C is dumpable (`aoxn c`), and the semantics are
  deliberately strict and deterministic so AI-generated code is verifiable
- **Value semantics** 鈥?arrays, structs, and strings copy by value; no hidden
  references; array indexing is unchecked, C-style

## Quick start

### Install (one file, Windows)

Download **`Aoxn-..5..9-Setup.exe`** from the
[releases page](https://github.com/AlonechatWorkspace/Aoxn-language/releases)
and double-click it. That single executable carries the compiler, the
standard library, the UI toolkit and the examples 鈥?nothing else to download,
no archive to unpack:

```console
Aoxn Setup
  [======                ]  Installing鈥?
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
**Finish** button only lights up once the toolchain really works 鈥?the
installer compiles and runs a test program as its own check. Open a new
terminal afterwards, then:

```powershell
aoxn run examples\hello.ax
hello, Aoxn
aoxn doctor          # verify any install, any time
aoxn doctor --json   # same report, for scripts and agents
```

Aoxn's first-class platform is **Windows** 鈥?the installer, the release
packaging and the Win32/GDI UI backend live there. **Linux is supported too
(v0.46.0)**: the compiler, the whole test suite and an Xlib/Xft UI backend
run on it 鈥?build from source with `cargo build` after `apt install clang
libx11-dev libxft-dev libfontconfig1-dev`. The single external prerequisite
on either platform is **clang** (the compiler lowers to C and hands it
over); on Windows the installer provisions LLVM through `winget` when it is
missing and linking uses the MSVC Build Tools, which clang finds
automatically. Full guide 鈥?scripted installs, `-Prefix`, uninstall,
troubleshooting: [`docs/install.md`](docs/install.md); the platform story:
[`docs/platform-support.md`](docs/platform-support.md).

With an install in place the standard library resolves **by name** from any
directory 鈥?no path back into the toolchain:

```Aoxn
import * from "stdlib"

def main() -> int:
    print(isqrt(99))
    return 0
```

### From source

Prerequisites: Rust (msvc host), clang, MSVC Build Tools. Since v0.29.0 the
compiler carries no LLVM dependency 鈥?`cargo build` needs only the Rust
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
compiler's only backend since v0.29.0 鈥?the LLVM backend was removed, see
[`docs/llvm-independence-report.md`](docs/llvm-independence-report.md));
`cargo run -- check examples\fib.ax` type-checks a file and prints only the
diagnostics (no C text, no clang 鈥?this is what the IDE's Check button
drives); `cargo run -- run examples\primes.ax --json` emits machine-readable
diagnostics; `--cpu native` targets the host CPU (AVX2 & co.) for maximum
speed 鈥?the default generic CPU keeps compiled output reproducible across
machines.

Since v0.35.0 the compiler can also describe what a program declares,
which is what the IDE's outline, "go to definition" and symbol search read:

```powershell
cargo run -- symbols examples\ui_gallery.ax            # a readable table
cargo run -- symbols examples\ui_gallery.ax --json     # for editors and agents
```

It walks the AST after import resolution, so the listing covers the entry
file **and every module it imports** 鈥?each `def` and `struct` with its
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
sources skips compile and link entirely (measured ~1.1s 鈫?~85ms for
`examples/hello.ax`). Set `AOXN_NO_CACHE=1` to disable it or
`AOXN_CACHE_DIR=<dir>` to relocate it.

## Language tour

```Aoxn
# arrays, structs, strings 鈥?all value types
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
    print("a" < "b")     # true 鈥?strings compare byte-wise
    return 0
```

Types are strict and every expression is checked: `int`/`float` never mix
implicitly, conditions must be `bool`, array indexing is unchecked (C-style),
and every function must return a value on all paths. Strictness is a feature:
the guarantees are simple enough for a machine to reason about.

### v0.51.2 on the surface — Python syntax sugar, part 2

Strings and containers get real indexing, slicing and membership. Every value
below is what CPython 3 prints for the same expression.

```Aoxn
def main() -> int:
    s = "Aoxn"

    print(s[0], s[-1])              # A n      one byte, negative counts back
    print(s[1:3], s[:2], s[2:], s[:])   # ox Ao xn Aoxn
    print(s[-2:], s[:-1], s[3:1])       # xn Aox  (empty)
    print(s[99])                    # empty  -- clamped, never reads past the end

    print("ox" in "Aoxn", "zz" in "Aoxn")     # true false   (substring)
    print(1 in [3, 1, 4], 9 in [3, 1, 4])      # true false   (element)
    print("a" in {"a": 1}, "b" in {"a": 1})   # true false   (key)

    for byte in "abc":
        print(byte)                 # a b c  -- strings are BYTE sequences
    return 0
```

- **`s[i]`** yields a **one-byte string**. There is no `char` type and none is
  coming: `len(s)` has always been `strlen`, so `len("中")` is 3 and a
  character type would contradict that contract.
- **`s[a:b]`** — either end may be open, negative bounds count from the end,
  and both are clamped (`s[1:999]` is the rest of the string). **Arrays are not
  sliceable**: an Aoxn array is `[T; N]` with the length in its type, and it
  already copies on assignment, so `b = a` is its slice.
- **`x in c` / `x not in c`** — the RIGHT operand is the container and picks
  the question: substring, element, or key. The left must be comparable, so a
  struct or array (which has no `==`) is a type error rather than a wrong
  answer.
- **`for c in s`** walks bytes. The length is read **once**, before the loop,
  so a body that appends to the string cannot extend the iteration.

**Two deliberate deviations, both pinned by tests:**

- **An out-of-range index yields `""`.** Indexing an array is unchecked C-style
  and stays that way, but there the caller owns the bytes; inside the
  compiler's own string helpers an unchecked read would be *our* out-of-bounds
  read of someone else's buffer.
- **`not a in b` keeps C's precedence, not Python's.** Aoxn's `not` is the C
  prefix `!`, which binds tighter than every comparison, so it parses as
  `(not a) in b` — and `!` requires a bool, making the unparenthesised
  spelling a **type error** rather than a silent wrong answer. Write
  `not (a in b)`. Moving `not` to Python's level would silently change every
  existing `not b == c`, which is a semantic change to a shipped operator
  rather than new syntax sugar.

Like part 1, none of this reaches a program that does not use it: the emitted
C is byte-identical either way, so the self-host fixed point needs no
`selfhost/` mirror.

### v0.51.0 on the surface — Python syntax sugar, part 1

```Aoxn
def greet(name: string = "world", punct: string = "!") -> string:
    return "hello, " + name + punct

def scaled(v: int, factor: int = 2) -> int:
    return v * factor

def label(n: int) -> string:
    return "pos" if n > 0 else "non-pos"

def main() -> int:
    print(greet(), greet("aoxn", "?"))   # defaults: Python's optional arguments
    print(scaled(21), scaled(21, 5))     # 42, 105
    print(label(1), label(-1))           # conditional expression
    print(2 ** 10, 2 ** 3 ** 2)          # 1024, 512  (right-associative)
    print(-2 ** 2)                       # -4  (binds looser than unary minus)
    return 0
```

- **Default parameter values** — the default is materialized by the CALL
  SITE, so the C prototype is unchanged and a program that spells no default
  emits byte-identical C. A default may not name a parameter (it is written
  into the call), and `extern def` / generic functions may not have one.
- **Conditional expressions** — Python's precedence and right-nesting; both
  branches must have the SAME type, because this language has no implicit
  conversion to fall back on.
- **`**`** — two ints give an exact int, two floats a float, mixed operands
  are an error. A negative int exponent yields 0 (write `to_float(3) ** -1`).
- **`print` takes several values**, space-separated, and `print()` prints an
  empty line.
- The TypeScript front end now lowers `c ? a : b` into the same node.

**v0.51.0 also fixed six defects in the standard library's network and JSON
layer found by an audit** — the most serious being that a cross-origin
redirect carried Anthropic's `x-api-key` to the redirect target, because
WinHTTP follows redirects itself and takes the request headers along.
Redirects are no longer followed automatically (`r.location` carries the
target); the UTF-8 → UTF-16 encoder no longer reads past the end of a
truncated buffer; `j_dumps` terminates on an infinite float and the parser
has a nesting limit; SSE frame accumulation is linear instead of quadratic;
and a truncated HTTP response is now a transport error instead of a clean
`200`. See [SECURITY.md](SECURITY.md) for what is fixed and what is still
open.

### v0.44.0 on the surface 鈥?the standard library starts

Thirteen stdlib modules land as independent files, imported **by name**:

```Aoxn
import * from "stdlib/datetime"
import * from "stdlib/hashlib"

def main() -> int:
    now = datetime_now()                     # local wall clock
    print(datetime_iso(now))                 # 2026-10-05 21:14:07
    print(sha256_str("abc"))                 # ba7816bf8f01cfea鈥?
    print(hmac_sha256("key", "msg"))         # RFC 2104
    print(b64_decode_str("aGVsbG8="))        # hello 鈥?decode now exists
    print(cal_month(now.year, now.month))    # a text month grid
    return 0
```

The batch: `math` (CRT trig/exp + the `fdiv`/`NaN()`/`is_nan` toolkit the
numeric libraries were waiting for), `time` (monotonic + UTC wall clock:
QueryPerformanceCounter / `clock_gettime`), `datetime` (Hinnant civil-date
math, hand-parsed `strftime` subset, local time through `GetLocalTime` /
`localtime_r`), `calendar`, `pathlib`,
`base64` (encode **and** decode, std + URL-safe),
`hashlib` (SHA-256/SHA-1/MD5, one-shot + incremental, pure Aoxn bitwise),
`hmac`, `os` (filesystem core: all-wide UTF-16 on Windows, plain libc on Linux 鈥?
env, cwd, listdir),
`glob` (Python's dotfile rule, sorted output), `json` (the DOM promoted out
of `net/` + file I/O), `bisect`, `heapq`. None of them touches
`stdlib/stdlib.ax`, so the self-host fixed point is untouched. A module that
reached for one platform's API only would link nowhere else, so the
platform-touching ones declare both and pick with `target_os()` 鈥?the rule
and its trap are written up in [`docs/stdlib.md`](docs/stdlib.md), which is
also the per-module reference for the Python deltas and the traps (UCRT
symbol collisions, the zero-extended sentinel).

### v0.43.0 on the surface 鈥?modules you can name

```Aoxn
# util.ax and shapes.ax both define `area`. That used to be a hard error.
import util
import shapes as s
from util import area            # one name, unqualified (imports are top-level)

def main() -> int:
    print(util.area(2))         # qualify across files 鈥?no prefixes needed
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
`import m as n`. A **named import imports exactly what it lists** 鈥?anything
else stays out of scope 鈥?and a module binding is a namespace, not a value, so
`x = util` is an error while `util.f()` is not. A specifier can be written bare
and dotted (`import stdlib.net.json`, dots being the `/` subpath separator) or
quoted; the bare form also looks in the importing file's own directory, so
`import util` finds the `util.ax` next to it. Importing two modules that both
define `f` is fine as long as you reach them through their modules 鈥?
star-importing both into one scope is a conflict, reported by name.

### v0.42.0 on the surface

```Aoxn
def main() -> int:
    assert(0x1F == 31)          # hex / binary literals 鈥?malformed ones name
    assert(0b101 == 5)          #   the offending digit instead of "unknown variable 'x10'"

    i = 0
    while i < argc():           # command-line arguments: count + 0-based get
        print(arg(i))
        i = i + 1

    if argc() == 0:
        exit(2)                 # explicit status, from anywhere

    s = "line\r\n"              # \r and \0 join \n \t \\ \" (six escapes)
    print(len(s))               # 6 鈥?CR and LF are real bytes
    return 0
```

`assert` aborts with the source line (not catchable 鈥?`raise` is for that),
`exit(code)` sets the status, `argc()`/`arg(i)` read what `aoxn run app.ax --
a b` forwarded, and `main` itself takes no parameters. The compiler also has a
**warning tier** now: `W001` reports a local that is assigned and never read
without failing the build, `--json` returns `{"ok":鈥?"errors":[鈥,"warnings":[鈥}`
on success too, and `--clang-arg`/`-g` forwards flags to clang (debug info,
`-fsanitize=undefined`) 鈥?with a failed C compile reported as the `cc` stage
rather than `internal`.

## Standard library and the UI toolkit

`stdlib/stdlib.ax` is written in Aoxn itself: generic `sort` /
`binary_search` / aggregation, a growable `Vec`, byte buffers, file IO and
process spawning. Since v0.44.0 the stdlib is a set of **named modules**
under `stdlib/` 鈥?`math`, `time`, `datetime`, `calendar`, `pathlib`,
`base64`, `hashlib`, `hmac`, `os`, `glob`, `json`, `bisect`, `heapq` 鈥?each
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
and a monospace face (`ui_font_mono(c)` / `ui_font_sans(c)` 鈥?Consolas
on Windows, the fontconfig `Monospace` alias on Linux); **v0.47.0 fixed
the GDI clip stack, which had never restored anything** 鈥?one `ui_textbox`
anywhere in a frame clipped the whole window (the final `BitBlt` included)
to that textbox rect for the rest of the process 鈥?and added clip-aware
draw culling, a 64-entry text-measure cache and a DC state cache in the
Windows backend. **v0.49.0** fixed the remaining silent failures: a 9th
layout level used to pop the 8th (the rest of the frame landed in the wrong
box, with nothing reporting it), `itoa10`'s arena-exhaustion escape returned
UTF-16 where the draw path expects ASCII (so `ui_label_int` drew a one-digit
number once the arena filled), `utf16_write` was O(n虏) because
`while i < len(s)` re-scanned the string once per character (626 碌s 鈫?3 碌s
per 2048-char call), and the X11 clip stack could pop a rect it never pushed.
It also collapsed the fifteen copies of the press-claim/click-release cycle
into `ui_press` / `ui_press_claim` / `ui_press_release` / `ui_pressed`, and
replaced the text box's whole-field measure (an O(n) content hash per frame
to learn a constant line height) with a cached `ui_font_height`.

**v0.50.0 was the second stdlib performance audit** and it swept the whole
tree, not one module: `len()` on a string is `strlen` emitted inline at
every use site, and a loop that WRITES through a pointer (rather than only
reading its string) loses clang's loop-invariance hoist 鈥?so the two UTF-16
encoders under every `os_*` path call and every WinHTTP wide string were
re-scanning the string per character (1950 碌s 鈫?10 碌s per 4096-char call),
and a second family of loops that grew strings with `out = out + 鈥 paid
O(n虏) allocations and O(n虏) leaked bytes (rendering a 200-message
conversation: 2105 ms 鈫?~3 ms). Ten loops now build into byte sinks
(`JOut` / the new `StrBuf`), JSON numbers no longer re-`strlen` the whole
document per token (1018 ms 鈫?7鈥?3 ms for 20 000 numbers), and the
rewrites exposed one new crash class 鈥?pinned as `msg-text-empty` 鈥?where
an empty result wrote its final NUL through a NULL buffer. The two rules
are written up in [`docs/stdlib.md`](docs/stdlib.md); the self-hosting
fixed point re-verifies byte-identical stage-1/stage-2 output against the
changed `stdlib.ax`.

**The toolkit is three portable files plus one backend per platform** 鈥?a
portable core (`ui.ax`), a platform-neutral widget layer (`ui_draw.ax`),
and ONE of `ui_win.ax` (Win32/GDI) or `ui_x11.ax` (Xlib + Xft, v0.46.0) 鈥?
and a program picks it up with one import:

```Aoxn
import * from "stdlib/ui_win"     # Windows: Win32 + GDI (-l user32 -l gdi32)
import * from "stdlib/ui_x11"     # Linux: Xlib + Xft  (-l X11 -l Xft)
```

Widgets never name a platform symbol: they call `plat_*` primitives that
the backend supplies, and the contract tests pin BOTH backends to the same
primitive set, so the widget layer stays testable headlessly and the widget
set is identical everywhere. The X11 backend was removed in v0.30.0 and
rewritten against the v3 contract in v0.46.0 鈥?Xft text, an off-screen
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

Details: [`docs/ui.md`](docs/ui.md) 路 gallery: `examples/ui_gallery.ax` 路
tests: `tests/ui.rs`.

### The API SDKs (v0.41.0)

Two provider SDKs ship in the stdlib over **one shared transport**.
`stdlib/net/` is provider-neutral 鈥?base64/percent/UTF-16 codecs, a JSON
DOM with a builder, HTTPS over WinHTTP, SSE 鈥?and each SDK adds its own
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

`stdlib/anthropic/` (v0.41.0) covers the Messages API 鈥?where the payload
is an array of typed content blocks, not a list of strings 鈥?plus token
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
JSON text fragments 鈥?so `AnAcc` reassembles the stream into a finished
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

Reference: [`docs/openai-sdk.md`](docs/openai-sdk.md) 路
[`docs/anthropic-sdk.md`](docs/anthropic-sdk.md) 路 examples:
`examples/openai_chat.ax`, `examples/anthropic_chat.ax` 路 tests:
`tests/openai_sdk.rs`, `tests/anthropic_sdk.rs`.

## The IDE

Aoxn ships an official editor 鈥?`ide/`, a native desktop app built on
Tauri 2 with a Next.js + Monaco workbench and a small Rust command layer:

- **Explorer** with a project tree (build/dependency directories skipped)
  and real **New file / New folder** 鈥?names are validated in the dialog,
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
- **Outline, go to definition and symbol search** (v0.35.0) 鈥?all three read
  `aoxn symbols --json`, a new compiler command that exports the
  declarations of a file and **everything it imports** from the AST the
  compiler just built. The outline lists the current file's `def`s and
  `struct`s in source order; Ctrl+click jumps to a declaration across an
  `import`; Ctrl+Shift+O searches by name with the origin `file:line`
  beside each hit. Nothing here is a regex over source text 鈥?a parameter
  is never mistaken for a declaration, and an `extern def` is recognisable
  as having no body to jump into.
- **Check / Build / Run** (F7 / F6 / F5) drive the real `aoxn` binary with
  the arguments a user would type 鈥?nothing about the build is
  reimplemented, so a build inside the IDE cannot disagree with a build in
  a terminal. Check runs `aoxn check` (diagnostics only, no C dump).
- **Packages** (v0.33.0) 鈥?a sidebar view over the package manager: the
  workspace's `aoxn.json` with its dependencies and what is installed in
  `aox_modules/`, and buttons for the whitelisted verbs 鈥?Init, Add, Install,
  Update, Outdated, Tree, Audit, Why, Remove. Every command runs the real
  `aoxn pkg` in the opened folder and streams its output verbatim;
  outward-facing or global commands (publish, yank, cache, trust bootstrap,
  npm-import) are refused at the Rust gate, not hidden in the UI. `aoxn
  doctor` lives here too: the status bar's "compiler / clang not found"
  button runs it into the output panel.
- **Resolved versions in the packages panel** (v0.35.0) 鈥?`aoxn pkg list
  --json` and `aoxn pkg outdated --json`, so the panel shows what the
  lockfile actually installed (`2.1.0`, not the manifest's `^2`) with its
  scope and source registry, and marks anything with a newer version. A
  registry that cannot be reached is reported, never shown as "up to
  date".
- **Quick open** 鈥?Ctrl+P files, Ctrl+Shift+P commands, Ctrl+Shift+O symbols.

The workspace is a boundary, not a suggestion: every path arriving from the
webview is canonicalised and refused if it resolves outside the opened
folder (compared component-wise, so `C:\proj-evil` does not pass a
`C:\proj` check), there is no read-any-path command and no shell plugin.

```powershell
cd ide
pnpm install
pnpm ide:dev        # the native app, hot reload
pnpm dev            # browser preview against an in-memory fixture 鈥?
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
| Loop sum | 2脳10鈦?iterations | ~15 ms | ~23 ms |
| Array fill + scan | 2脳10鈦?reads | 86 ms | 99 ms |
| Struct copies (by value) | 7.5脳10鈦?copies | 237 ms | 206 ms |

Native code is native code 鈥?Aoxn sits within noise of clang.

Web servers too: the [`web/`](web/README.md) suite ships an HTTP/1.1 server
written in Aoxn and benchmarks it against the pnpm + Node.js + Next.js stack
on identical routes 鈥?it matches plain Node.js throughput at ~1/50 the p50
latency and serves 26-54x more requests than Next.js, from a single 173 KB
binary with a 5 MB RSS. Numbers: [docs/web-benchmark.md](docs/web-benchmark.md).
Since 2026-10-02 the server also speaks static files (W2): a preloaded
in-memory file table with ETag / `If-None-Match` 鈫?304 / single-range 鈫?206
/ unsatisfiable 鈫?416 / `Cache-Control`, cross-checked against the Node
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
| `src/` | the compiler: lexer 鈫?parser 鈫?typecheck 鈫?C-text codegen 鈫?clang link |
| `src/codegen_c.rs` | the C-emitting backend (the only backend since v0.29.0) |
| `src/paths.rs` | install-layout discovery (`AOXN_HOME` / stdlib / bundled clang) + the `aoxn doctor` report |
| `src/ts/` | the TypeScript front end (TS-M1 W1 complete: S2b type layer + S3 modules) |
| `stdlib/stdlib.ax` | the standard-library core, written in Aoxn itself |
| `stdlib/{math,time,datetime,calendar,pathlib,base64,hashlib,hmac,os,glob,json,bisect,heapq}.ax` | the v0.44.0 module batch 鈥?`docs/stdlib.md` is its reference |
| `stdlib/ui.ax`, `stdlib/ui_draw.ax` | the UI toolkit v3: portable core + platform-neutral widget layer (20+ widgets); `ui.ax` also holds the layout engine, focus chain, text-editing primitives and the shared heap block |
| `stdlib/ui_win.ax`, `stdlib/ui_x11.ax` | the two `plat_*` backends: Win32/GDI (Windows) and Xlib/Xft (Linux, v0.46.0) |
| `stdlib/net/` | the shared API-client layer (v0.41.0, moved out of `stdlib/openai/`): codecs, JSON DOM + builder, WinHTTP transport, SSE 鈥?used by both SDKs |
| `stdlib/openai/` | the OpenAI SDK (v0.40.1): headers, client, resources, accessors, `OaStream` 鈥?`docs/openai-sdk.md` |
| `stdlib/anthropic/` | the Anthropic SDK (v0.41.0): content blocks, tools/schemas, client, `AnStream` + `AnAcc` 鈥?`docs/anthropic-sdk.md` |
| `docs/openai-sdk.md`, `docs/anthropic-sdk.md` | the two SDK references |
| `docs/stdlib.md` | the standard-library module reference (v0.44.0 batch): per-module APIs, Python-parity deltas, the traps |
| `examples/*.ax` | demo programs (hello, fib, primes, stdlib_demo, ui_demo, ui_gallery, math/time/pathlib/hashlib/os/json/bisect _demo, 鈥? |
| `dist/package.ps1`, `src/setup/` | the single-file installer: the packager and the installer stub it fills |
| `selfhost/` | the compiler rewritten in Aoxn (fixed point reached) |
| `ide/` | the Aoxn IDE: Tauri 2 + Next.js + Monaco workbench (`ide/src-tauri` is its own Cargo workspace) |
| `web/` | web benchmark suite: an HTTP server in Aoxn vs pnpm+Node.js+Next.js |
| `tests/` | end-to-end tests: compile 鈫?run 鈫?verify output, including the self-hosting fixed point (byte-identical generated C + objects) and the install layout |
| `docs/install.md` | install guide (Windows) |
| `docs/ide.md` | the IDE: architecture, security boundary, development workflow |
| `docs/spec.md` | full language specification |
| `docs/stdlib-todo.md` | the standard-library roadmap: the 31 remaining Python stdlib modules batched P0鈥揚3, plus the 15 third-party libraries (`numpy`, `pandas`, `flask`, `matplotlib`, `pytorch`, 鈥? ordered by dependency chain 鈥?each with the language constraints it runs into |
| `docs/language-gaps.md` | why some of those cannot be built: every missing language feature (no references, no `enum`/tagged unions, no tuples, no namespaces, no GC, 鈥? with what it blocks, which files a change touches, its cost, and a ranked order for closing them |
| `wiki/` | bilingual (涓枃/English) wiki 鈥?frozen since v0.29.3; `docs/` is the living documentation |

## Testing & CI

`cargo test` runs the end-to-end suite 鈥?**1017 tests** in the compiler
workspace (pipeline 172, compiler unit tests 18, installer stub 3, TypeScript
front end 35, UI 14, install layout 6, CSS assets 21, CSS assets v0.36 18,
symbol export 8, OpenAI SDK 2, Anthropic SDK 2, and one driver test per
v0.44.0 stdlib module group: hashlib, math, pathlib, containers, os+glob,
json file-IO 鈥?plus `datetime`, which carries two since v0.50.1 (the driver
plus the two-platform clock-symbol pin), `stdlib_defect_pins`, the v0.48.0
regression suite for the JSON builder corruption and the quadratic loops, and
`stdlib_perf_pins`, the v0.50.0 pins for the `len()`-is-`strlen` and
concatenation-in-a-loop rewrites) plus the `aoxn-pkg` crate's 94 via
`bash run_pkg_tests.sh` 鈥?**385 in total** 鈥?where every pipeline test
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
suite on Linux 鈥?proving the compiler, the dual-platform stdlib and the
fixed point on a second host 鈥?and then builds
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
import resolves its entry through the package's `aoxn.json` 鈥?`main`,
`exports` (incl. `pkg/sub` subpaths), and `types` 鈥?so an installed package
whose entry is not `index.ax` is importable:

```Aoxn
import * from "http"          # 鈫?aox_modules/http/aoxn.json `main`/`exports["."]`
import * from "http/client"   # 鈫?exports["./client"]
```

A package without a manifest falls back to the legacy `aox_modules/<name>`
directory probe (`<name>.ax` / `index.ax`).

**v0.32.0** brings the everyday features the manager was missing, measured
against pip and pnpm:

```bash
aoxn add -D harness        # devDependencies 鈥?tooling that must not ship
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

A registry may additionally be **curated** 鈥?one repository carrying the
packages, a `trust.json` review record per package, and `advisories/`:

```bash
aoxn trust bootstrap https://github.com/AlonechatWorkspace/Aoxn-trusted-third-party-package
aoxn trust check http --tier audited   # CI gate
```

Install warns about packages a curated registry has no reviewed record for,
and says nothing for registries that make no trust claims. The trust index
is a curation signal, not a signature 鈥?the cryptographic anchor is still
the manifest hash pinned in `aoxn.lock`. Layout and schema:
[`docs/trusted-registry.md`](docs/trusted-registry.md).

The npm bridge (`aoxn npm-import`, the npm CLI as transport) imports Aoxn
packages published to any npm-compatible registry into `vendor/<name>/` as
path dependencies; plain JavaScript packages are rejected.

## CSS assets

A `.css` file you `import` is a **build asset, not source**. It is bundled
(`@import`s inlined in place), minified, fingerprinted, and embedded into the
binary 鈥?no output directory, no file layout to agree on:

```Aoxn
import * from "./style.css"

def main() -> int:
    print(styles())               # the whole bundle
    print(styles_fingerprint())   # "18603d4d4686e2cc.css"
    return 0
```

Minification removes comments and redundant whitespace, and deliberately
nothing else 鈥?no selector merging, no reordering 鈥?so the emitted CSS is a
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

**Tailwind** enters as pre-generated CSS, not as a dependency 鈥?it is a
plain-JavaScript npm package, which `aoxn npm-import` rejects by design, and
shelling out to its CLI would make Node a build prerequisite. Either run
`npx tailwindcss -o generated.css` and import the result, or let the compiler
generate a documented utility subset:

```sh
aoxn build app.ts --tailwind
```

It scans `class`/`className` attributes only 鈥?prose is never mined 鈥?and
names any utility it does not cover instead of dropping it silently.

**Serving assets from disk.** `--emit-assets <dir>` writes the fingerprinted
bundle, each stylesheet and every `url()` target beside the executable, with
`url()`s rewritten to the emitted names. A program finds them without knowing
any build-time path:

```Aoxn
print(asset_path(styles_fingerprint()))   # 鈥assets\82b4fb25鈥?css
```

Full reference, including limits: [`docs/css-assets.md`](docs/css-assets.md).


## Status

**v0.51.2** — **Python syntax sugar, part 2: strings and containers get real
indexing, slicing and membership** —
427 tests green (327 in the compiler workspace + 100 in `aoxn-pkg`; the IDE
adds 36 Rust + 62 frontend tests of its own) ·

`s[i]` yields a **one-byte string**, `s[a:b]` slices with open and negative
bounds (both clamped), `x in c` / `x not in c` answers **substring**, **element**
or **key** depending on the right operand, and `for c in s` walks bytes. Every
runtime value matches CPython 3.

**Two deviations, both deliberate and both pinned by a test.** An
out-of-range index yields `""` instead of reading past the terminator —
unchecked C-style indexing is right where the caller owns the bytes, and wrong
inside the compiler's own helpers, which would be reading *someone else's*
buffer. And `not a in b` keeps C's precedence, so the unparenthesised spelling
is a **type error** rather than a silent wrong answer: moving `not` to Python's
level would silently change every existing `not b == c`, which is a semantic
change to a shipped operator, not new syntax.

**Nothing here reaches a program that does not use it.** The emitted C is
byte-identical either way — checked against `main` on four existing examples —
so the self-host fixed point needs no `selfhost/` mirror, the same status
v0.51.0's sugar has. The `in` helpers are keyed in **sorted** order rather
than hash order, and a test runs the compiler five times in five separate
processes over one source with three different array element types, because
inside a single process the allocator hides exactly this class of bug.

**v0.51.1** — the four `crates/aoxn-pkg` findings and the three IDE findings
from the audit are closed; see [SECURITY.md](SECURITY.md) ·

**v0.51.0** — Python syntax sugar part 1 (default parameter values,
conditional expressions, `**`, multi-argument `print`) plus **six standard
library security fixes** ·

**v0.50.10** — **`aoxn c` was not reproducible: the array-replication helper was named after a heap address** ·

**six runs, six different files.** `[X] * N` emitted its `static` fill helper
under a C identifier built from the `Expr`'s heap address, so the address went
straight into the generated C. The helper table was keyed by that address too,
so even the *order* the definitions appeared in was random, and the
non-determinism reached the output even for a single site 路

**the two backends never agreed on this naming in the first place.**
`selfhost/codegen.ax` has always numbered these from a plain counter (`g.repn`
鈫?`ax_rep_0`, `ax_rep_1`, 鈥?in first-use order) while the Rust backend invented
an address, so **no program using array replication could ever have passed the
fixed point**. It simply had never been written. Sites are now identified by
`lit_id`, the id the parser hands a literal in source order, and numbered in
first-use order 鈥?the spelling the self-hosted emitter already produced 路

**the test that claims to guard backend agreement was comparing selfhost
against itself.** `selfhost_driver_self_compiles` builds the same driver twice
and compares the C they emit, but the driver is an Aoxn program that imports
`selfhost/driver.ax`, so **both** stages emit through `selfhost/codegen.ax`.
`src/codegen_c.rs` is never compared against it. That is exactly how the
address-named helper survived. (The two do currently diverge on `extern def`
declaration order 鈥?Rust sorts by C name, the self-hosted emitter keeps
first-use order. Both are individually deterministic, so it is a gap in what
the fixed point *proves*, not a live correctness bug.) 路

**the pin that catches this has to cross a process boundary.** Inside one
process the allocator hands the same address back for the same node, so an
in-process comparison passes happily while the bug is live. The test runs the
binary five times in five separate processes and requires byte-identical C 路

**the previous milestone** (v0.50.8) 鈥?**the v0.50.7 Linux-simulation pin only worked on Windows** 路
386 tests green (293 in the compiler workspace + 94 in `aoxn-pkg`; the IDE
adds 36 Rust + 62 frontend tests of its own) 路

**the pin that checks Linux from Windows was itself half-blind.** It rewrote
`__builtin_strcmp("windows", "windows")` to `"linux"` and asserted that
pattern was present 鈥?but the backend spells the fold's second argument with
the *build's* OS name, so on a Linux build the text already reads
`("windows", "linux")` and the pin failed there instead of running. Both
spellings are now accepted and normalised, and on a Linux build the
replacement is a no-op, which is exactly right: that object already **is**
the Linux object 路

**the "the check is void" assertion earned its keep.** It exists to stop a
renamed fold from turning the pin into something that passes while measuring
nothing, and that is precisely what it caught 鈥?a pin that would have been
silently meaningless on half the CI matrix. A guard you wrote for one failure
class turning out to cover a second, in the same class, is the best outcome a
guard can have 路

**the previous milestone** (v0.50.7) 鈥?**the HTTP transport did not link on
Linux, and you can now check that from Windows** 鈥?
**one use-after-free, nine reported failures** 鈥?on Linux it did not crash so
much as lie. The instrumented CI run said it outright:
`FAIL chdir-no-move 鈥?sandbox`, meaning `os_cwd()` answered `.../sandbox`
*before* the `chdir` that was supposed to produce that path, and answered the
same string again after. `as_string` is a **cast, not a copy**
(`codegen_c.rs` emits `((char*)(x))`), so it aliases the buffer it is given 鈥?
and the POSIX arm returned `as_string(p)` then freed `buf` (`p` *is* `buf`).
glibc hands back the **same 4096-byte chunk** on the next request, so the
second call overwrote the first answer in place: the two strings compared
**equal with the working directory having moved**. Anything that captured a
cwd and restored it later stayed in the wrong directory, so every relative
path after it failed 鈥?the empty glob results and the dead json file I/O were
all symptoms of that one line 路
**a use-after-free can lie instead of crashing, so pin it at RUNTIME.** A
static scan cannot see this class: the alias ran through two different
variable names (`as_string(p)` aliasing a buffer freed as `buf`), which is
exactly what defeated the first attempt at such a check. The new
`cwd-answers-own-their-buffer` pin asserts the observable property 鈥?two
`os_cwd()` calls around a `chdir` must differ. All 60 `as_string` sites in
`stdlib/` were audited; this was the only real one 路
**and the lesson about the v0.50.4 instrumentation**: a bare `FAIL chdir`
told us nothing, and splitting it into named terms turned the next failure
into a one-line diagnosis. When a failure's cause is unknown, spend the edit
on the diagnostic rather than a speculative rewrite 路

**the previous milestone** (v0.50.4) 鈥?**the stdlib built paths with a
hardcoded `\`** 鈥?

**`glob.ax` and `pathlib.ax` spelled every joined path with `"\\"`**, so on
Linux a nested glob walked a path that names nothing:
`glob("sandbox/sub/*.txt")` reached `os_exists("sandbox\\sub")` 鈥?false 鈥?and
returned empty, while `path_join` handed callers a string no file answers to.
Five sites: `glob_join`, `glob`'s two root branches, `path_join`,
`path_with_suffix`, `path_with_name`, and `path_norm`'s join byte. pathlib now
publishes **`path_sep()` / `path_sep_byte()`**, and the asymmetry is the
design: `path_is_sep` accepts BOTH separators on INPUT (a Windows path may
carry either), but what a function BUILDS must be spelled the way the
platform spells paths, or it is a different path 路
**and the tests were pinning the bug, not catching it** 鈥?`tests/pathlib.rs`
asserted `path_join("a","b") == "a\\b"` and `tests/osglob.rs` compared glob
hits against `"sandbox\\a.txt"`, so on Windows they passed, and on Linux they
would have failed on the *spelling* even after the code was right. Both build
their expectations from `path_sep()` now, and a new
`cross_platform_modules_never_hardcode_a_path_separator` pin scans the
dual-platform modules for the construct (comment-stripping, Windows-only
modules exempt because they name `\` inside a `target_os()` arm) 路
**open, and instrumented rather than guessed**: `osglob`'s `chdir` check and
three json file-I/O checks failed on Linux for a reason that turned out to sit
upstream of all of them 鈥?see v0.50.5 below 路

**the previous milestone** (v0.50.3) 鈥?**the stdlib clock linked on Linux and
still told the wrong time: 369 years ahead** 鈥?

**`time_unix_ft` returns 100 ns ticks from the 1970 origin on BOTH
platforms** 鈥?the Windows branch subtracts the 1601 gap from
`GetSystemTimeAsFileTime`, and `clock_gettime(CLOCK_REALTIME)` is already on
1970, so the POSIX branch subtracts nothing. v0.50.1 read the name (`..._ft`,
`FT_UNIX_EPOCH`) as "raw FILETIME" and **added** the gap instead. Every
consumer divides that value, so `time_unix`, `time_unix_ms`,
`datetime_utcnow` and `datetime_now` were all 11644473600 s out:
`13436106024` instead of `1791632424`. The Windows branch was always right 路
**a symbol-table test proves linkage, never values** 鈥?the v0.50.1 CI pin
(read the object's undefined symbols) passed while the module answered
`FAIL now-year 2395`. Worse, `time-unix-ms-agrees` passed too: it compares
two readings of the same clock, so it is blind to ANY constant offset. The
absolute check, `time-unix-sane`, is the one that caught it, and it is now
labelled as such 路
**the failure was a misread contract, so the comments were the real bug**:
`time_unix_ft`'s doc said "raw 100 ns FILETIME ticks since 1601-01-01",
which was wrong for Windows too 鈥?the code right below it subtracts the
gap. Header, function comments and `docs/stdlib.md` now state the
normalisation and name the 369-year cost of reading it backwards 路
**new check `now-is-local-and-coherent`**, because nothing covered the POSIX
`struct tm` field offsets. A wrong offset still yields a *well-formed*
`DateTime` 鈥?shifting the day field by 9 leaves `now-year`, `now-iso-len`
and `utcnow-year` all green, which is exactly what the new check catches by
round-tripping `datetime_now()` through `datetime_to_unix` and demanding it
land within a day of `time_unix()` 路

**the previous milestone** (v0.50.2) 鈥?**the seven open Dependabot alerts:
six closed, one with nothing to bump** 鈥?

**two Next.js cache-poisoning advisories** (v0.50.2) 鈥?an SSG/ISR cache
poison that becomes cross-user content substitution and a persistent DoS in
a self-hosted app. The benchmark app moved 15.5.26 鈫?**15.5.27**, the
official backport release; 15.x is held on purpose rather than taking 16.x,
because the benchmark compares bodies and latency against the Aoxn server
and a major bump would move that baseline for nothing 路
**sharp (libvsvg CVE-2026-96889) and source-map-js (event-loop DoS through
indexed source-map section offsets)** (v0.50.2) 鈥?both transitive through
next, and bumping next alone does NOT move them (sharp 0.35.4 鈫?0.35.5,
source-map-js 1.2.1 鈫?1.2.2 via `pnpm update --depth Infinity`: the
difference between bumping the package the alert names and having that
package actually in range) 路
**two DOMPurify `IN_PLACE` XSS advisories** (v0.50.2) 鈥?a node-removing
`afterSanitize` hook leaves a detached subtree's handlers armed, and a
force-removed rawtext root's text carries markup that a pure HTML reparse
executes. dompurify 3.4.15 鈫?**3.4.16** through a `pnpm.overrides` entry,
because monaco-editor pins it to an EXACT version and 0.57.0 is monaco's
latest release: there was no bump to make, only an override 路
**and one alert has no fix to ship** (v0.50.2) 鈥?RUSTSEC-2024-0429
(unsound `VariantStrIter` iterators) is patched only in glib 鈮?0.20, while
glib reaches the IDE's lockfile through wry 0.57.0 鈫?`gtk ^0.18` 鈫?
`glib ^0.18`, with wry and tao both already at their latest releases, so
`cargo update` correctly leaves glib at 0.18.5 and there is no version to
move. `ide/src-tauri/audit.toml` records the acceptance in-tree with its
exit condition; the unsound path is a Linux-only transitive webview
dependency this app never calls.

**the previous milestone** (v0.50.1) 鈥?the stdlib clock modules stopped
being Windows-only: `stdlib/time.ax` and `stdlib/datetime.ax` (v0.44.0)
called `GetLocalTime`, `QueryPerformanceCounter`,
`GetSystemTimeAsFileTime`, `GetTickCount64` and `Sleep` with no guard at all,
so every program importing them died at the link step on `ubuntu-latest`
(`undefined reference to 'GetLocalTime'`) with nothing pointing at the
cause, while Windows never noticed. Both are dual-platform now in the shape
`os.ax` has used since v0.46.0 鈥?POSIX `clock_gettime` / `nanosleep` /
`localtime_r` selected by `target_os()` 鈥?and the guard sits in the function
that makes the call, because the compiler emits every body. Two new tests,
now 290 in the workspace 路

**the previous milestone** (v0.50.0) 鈥?**the second stdlib audit: the
`len()`-is-`strlen` sweep finished, and the concatenation-in-a-loop sweep
that followed it** 鈥?

**thirteen loops re-scanned a string once per character** (v0.50.0) 鈥?
`len()` on a string is `strlen` emitted inline at every use site, and a loop
that WRITES through another pointer 鈥?rather than only reading its string 鈥?
loses clang's loop-invariance hoist, so the whole string is re-scanned per
character. The worst copies sat under every `os_*` path call and every
WinHTTP wide string (the UTF-16 encoders: 1950 碌s 鈫?10 碌s per 4096-char
call); the URL and header scanners, the JSON number path (1018 ms 鈫?7鈥?3 ms
for 20 000 numbers) and the glob and pathlib scanners were the rest 路
**ten loops built strings with `out = out + 鈥** (v0.50.0) 鈥?concatenation
allocates a fresh buffer per step and abandons the old one, so rendering a
200-message conversation cost 2.1 s with every intermediate leaked: ~3 ms
now, into `json.ax`'s existing `JOut` and the new `StrBuf` in `stdlib.ax`.
No output changed; every rewrite is pinned byte-identical 路
**the rewrites exposed one new crash class** (v0.50.0) 鈥?a byte sink whose
result may stay EMPTY wrote its final NUL through a buffer that was never
allocated (`0xC0000005` on a response with no text block 鈥?a pure tool call,
a normal answer). All three joiners answer `""` now; `msg-text-empty` is the
pin 路
**new primitives** (v0.50.0): `buf_str(p, n)`, the pointer-plus-length escape
from strlen for callers that already know a span (a parser token, an SSE
line), and `StrBuf` / `sb_*`, a small geometric byte sink. `stdlib.ax` is on
the self-host critical path, so both are plain Aoxn; the fixed point
re-verifies byte-identical stage-1/stage-2 output against the changed file.

**the previous milestone** (v0.49.0) 鈥?the UI toolkit's remaining silent
failures and the fifteen copies of its input cycle: a 9th layout level used
to pop the 8th (the rest of the frame landed in the wrong box, with nothing
reporting it), `itoa10`'s arena-exhaustion escape returned UTF-16 where the
draw path expects ASCII (so `ui_label_int` drew one digit once the arena
filled), `utf16_write` was O(n虏) (626 碌s 鈫?3 碌s per 2048-char call 鈥?the
trap v0.50.0 then found in its two twins), the X11 clip stack could pop a
rect it never pushed, and the press-claim/click-release cycle is one
`ui_press` family now. No widget signature changed 路

**the JSON builder could shrink a parsed node's buffer under you** (v0.48.0) 鈥?
the parser adopts a `Vec`'s child buffer but leaves the node's capacity at 0,
and the grow read that as "default 8", doubled to 16, and `realloc`'d a
20-member object's 256-byte block **down** to 128 before writing slot 20. Any
parsed object or array with 16+ members died with `0xC0000005` the first time
a field was added 鈥?which is exactly what `jb_set_raw` exists to do, and why
it survived: the SDK tests only ever built fresh nodes. Confirmed
differentially (8 members fine, 20 members access violation). A grow now
floors the capacity at the live child count 路
**and `len()` on a string is `strlen` emitted inline at every use site**,
which made three loops quadratic: `j_at`'s end-of-input test (the JSON parser
ran at ~33 KB/s 鈥?a 266 KB response took 7 s), `jp_str`'s buffer size (every
string allocated the whole document: a 330 KB payload with 20 000 strings
peaked at 325.8 MB), and `b64_decode`'s loop bound (1.4 MB did not finish in
600 s). `j_dumps` compounded it 鈥?concatenation allocates a fresh buffer per
step and abandons the old one, so 20 000 keys touched **8627.8 MB and 28.4 s**.
All four are now linear: 266 KB parses in 7 ms, the same 20 000-key response
serializes in 282 ms at 10.2 MB, and base64 is 3 ms for 2.8 MB. Also fixed: an
unbounded JSON exponent (`1e999999999` spun for minutes 鈥?a reachable DoS on
any untrusted JSON), base64's rejection paths leaked a buffer whose length the
sender chooses, and `os_has_env` read a stale `GetLastError` that Win32 does
not clear on success 鈥?so a variable that really existed answered `False`
after any earlier miss 路

**the previous milestone** (v0.47.0) 鈥?the UI toolkit finally paints what it draws, and stops paying for what
nobody can see 鈥?the GDI clip stack had never restored
anything (`IntersectClipRect` only ever narrows, so the old pop narrowed it
further), which meant **one `ui_textbox` anywhere in a frame left the whole
window clipped to that textbox rect for the rest of the process** 鈥?
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
ms/frame) because the old one was not drawing most of the frame 路

**the previous milestone** (v0.46.0) brought Linux back and gave the
toolkit a second backend through the same `plat_*` contract 鈥?v0.30.0 had
cut the platform list to Windows alone; v0.46.0 restores Linux for the
compiler (ELF objects, `-lm` linked, no `/STACK` flag), the whole test
suite (a new `ubuntu-latest` CI job) and a **rewritten `stdlib/ui_x11.ax`**:
Xlib for the window and input, Xft for antialiased UTF-8 text, an
off-screen Pixmap for double buffering, the X11 selection protocol for the
clipboard, a per-frame `XQueryKeymap` sweep so held keys read continuously
down instead of flickering between autorepeat pairs, and the close request
travelling through shared-block slot 824 (Aoxn passes `UI` by value, so
`plat_close` cannot set a field the app loop would see). `target_os()` now
folds the OS the compiler was BUILT on 鈥?`"windows"` or `"linux"` 鈥?and the
self-hosted compiler needs no platform check of its own: its builtin emitter
calls `target_os()`, a value burned in by whatever compiler built it, so
every stage of the fixed-point chain on one machine folds the same host OS
and the fixed point stays byte-identical by construction. The stdlib
branches rather than forks: `os.ax` serves Linux through plain libc
(mkdir/stat/opendir/readdir/getenv/setenv, the glibc struct offsets
documented in the header) while the `W` Win32 calls keep serving Windows 鈥?
an unreferenced `extern def` emits no symbol reference, so each platform
links only its own side; `exe_path()` answers through `/proc/self/exe` on
Linux. What stays Windows-only does so honestly: the HTTP transport rides
WinHTTP and the entry points return a clean "no transport on this platform"
error on Linux rather than failing to link, and the single-file installer
remains a Windows artifact. That release also added horizontal scroll areas
(`ui_scroll_begin_h` / `ui_scroll_end_h`) and a monospace face
(`ui_font_mono` / `ui_font_sans`; the clip stack intersects, so vertical and
horizontal scroll areas nest) 路

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
[`CHANGELOG.md`](CHANGELOG.md) 路

standing facts: self-hosting fixed point (byte-identical generated C +
object files) 路 UI toolkit v3 in the stdlib (Qt-grade: layout managers,
text input, focus chain, 20+ widgets, floating overlays 鈥?
`examples/ui_gallery.ax`) 路 no LLVM dependency: the C-emitting backend is
the only backend (clang compiles it) 路 TS-M1 W1 complete 路 package manager
beta (PubGrub, curated registries, npm bridge) 路 one-file installer
(`Aoxn-<version>-Setup.exe`) that downloads nothing by default.

See [`docs/install.md`](docs/install.md) to install,
[`docs/spec.md`](docs/spec.md) for the complete language specification and
[`CHANGELOG.md`](CHANGELOG.md) for the release history.

## License

Apache-2.0 鈥?see [`LICENSE`](LICENSE).

---
---

# 涓枃

**Aoxn** 鏄竴闂?AI 鍘熺敓銆侀潤鎬佺被鍨嬨€佹彁鍓嶇紪璇戠殑缂栫▼璇█锛氳〃闈㈡槸 Python 寮忚娉曪紝
搴曚笅鏄?C++ 绾у埆鐨勫師鐢熸€ц兘鈥斺€旂▼搴忛檷绾т负 ISO C 鍚庣敱 clang锛圤3锛夌洿鎺ョ紪璇戞垚鏈哄櫒鐮侊紝
缂栬瘧鍣ㄨ嚜韬笉鎼哄甫浠讳綍 LLVM 渚濊禆銆?

## 涓轰粈涔堥€?Aoxn

- **Python 寮忚娉?* 鈥斺€?缂╄繘鍧椼€乣def` / `elif` / `pass`銆乣#` 娉ㄩ噴銆?
  `x = 5` 绫诲瀷鎺ㄦ柇銆乣and` / `or` / `not`銆乣[0] * n`銆佸寮鸿祴鍊?
  锛坄x += 1`銆乣arr[0] += 10`銆乣p.f += 1`锛夈€乣//`銆佷竴鍏?`+`銆侀摼寮忔瘮杈?
  锛坄0 <= x < n`锛?
- **鍘熺敓閫熷害** 鈥斺€?鐢熸垚鐨?C 浜ょ粰 `clang -O3` 缂栬瘧锛屽疄娴嬩笌鎵嬪啓 C 鍚岀骇锛堝熀鍑嗚涓嬶級
- **娉涘瀷** 鈥斺€?`def sort[T, N](arr: [T; N])` 鎸夎皟鐢ㄧ偣鍗曟€佸寲锛屾棤瑁呯銆佹棤杩愯鏃跺紑閿€
- **AI 鍘熺敓宸ュ叿閾?* 鈥斺€?璇婃柇鍙緭鍑虹粨鏋勫寲 JSON锛坄--json`锛夈€佺敓鎴愮殑 C 鏂囨湰鍙?
  瀵煎嚭锛坄aoxn c`锛夛紝璇箟鍒绘剰淇濇寔涓ユ牸涓庣‘瀹氾紝AI 鐢熸垚鐨勪唬鐮佸彲琚満鍣ㄩ獙璇?
- **鍊艰涔?* 鈥斺€?鏁扮粍銆佺粨鏋勪綋銆佸瓧绗︿覆鎸夊€煎鍒讹紝娌℃湁闅愯棌寮曠敤锛涙暟缁勭储寮曚笉妫€鏌ワ紙C 椋庢牸锛?

## 蹇€熶笂鎵?

### 涓€涓?exe 瑁呭叏閮紙Windows锛?

浠?[releases 椤甸潰](https://github.com/AlonechatWorkspace/Aoxn-language/releases)
涓嬭浇 **`Aoxn-..5..9-Setup.exe`**锛屽弻鍑诲嵆鍙€?*杩欎竴涓?exe 閲屽氨甯︾潃缂栬瘧鍣ㄣ€佹爣鍑嗗簱銆?
UI 宸ュ叿绠卞拰绀轰緥绋嬪簭**鈥斺€斾笉鐢ㄥ啀涓嬭浇鍒殑锛屼篃涓嶇敤鑷繁瑙ｅ帇锛?

```console
Aoxn Setup
  [======                ]  Installing鈥?
    23 files installed
  ==> Adding aoxn to your PATH
  ==> Checking the C toolchain (clang)
    clang: C:\Program Files\LLVM\bin\clang.exe
  ==> Checking the installation with 'aoxn doctor'
    [ok  ] install root C:\Users\you\AppData\Local\aoxn
    [ok  ] smoke test   ok  (aoxn-doctor-ok)
  [Finish]
```

鍏ㄩ儴钀藉湪 `%LOCALAPPDATA%\aoxn`锛坄bin\aoxn.exe`銆乣lib\stdlib\`銆乣examples\`锛夛紝
`aoxn` 杩?PATH锛涙寜閽彉鎴?**Finish** 涔嬪墠锛屽畨瑁呯▼搴忎細**鐪熺殑缂栬瘧骞惰繍琛屼竴涓▼搴?*
鑷锛岄€氳繃浜嗘墠绠楄濂姐€傝瀹屽紑涓€涓柊缁堢锛?

```powershell
aoxn run examples\hello.ax
hello, Aoxn
aoxn doctor          # 闅忔椂鑷
aoxn doctor --json   # 鍚屾牱鐨勪俊鎭紝JSON 杈撳嚭
```

Aoxn 鐨?*绗竴姊槦骞冲彴鏄?Windows**鈥斺€斿畨瑁呭櫒銆佸彂甯冩墦鍖呬笌 Win32/GDI UI 鍚庣閮?
鍦ㄩ偅閲屻€?*Linux 鍚屾牱鍙楁敮鎸侊紙v0.46.0锛?*锛氱紪璇戝櫒銆佸畬鏁存祴璇曞浠朵笌 Xlib/Xft UI
鍚庣閮借兘璺戔€斺€擿apt install clang libx11-dev libxft-dev libfontconfig1-dev`
涔嬪悗 `cargo build` 浠庢簮鐮佹瀯寤哄嵆鍙€備袱涓钩鍙颁笂鍞竴鐨勫閮ㄤ緷璧栭兘鏄?**clang**
锛圓oxn 鐢熸垚 C 鍚庝氦缁欏畠缂栬瘧閾炬帴锛夛紱Windows 涓婂畨瑁呭櫒浼氬湪缂哄け鏃剁敤 `winget` 瑁?
LLVM锛岄摼鎺ヨ繕闇€瑕?MSVC Build Tools锛宑lang 浼氳嚜鍔ㄦ帰娴嬨€傞潤榛樺畨瑁咃紙鑴氭湰 / CI锛?
鏃犻渶绐楀彛锛夈€佽嚜瀹氫箟鐩綍銆佸嵏杞戒笌鎺掗敊瑙?[`docs/install.md`](docs/install.md)锛?
骞冲彴鍏ㄦ櫙瑙?[`docs/platform-support.md`](docs/platform-support.md)銆?


瑁呭ソ鍚庝换浣曠洰褰曢兘鑳芥寜鍚嶅瓧寮曠敤鏍囧噯搴擄紝涓嶉渶瑕佸啓宸ュ叿閾鹃噷鐨勮矾寰勶細

```Aoxn
import * from "stdlib"

def main() -> int:
    print(isqrt(99))
    return 0
```

### 浠庢簮鐮佹瀯寤?

鍓嶇疆锛歊ust锛坢svc 涓绘満锛夈€乧lang銆丮SVC Build Tools銆傝嚜 v0.29.0 璧风紪璇戝櫒涓嶅啀渚濊禆
LLVM鈥斺€擿cargo build` 鍙渶瑕?Rust 宸ュ叿閾撅紝缂栬瘧绋嬪簭鍙渶瑕?clang銆傝
[`docs/platform-support.md`](docs/platform-support.md)銆?

```powershell
cargo build
cargo run -- run examples\hello.ax
```

涓夊钩鍙扮殑棰勭紪璇戜簩杩涘埗鍙戝竷鍦?
[releases 椤甸潰](https://github.com/AlonechatWorkspace/Aoxn-language/releases)銆?

缂栬瘧鍑虹嫭绔嬬殑鍘熺敓鍙墽琛屾枃浠讹細

```powershell
cargo run -- build examples\fib.ax -o fib.exe
.\fib.exe
```

鏇村锛歚cargo run -- c examples\fib.ax` 鎵撳嵃鐢熸垚鐨?C 鏂囨湰锛坴0.29.0 璧疯繖鏄敮涓€
鍚庣鈥斺€擫LVM 鍚庣宸茬Щ闄わ紝瑙?
[`docs/llvm-independence-report.md`](docs/llvm-independence-report.md)锛夛紱
`cargo run -- check examples\fib.ax` 鍙仛绫诲瀷妫€鏌ャ€佸彧杈撳嚭璇婃柇锛堜笉鎵撳嵃 C銆?
涓嶈皟 clang鈥斺€擨DE 鐨?Check 鎸夐挳椹卞姩鐨勫氨鏄畠锛夛紱
`--json` 杈撳嚭鏈哄櫒鍙璇婃柇锛沗--cpu native` 閽堝瀹夸富 CPU锛圓VX2 绛夛級鏋佽嚧鎻愰€熲€斺€?
榛樿鐨勯€氱敤 CPU 淇濊瘉缂栬瘧浜х墿璺ㄦ満鍣ㄥ彲澶嶇幇銆?

缂栬瘧閫熷害浼樺厛鏃剁敤浼樺寲绾у埆锛堢骇鍒€夋嫨缂栬瘧鐢熸垚 C 鎵€鐢ㄧ殑 clang `-O`锛夛細

```powershell
cargo run -- run examples\fib.ax --O1     # clang -O1锛氱紪璇戞洿蹇紱榛樿浠嶆槸 O3
cargo run -- build examples\fib.ax --O0   # clang -O0
```

`aoxn run` 涓?`aoxn build` 鎸?绋嬪簭鍐呭鍝堝笇 + 缂栬瘧鍣?+ 閫夐」"缂撳瓨浜х墿
锛坄target/cache`锛夛細閲嶅杩愯/鏋勫缓鏈敼鍔ㄧ殑婧愮爜瀹屽叏璺宠繃缂栬瘧涓庨摼鎺ワ紙瀹炴祴
`examples/hello.ax` 绾?1.1s 鈫?85ms锛夈€俙AOXN_NO_CACHE=1` 鍏抽棴锛宍AOXN_CACHE_DIR`
杩佺Щ缂撳瓨鐩綍銆?

## 璇█閫熻

锛堜唬鐮佺ず渚嬪悓涓婃柟鑻辨枃鍖猴細`Particle` 缁撴瀯浣撱€佸€艰涔夈€佸瓧绗︿覆閫愬瓧鑺傛瘮杈冦€傦級

绫诲瀷涓ユ牸銆佹瘡涓〃杈惧紡閮芥鏌ワ細`int`/`float` 姘镐笉闅愬紡浜掕浆锛屾潯浠跺繀椤绘槸 `bool`锛?
鏁扮粍绱㈠紩涓嶆鏌ワ紙C 椋庢牸锛夛紝鍑芥暟鎵€鏈夎矾寰勫繀椤昏繑鍥炪€備弗鏍兼槸鐗规€э細淇濊瘉绠€鍗曞埌鏈哄櫒
鍙互鎺ㄧ悊銆?

### v0.44.0 鍦ㄥ簱闈笂鍔犱簡浠€涔堚€斺€旀爣鍑嗗簱寮€寮?

鍗佷笁涓爣鍑嗗簱妯″潡浠ョ嫭绔嬫枃浠惰惤鍦帮紝**鎸夊悕瀛楀鍏?*锛?

```Aoxn
import * from "stdlib/datetime"
import * from "stdlib/hashlib"

def main() -> int:
    now = datetime_now()                     # 鏈湴澧欎笂鏃堕棿
    print(datetime_iso(now))                 # 2026-10-05 21:14:07
    print(sha256_str("abc"))                 # ba7816bf8f01cfea鈥?
    print(hmac_sha256("key", "msg"))         # RFC 2104
    print(b64_decode_str("aGVsbG8="))        # hello 鈥斺€?decode 鏂瑰悜琛ラ綈浜?
    print(cal_month(now.year, now.month))    # 鏂囨湰鏈堝巻
    return 0
```

鏈壒妯″潡锛歚math`锛圕RT 涓夎/鎸囨暟 + 鏁板€煎簱绛変簡寰堜箙鐨?`fdiv`/`NaN()`/`is_nan`
宸ュ叿绠憋級銆乣time`锛堝崟璋冮挓 + UTC 澧欎笂閽燂細Windows 涓婃槸 QueryPerformanceCounter锛?
Linux 涓婃槸 `clock_gettime`锛夈€乣datetime`锛圚innant 姘戠敤鍘嗘暟瀛︺€佹墜鍐?`strftime`
瀛愰泦锛屾湰鍦版椂闂磋蛋 `GetLocalTime` / `localtime_r`锛夈€乣calendar`銆?
`pathlib`銆乣base64`锛堢紪鐮?*鍜?*
瑙ｇ爜銆佹爣鍑?+ URL-safe 涓ゅ瀛楁瘝琛級銆乣hashlib`锛圫HA-256/SHA-1/MD5锛屼竴娆℃€?+
澧為噺锛岀函 Aoxn 浣嶈繍绠楋級銆乣hmac`銆乣os`锛堟枃浠剁郴缁熸牳蹇冿細Windows 鍏ㄥ瀛楃銆丩inux 鐩磋蛋 libc鈥斺€?
鐜鍙橀噺銆佸伐浣滅洰褰曘€乴istdir锛夈€乣glob`锛圥ython 鐨?dotfile 瑙勫垯銆佹帓搴忚緭鍑猴級銆乣json`
锛圖OM 鑷?`net/` 鎻愬崌 + 鏂囦欢 I/O锛夈€乣bisect`銆乣heapq`銆傛病鏈変竴涓
`stdlib/stdlib.ax`锛岃嚜涓惧浐瀹氱偣涓嶅姩銆傚彧浼稿悜鏌愪竴涓钩鍙?API 鐨勬ā鍧楁崲涓钩鍙板氨閾炬帴
涓嶄笂锛屾墍浠ョ鍒板钩鍙扮殑閭ｅ嚑涓兘涓や釜骞冲彴閮藉０鏄庡啀鐢?`target_os()` 閫変竴杈光€斺€旇繖鏉¤鍒?
杩炲悓瀹冪殑闄烽槺鍐欏湪 [`docs/stdlib.md`](docs/stdlib.md)锛岄偅閲屼篃鏄€愭ā鍧?API銆佷笌
Python 鐨勫樊寮備互鍙婂悇绉嶅潙锛圲CRT 閾炬帴绗﹀彿鍐茬獊銆侀浂鎵╁睍鍝ㄥ叺鍊硷級鐨勫弬鑰冦€?

### v0.43.0 鍦ㄨ瑷€闈笂鍔犱簡浠€涔堚€斺€斿彲浠ョ偣鍚嶇殑妯″潡

```Aoxn
# util.ax 鍜?shapes.ax 閮藉畾涔変簡 area銆傛鍓嶈繖鏄‖閿欒銆?
import util
import shapes as s
from util import area            # 鍙彇涓€涓悕瀛楋紝涓嶅姞闄愬畾锛坕mport 鍙兘鍐欏湪椤跺眰锛?

def main() -> int:
    print(util.area(2))         # 璺ㄦ枃浠堕檺瀹氳闂€斺€斾笉鍐嶉渶瑕佸墠缂€
    print(s.area(3))            # as 鏀圭殑鏄€屾ā鍧椼€嶇殑鍚嶅瓧锛屼笉鏄畠鎴愬憳鐨勫悕瀛?
    print(area(4))              # from 甯﹁繘鏉ョ殑閭ｄ釜鍚嶅瓧
    return 0
```

```Aoxn
import shapes

def scale(c: shapes.Circle) -> shapes.Circle:   # 缁忔ā鍧楀紩鐢ㄧ殑绫诲瀷
    return shapes.Circle(r=c.r * 2)             # 浠ュ強缁忔ā鍧楁瀯閫?

def main() -> int:
    return scale(shapes.Circle(r=2)).r
```

Python 鐨勫啓娉曞叏閮ㄥ彲鐢紝涓斿惈涔変笌 Python 涓€鑷达細`from m import *`銆?
`from m import a, b`銆乣from m import a as b`銆乣import m`銆乣import m as n`銆?
**鍏峰悕瀵煎叆鍙鍏ュ垪鍑虹殑鍚嶅瓧**锛屽叾浣欏悕瀛椾笉鍦ㄤ綔鐢ㄥ煙鍐咃紱妯″潡缁戝畾鏄懡鍚嶇┖闂磋€岄潪
鍊硷紝鎵€浠?`util.f()` 鎴愮珛鑰?`x = util` 鏄紪璇戦敊璇€傛ā鍧楄鏄庣鍙啓鎴愯８鐨勭偣鍙峰舰寮?
锛坄import stdlib.net.json`锛岀偣鍗?`/` 瀛愯矾寰勫垎闅旂锛夋垨鍔犲紩鍙凤紱瑁稿啓娉曡繕浼氬厛鍦?
瀵煎叆鏂囦欢鑷繁鐨勭洰褰曢噷鎵撅紝鎵€浠?`import util` 鑳芥壘鍒版梺杈圭殑 `util.ax`銆備袱涓ā鍧?
閮藉畾涔?`f` 鏃讹紝鍙缁忓悇鑷殑妯″潡璁块棶灏辨病闂鈥斺€旀妸涓や釜閮芥槦鍙峰鍏ュ埌鍚屼竴浣滅敤鍩?
鍒欎細鍐茬獊锛屽苟涓斾細鎸囧悕閬撳鍦版姤鍑烘潵銆?

### v0.42.0 鍦ㄨ瑷€闈笂鍔犱簡浠€涔?

```Aoxn
def main() -> int:
    assert(0x1F == 31)          # 鍗佸叚杩涘埗 / 浜岃繘鍒跺瓧闈㈤噺锛涘啓閿欐椂鎶?
    assert(0b101 == 5)          #   銆宨nvalid digit銆嶈€屼笉鏄€寀nknown variable 'x10'銆?

    i = 0
    while i < argc():           # 鍛戒护琛屽弬鏁帮細涓暟 + 鎸変笅鏍囧彇
        print(arg(i))
        i = i + 1

    if argc() == 0:
        exit(2)                 # 鏄惧紡閫€鍑虹爜锛屼换浣曚綅缃?

    s = "line\r\n"              # \r 涓?\0 鍔犲叆杞箟闆嗭紙鍏卞叚涓級
    print(len(s))               # 6 鈥斺€?CR/LF 鏄湡瀹炲瓧鑺?
    return 0
```

`assert` 澶辫触鍗充腑姝㈠苟甯︽簮琛屽彿锛堜笉鍙崟鑾封€斺€旈偅鏄?`raise` 鐨勪簨锛夈€乣exit(code)`
璁鹃€€鍑虹爜銆乣argc()`/`arg(i)` 璇诲埌 `aoxn run app.ax -- a b` 杞彂鐨勫弬鏁般€?
`main` 鏈韩涓嶅甫鍙傛暟銆傜紪璇戝櫒涔熸湁浜?*璀﹀憡灞?*锛歚W001` 鎶ャ€岃祴鍊煎悗浠庢湭璇诲彇銆嶇殑
灞€閮ㄥ彉閲忎笖涓嶄娇鏋勫缓澶辫触锛沗--json` 鍦ㄦ垚鍔熸椂涔熻繑鍥?
`{"ok":鈥?"errors":[鈥,"warnings":[鈥}`锛沗--clang-arg`/`-g` 閫忎紶缁?clang
锛堣皟璇曚俊鎭€乣-fsanitize=undefined`锛夆€斺€擟 缂栬瘧澶辫触鐜板湪鏄?`cc` 闃舵锛?
涓嶅啀鏄?`internal`銆?

### v0.51.2 在语言面上加了什���——Python 语法糖第二批

字符串和容器终于有了真正的索引、切片和成员判断。下面每个值都是
CPython 3 对同一表达式的输出。

```Aoxn
def main() -> int:
    s = "Aoxn"

    print(s[0], s[-1])                   # A n     一个字节，负数从末尾数
    print(s[1:3], s[:2], s[2:], s[:])    # ox Ao xn Aoxn
    print(s[-2:], s[:-1], s[3:1])        # xn Aox  （空）
    print(s[99])                         # 空——夹住了，绝不读过界

    print("ox" in "Aoxn", "zz" in "Aoxn")        # true false   （子串）
    print(1 in [3, 1, 4], 9 in [3, 1, 4])         # true false   （元素）
    print("a" in {"a": 1}, "b" in {"a": 1})      # true false   （键）

    for byte in "abc":
        print(byte)                      # a b c   —— 字符串是字节序列
    return 0
```

- **`s[i]`** 是**一个字节**的字符串。**不会有 `char` 类型，也不打算有**：
  `len(s)` 从一开始就是 `strlen`，`len("中")` 等于 3，字符类型会和这条既有
  契约直接冲突。
- **`s[a:b]`** 两端都可以留空，负数从末尾数，两端都会**夹住**
  （`s[1:999]` 就是剩下的部分）。**数组不能切片**：Aoxn 数组是 `[T; N]`，
  长度在类型里，切出来没有类型可写；而且它本来就值语义复制，`b = a`
  就是它的切片。
- **`x in c` / `x not in c`** ——**右边**是容器，由它决定问的是什么：子串、
  元素、还是键。左边必须可比较，所以结构体和数组（没有 `==`）是类型错误，
  而不是给一个错答案。
- **`for c in s`** 走的是字节。长度在进循环**之前**只读一次，所以循环体里
  往字符串追加内容不会把迭代拉长。

**两处有意为之的偏差，都有测试钉住：**

- **越界返回 `""`。** 数组索引仍然是无检查的 C 风格——那里字节是调用方的；
  但在编译器自己的字符串 helper 里越界读，就是**我们**去读别人的缓冲区。
- **`not a in b` 按 C 的优先级，不是 Python 的。** Aoxn 的 `not` 就是 C 的
  前缀 `!`，比所有比较运算符结合更紧，所以读作 `(not a) in b`；而 `!` 只
  接受 `bool`，于是不加括号的写法是**类型错误**，而不是一个悄悄的错答案。
  正确写法是 `not (a in b)`。把 `not` 提到 Python 的层级会**静默改变**每一个
  已有的 `not b == c`（今天读作 `(!b) == c`），那是对已发布运算符的语义变更，
  不是新增语法糖。

和第一批一样，用不到的程序**完全不受影响**：生成的 C 逐字节相同，所以自举
不动点不需要 `selfhost/` 镜像。
## 鏍囧噯搴撲笌 UI 宸ュ叿绠?

`stdlib/stdlib.ax` 鐢?Aoxn 鑷繁鍐欐垚锛氭硾鍨?`sort` / `binary_search` / 鑱氬悎銆?
鍙闀?`Vec`銆佸瓧鑺傜紦鍐层€佹枃浠?IO 涓庤繘绋嬭皟鐢ㄣ€傝嚜 v0.44.0 璧锋爣鍑嗗簱鏄竴缁?
**鍏峰悕妯″潡**锛坄stdlib/` 涓嬶級锛歚math`銆乣time`銆乣datetime`銆乣calendar`銆?
`pathlib`銆乣base64`銆乣hashlib`銆乣hmac`銆乣os`銆乣glob`銆乣json`銆乣bisect`銆?
`heapq`鈥斺€斿悇鑷嫭绔嬫垚鏂囦欢銆佹寜鍚嶅瓧瀵煎叆锛屽弬鑰冩枃妗ｅ湪
[`docs/stdlib.md`](docs/stdlib.md)锛岄厤濂?`examples/*_demo.ax` 绀轰緥涓?
`tests/*.rs` 椹卞姩娴嬭瘯銆?

鑷?v0.27.0 璧锋爣鍑嗗簱甯︽湁 **Qt 椋庢牸鐨勭珛鍗虫ā寮?UI 宸ュ叿绠?*锛堢函 Aoxn + 鍘熷
FFI锛夛紝v0.29.3 鍗囧埌 **Qt 绾?*锛氬竷灞€绠＄悊鍣紙vbox/hbox/grid +
鍗冨垎姣旀媺浼革級銆佺湡鏂囨湰杈撳叆锛堝厜鏍囥€乁TF-8 鎰熺煡缂栬緫銆?*鏂囨湰閫夊尯** Shift+鏂瑰悜閿?
鎷栨嫿銆丆trl+A/C/X/V 鍓创鏉匡級銆?*澶氳缂栬緫鍣?*銆侀敭鐩樼劍鐐归摼锛圱ab/Enter/Space锛夈€?
**鑿滃崟鏍?*锛坴0.45.0 璧锋敮鎸侀敭鐩樺鑸笖瀵硅緭鍏ユā鎬侊細鈫?鈫?寰幆绉诲姩銆丒nter 閫変腑銆?
Esc 鍏抽棴锛夈€?*鏍?琛ㄦ牸妯″瀷瑙嗗浘**锛堝爢 `TreeModel`/`TableModel`锛夈€佹棤鍑芥暟鎸囬拡鐨?
**淇″彿妲戒簨浠?*銆?0 浣欐帶浠讹紙鎸夐挳銆佸垏鎹㈤挳銆佸閫夋銆佸崟閫夌粍銆佹粦鏉°€佸井璋冩銆?
鏂囨湰妗嗐€佸垪琛ㄦ銆佹诞灞備笅鎷夋銆佹爣绛鹃〉銆佸垎缁勬銆佹粴鍔ㄥ尯銆佸伐鍏锋彁绀猴級銆佺鐢ㄦ€併€?
娴眰瑕嗙洊涓?16 鑹蹭寒/鏆椾富棰樸€傛帶浠舵槸姣忓抚璋冪敤鐨勬櫘閫氬嚱鏁帮紝鐘舵€佺敱搴旂敤鑷繁
鎸佹湁鈥斺€旇繖姝ｆ槸"鏆傛棤鍥炶皟"鐨勮瑷€鎵€鑳芥壙杞界殑褰㈡€侊紙鐢ㄦ硶绀轰緥瑙佷笂鏂硅嫳鏂囧尯锛夈€?
v0.45.0 鏄鍐呴儴鏈哄埗鐨勪竴娆℃€ц兘涓庢纭€ф暣澶囷紱v0.46.0 鏂板妯悜婊氬姩鍖哄煙
锛坄ui_scroll_begin_h` / `ui_scroll_end_h`锛屽簳閮ㄦ粴鍔ㄦ潯锛屼笌绔栧悜鍙祵濂楋級
涓庣瓑瀹藉瓧浣擄紙`ui_font_mono(c)` / `ui_font_sans(c)`鈥斺€擶indows 涓婃槸
Consolas锛孡inux 涓婃槸 fontconfig 鐨?`Monospace` 鍒悕锛夛紱**v0.47.0 淇ソ浜?
GDI 瑁佸壀鏍堚€斺€斿畠浠庢潵娌¤兘鎭㈠杩?*锛氫竴甯ч噷鍙鍑虹幇涓€涓?`ui_textbox`锛屾暣涓?
绐楀彛锛堝惈鏈€鍚庣殑 `BitBlt`锛変粠閭ｄ竴鍒昏捣灏辫瑁佸埌閭ｄ釜 textbox 鐨勭煩褰㈤噷锛岀洿鍒?
杩涚▼缁撴潫锛涘悓鏃跺姞鍏ユ寜瑁佸壀鍖哄墧闄ょ粯鍒躲€?4 椤规枃鏈祴閲忕紦瀛橈紝浠ュ強 Windows
鍚庣鐨?DC 鐘舵€佺紦瀛樸€?*v0.49.0** 鏀舵帀浜嗗叾浣欑殑闈欓粯閿欒锛氱 9 灞傚竷灞€鐜板湪浼?
寮规帀绗?8 灞傦紙姝ゅ悗鏁村抚鎺т欢钀借繘閿欒鐨勭洅瀛愰噷锛屼笖鏃犱换浣曟彁绀猴級銆乣itoa10` 鐨?
arena 鑰楀敖閫冮€歌矾寰勮繑鍥炵殑鏄?UTF-16 鑰岀粯鍒惰矾寰勮鐨勬槸 ASCII锛堟墍浠?arena 涓€婊?
`ui_label_int` 灏卞彧鐢诲嚭涓€浣嶆暟瀛楋級銆乣utf16_write` 鍥?`while i < len(s)`
閫愬瓧绗﹂噸鎵暣涓茶€屾槸 O(n虏)锛?048 瀛楃涓€娆?626 碌s 鈫?3 碌s锛夛紝浠ュ強 X11 瑁佸壀鏍?
鍙兘寮瑰嚭涓€涓粠鏈?push 杩囩殑鐭╁舰銆傚悓鏃舵妸鍗佷簲浠?press-claim/click-release
鍛ㄦ湡鍚堝苟鎴?`ui_press` / `ui_press_claim` / `ui_press_release` / `ui_pressed`锛?
骞舵妸鏂囨湰妗嗘瘡甯ф祴閲忔暣娈靛瓧娈靛彧涓鸿涓€涓父閲忚楂橈紙鍦?v0.47.0 娴嬮噺缂撳瓨涔嬩笅浠嶆槸
姣忓抚 O(n) 鍐呭鍝堝笇锛夋崲鎴愬甫缂撳瓨鐨?`ui_font_height`銆?

**宸ュ叿绠辩敱涓変釜鍙Щ妞嶆枃浠跺姞姣忓钩鍙颁竴涓悗绔粍鎴?*锛氬彲绉绘鍐呮牳锛坄ui.ax`锛夈€佷笌
骞冲彴鏃犲叧鐨勬帶浠跺眰锛坄ui_draw.ax`锛夛紝浠ュ強 `ui_win.ax`锛圵in32/GDI锛夋垨
`ui_x11.ax`锛圶lib + Xft锛寁0.46.0锛変簩閫変竴銆備竴琛?import 鍗冲彲寮曞叆锛?

```Aoxn
import * from "stdlib/ui_win"     # Windows: Win32 + GDI锛?l user32 -l gdi32锛?
import * from "stdlib/ui_x11"     # Linux: Xlib + Xft锛?l X11 -l Xft锛?
```

鎺т欢灞備粠涓嶅嚭鐜颁换浣曞钩鍙扮鍙凤細瀹冨彧璋冪敤鍚庣鎻愪緵鐨?`plat_*` 鍘熻锛屽绾︽祴璇曟妸
**涓や釜鍚庣**閽夊湪鍚屼竴濂楀師璇泦涓婏紝鍥犳杩欎竴灞傚彲浠ユ棤绐楀彛鍦版祴璇曪紝鎺т欢闆嗗悎鍦?
涓や釜骞冲彴涓婂畬鍏ㄤ竴鑷淬€俋11 鍚庣鍦?v0.30.0 琚Щ闄ゃ€乿0.46.0 鎸?v3 濂戠害閲嶅啓鈥斺€?
Xft 鏂囨湰銆佺灞?Pixmap 鍙岀紦鍐层€乆11 selection 鍗忚鍓创鏉匡紝浠ュ強姣忓抚涓€娆＄殑
`XQueryKeymap` 鎵弿锛堟寜浣忕殑閿笉鍐嶅湪鑷姩閲嶅鐨?release+press 瀵逛箣闂撮棯鐑侊級銆?

```powershell
cargo run -- run examples\ui_gallery.ax -l user32 -l gdi32   # 鍏ㄦ帶浠剁敾寤?
cargo run -- run examples\ui_demo.ax -l user32 -l gdi32      # 鍏ラ棬绀轰緥
```

```bash
aoxn run examples/ui_probe_x11.ax -l X11 -l Xft   # Linux 鍐掔儫鎺㈤拡锛圕I 閲岃繕浼氬湪 Xvfb 涓嬭窇锛?
```


缁嗚妭瑙?[`docs/ui.md`](docs/ui.md)锛涚敾寤?`examples/ui_gallery.ax`锛涙祴璇?
`tests/ui.rs`銆?

### API SDK锛坴0.41.0锛?

鏍囧噯搴撻噷鏈変袱濂楀巶鍟?SDK锛屽叡鐢?*鍚屼竴灞備紶杈?*銆俙stdlib/net/` 涓庡巶鍟嗘棤鍏斥€斺€?
base64/鐧惧垎鍙风紪鐮?UTF-16 缂栬В鐮併€佸甫鏋勫缓鍣ㄧ殑 JSON DOM銆乄inHTTP 涓婄殑 HTTPS銆?
SSE鈥斺€斿悇 SDK 鍦ㄥ叾涓婅拷鍔犺嚜宸辩殑鏂囦欢銆備袱鑰呯殑榛樿鍊笺€佺幆澧冨彉閲忓悕涓庨噸璇曠瓥鐣ラ兘鍙栬嚜
鍙傝€?Python 瀹㈡埛绔€?

`stdlib/openai/`锛坴0.40.1锛夎鐩?chat completions銆乺esponses銆乪mbeddings銆?
models 涓?moderations锛岄樆濉炰笌娴佸紡鐨嗗彲锛?

```Aoxn
import * from "stdlib/openai/client"

def main() -> int:
    c = oa_client_env()                 # OPENAI_API_KEY銆丱PENAI_BASE_URL 鈥?
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

`stdlib/anthropic/`锛坴0.41.0锛夎鐩?Messages API鈥斺€旈偅閲岀殑杞借嵎鏄?*甯︾被鍨嬬殑鍐呭鍧?
鏁扮粍**锛屼笉鏄瓧绗︿覆鍒楄〃鈥斺€斿鍔?token 璁℃暟銆乵odels銆乫iles 涓庢秷鎭壒澶勭悊锛屽苟涓?
**宸ュ叿璋冪敤**鏄畬鏁撮棴鐜殑锛?

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

涓変欢浜嬫槸涓ゅ SDK 鍏辨湁鐨勩€?*娴佸紡**閮芥槸鎷夊彇寰幆锛坄*_stream` /
`*_stream_next` / `*_stream_close`锛夎窇鍦?SSE 杩囨护鍣ㄤ笂锛汚nthropic 鐨勫抚鏄?
**甯﹀悕瀛?*鐨勶紝鑰屽伐鍏疯皟鐢ㄧ殑鍙傛暟鍙互 JSON 鏂囨湰鐗囨鐨勫舰寮忔姷杈锯€斺€斾簬鏄?`AnAcc`
鎶婃暣鏉℃祦閲嶆柊缁勮鎴愪竴鏉″畬鏁寸殑 Message锛屽悓涓€缁勮闂櫒鐓ф牱璇诲畠
锛坄an_acc_resp(st.acc)`锛夈€?*宸ュ叿**鐢?schema 鏋勫缓鍣ㄨ€屼笉鏄墜鍐欒浆涔?JSON锛?
骞朵笖鏄畬鏁撮棴鐜細澹版槑宸ュ叿銆佹敹鍒?`tool_use` 鍧椼€佹妸 assistant 杞鍘熸牱鍥炰紶銆?
鎶?`tool_result` 鏀捐繘 user 瑙掕壊鐨勬秷鎭€佸啀闂竴娆°€?*姘镐笉 raise**锛?
`an_resp_ok` / `oa_resp_ok` 鎶婂叧锛宍an_err_msg` / `oa_err_msg` 鎶婃渶鍏堝け璐ョ殑
閭ｄ竴灞傛覆鏌撴垚涓€涓瓧绗︿覆锛宍an_status_class(status)` 鍒欐妸鍙傝€冨疄鐜扮殑寮傚父浣撶郴
鍙樻垚璋冪敤鑰呭彲 switch 鐨勬暟鎹€?

閾炬帴鏃跺姞 `-l winhttp`锛堝敮涓€鑷甫 TLS 鐨勪紶杈擄級锛汬TTP 鍏ュ彛鍦?Linux 涓婁細杩斿洖
骞插噣鐨勩€屾湰骞冲彴鏆傛棤浼犺緭灞傘€嶉敊璇€屼笉鏄摼鎺ュけ璐ャ€俙an_client_at` /
`oa_client_at` 鍙妸浠讳竴 SDK 鎸囧悜缃戝叧銆佷唬鐞嗘垨鐜洖 mock銆?

```powershell
cargo run -- run examples\openai_chat.ax -l winhttp      # 鍦ㄧ嚎绀轰緥锛堥渶 key锛?
cargo run -- run examples\anthropic_chat.ax -l winhttp   # 娑堟伅銆佹祦寮忋€佸伐鍏烽棴鐜€乼oken 璁℃暟
cargo test --test openai_sdk                             # 绂荤嚎锛氱函灞?+ 鐢?Aoxn 鍐欑殑 mock 鏈嶅姟鍣?
cargo test --test anthropic_sdk
```

鍙傝€冿細[`docs/openai-sdk.md`](docs/openai-sdk.md) 路
[`docs/anthropic-sdk.md`](docs/anthropic-sdk.md) 路 绀轰緥
`examples/openai_chat.ax`銆乣examples/anthropic_chat.ax` 路 娴嬭瘯
`tests/openai_sdk.rs`銆乣tests/anthropic_sdk.rs`銆?

## Aoxn IDE

Aoxn 鑷甫瀹樻柟缂栬緫鍣ㄢ€斺€擿ide/`锛屼竴涓?Tauri 2 澶栧３ + Next.js + Monaco 宸ヤ綔鍙?+
涓€灞傚緢钖勭殑 Rust 鍛戒护灞傜殑鍘熺敓妗岄潰搴旂敤锛?

- **璧勬簮绠＄悊鍣?*锛氶」鐩爲锛堣嚜鍔ㄨ烦杩囨瀯寤?渚濊禆鐩綍锛夛紝甯︾湡姝ｇ殑**鏂板缓鏂囦欢 /
  鏂板缓鏂囦欢澶?*鈥斺€斿悕瀛楀湪瀵硅瘽妗嗛噷灏卞湴鏍￠獙锛屾敮鎸佷竴姝ュ缓鍑哄祵濂楄矾寰?
  锛坄src/util.ax`锛夛紝浠讳綍璇曞浘閫冨嚭鎵撳紑鐩綍鎴栬鐩栧凡鏈夋潯鐩殑鍚嶅瓧閮戒細琚嫆缁濄€?
- **Monaco + Aoxn 璇硶**锛坄#` 娉ㄩ噴銆乣f""` 鎻掑€笺€佸叧閿瓧/绫诲瀷/鍐呭缓鐫€鑹诧級锛?
  姣忎釜鏍囩椤典竴涓?model锛屾挙閿€鏍堟寜鏂囦欢闅旂銆?
- **璇婃柇鍗崇紪杈戝櫒鏍囪**锛氬墠绔В鏋愮紪璇戝櫒杈撳嚭锛岀偣鍑昏緭鍑洪潰鏉块噷鐨勮瘖鏂鐩存帴璺冲埌
  鍑洪敊琛岋紱**淇濆瓨 `.ax` 鏂囦欢鑷姩妫€鏌?*锛岀孩娉㈡氮绾胯窡鐫€缂栬緫璧般€?
- **Check / Build / Run**锛團7 / F6 / F5锛夐┍鍔ㄧ湡瀹炵殑 `aoxn` 浜岃繘鍒讹紝鍙傛暟涓?
  鐢ㄦ埛鍦ㄧ粓绔暡鐨勫畬鍏ㄤ竴鑷粹€斺€擨DE 涓嶉噸鏂板疄鐜颁换浣曟瀯寤洪€昏緫锛屽洜姝ゅ湪 IDE 閲屾瀯寤?
  鍜屽湪缁堢閲屾瀯寤轰笉鍙兘鍑虹幇涓ゅ琛屼负銆侰heck 璧?`aoxn check`锛堝彧鍑鸿瘖鏂紝涓嶅埛 C锛夈€?
- **鍖呯鐞?*锛坴0.33.0锛夆€斺€斾晶鏍忛噷鐨?Packages 瑙嗗浘锛氬睍绀哄伐浣滃尯 `aoxn.json` 鐨?
  渚濊禆涓?`aox_modules/` 閲屽凡瀹夎鐨勫寘锛岀櫧鍚嶅崟鍔ㄨ瘝鍋氭垚鎸夐挳鈥斺€擨nit銆丄dd銆?
  Install銆乁pdate銆丱utdated銆乀ree銆丄udit銆乄hy銆丷emove銆傛瘡鏉″懡浠ら兘鍦ㄦ墦寮€鐨?
  鐩綍閲岃繍琛岀湡瀹炵殑 `aoxn pkg` 骞跺師鏍疯緭鍑猴紱瀵瑰鎴栧叏灞€鎬х殑鍛戒护锛坧ublish銆?
  yank銆乧ache銆乼rust bootstrap銆乶pm-import锛夊湪 Rust 闂搁棬澶勭洿鎺ユ嫆缁濓紝鑰屼笉鏄?
  鍙湪鐣岄潰涓婇殣钘忋€俙aoxn doctor` 涔熷湪杩欓噷锛氱姸鎬佹爮鐨?缂栬瘧鍣?/ clang 鏈壘鍒?
  鎸夐挳浼氭妸瀹冭窇杩涜緭鍑洪潰鏉裤€?
- **蹇€熸墦寮€**锛欳trl+P 鏂囦欢銆丆trl+Shift+P 鍛戒护銆?

宸ヤ綔鍖烘槸杈圭晫涓嶆槸寤鸿锛歸ebview 浼犳潵鐨勬瘡涓矾寰勯兘浼氬厛瑙勮寖鍖栵紝瑙ｆ瀽鍚庤惤鍦ㄦ墦寮€
鐩綍涔嬪鐨勪竴寰嬫嫆缁濓紙鎸夎矾寰勫垎閲忔瘮杈冣€斺€擿C:\proj-evil` 楠椾笉杩?`C:\proj` 鐨?
妫€鏌ワ級锛涙病鏈?璇讳换鎰忚矾寰?鐨勫懡浠わ紝涔熸病鏈?shell 鎻掍欢銆?

```powershell
cd ide
pnpm install
pnpm ide:dev        # 鍘熺敓搴旂敤锛岀儹鏇存柊
pnpm dev            # 娴忚鍣ㄩ瑙堬紙鍐呭瓨 fixture锛夆€斺€旀敼甯冨眬涓嶇敤绛夊師鐢熼噸缂栬瘧
pnpm test           # 鍓嶇娴嬭瘯锛坣ode:test锛?
pnpm test:rust      # Rust 鍛戒护灞傛祴璇?
```

璇﹁ [`docs/ide.md`](docs/ide.md)銆?

## 鎬ц兘

鍚岀畻娉曚笌 `clang -O3` 鍚屾満瀵规瘮锛堥鐑悗 3 娆″彇鏈€浼橈級锛?

| 鍩哄噯 | 瑙勬ā | Aoxn | clang C++ |
|---|---|---|---|
| 寰幆姹傚拰 | 2脳10鈦?娆¤凯浠?| ~15 ms | ~23 ms |
| 鏁扮粍濉厖 + 鎵弿 | 2脳10鈦?娆¤ | 86 ms | 99 ms |
| 缁撴瀯浣撳鍒讹紙鎸夊€硷級 | 7.5脳10鈦?娆?| 237 ms | 206 ms |

鍘熺敓浠ｇ爜灏辨槸鍘熺敓浠ｇ爜鈥斺€擜oxn 涓?clang 鍦ㄥ櫔澹拌寖鍥村唴鎸佸钩銆?

Web 鏈嶅姟鍚屾牱鑳芥墦锛歔`web/`](web/README.md) 濂椾欢鐢?Aoxn 鍐欎簡 HTTP/1.1 鏈嶅姟鍣紝
鍦ㄧ浉鍚岃矾鐢变笂瀵归樀 pnpm + Node.js + Next.js鈥斺€斿悶鍚愭墦骞崇函 Node.js銆乸50 寤惰繜绾︿负
鍏?1/50锛屾瘮 Next.js 澶氭湇鍔?26鈥?4 鍊嶈姹傦紝鍗曚釜 173 KB 浜岃繘鍒躲€? MB 鍐呭瓨銆?
鏁版嵁瑙?[docs/web-benchmark.md](docs/web-benchmark.md)銆?
鑷?2026-10-02 璧疯鏈嶅姟鍣ㄨ繕鏀寔闈欐€佹枃浠舵湇鍔★紙W2锛夛細鍚姩鏃堕杞界殑鍐呭瓨鏂囦欢琛紝
甯?ETag / `If-None-Match` 鈫?304 / 鍗曞尯闂?`Range` 鈫?206 / 涓嶅彲婊¤冻 鈫?416 /
`Cache-Control`锛岀敱 parity 濂椾欢瀵?Aoxn/Node 涓ゅ彴鏈嶅姟鍣ㄥ仛璇箟瀵规瘮銆?

## 鑷妇

缂栬瘧鍣ㄥ凡鐢?Aoxn 閲嶅啓骞惰揪鍒?*鍥哄畾鐐?*锛欰oxn 鍐欑殑 driver 缂栬瘧鏁翠釜鑷妇缂栬瘧鍣?
锛堢害 7k 琛岋細璇嶆硶銆佽娉曘€佸甫鍗曟€佸寲鐨勭被鍨嬫鏌ャ€佽杞藉櫒銆丆 鏂囨湰浠ｇ爜鐢熸垚銆侀┍鍔級
寰楀埌 stage-2锛屼骇鐗╋紙鐢熸垚鐨?C 涓庣洰鏍囨枃浠讹級鍜?Rust 渚ф瀯寤虹殑缂栬瘧鍣?*閫愬瓧鑺備竴鑷?*锛?
骞惰兘缂栬瘧鐪?stdlib 涓庡叏閮?`examples/`銆傝瑙?[`docs/selfhost.md`](docs/selfhost.md)
涓?[wiki 鐨勮嚜涓鹃〉](wiki/Self-Hosting.md)銆?

## 椤圭洰甯冨眬

锛堣〃鏍煎悓涓婃柟鑻辨枃鍖猴細`src/` 缂栬瘧鍣ㄣ€乣src/codegen_c.rs` 鍞竴鐨?C 鍙戝皠鍚庣
锛坴0.29.0 璧凤級銆乣src/paths.rs` 瀹夎甯冨眬鍙戠幇銆乣src/setup/` 鍗曟枃浠跺畨瑁呭櫒銆?
`src/ts/` TS 鍓嶇锛圱S-M1 W1 瀹屾垚锛夈€乣stdlib/` 鏍囧噯搴撴牳蹇?+ v0.44.0 妯″潡鎵?
锛坄docs/stdlib.md`锛? UI v3 +
**OpenAI SDK**锛坄stdlib/openai/`锛寁0.40.1锛夈€乣ide/` Aoxn IDE
锛坄ide/src-tauri` 鏄嫭绔?Cargo 宸ヤ綔鍖猴級銆乣dist/` 鎵撳寘
鑴氭湰銆乣selfhost/` 鑷妇銆乣web/` Web 鍩哄噯銆乣tests/` 绔埌绔祴璇曪紙鍚?C 鏂囨湰
鍥哄畾鐐逛笌瀹夎甯冨眬锛夈€乣docs/stdlib.md` 鏍囧噯搴撴ā鍧楀弬鑰冦€乣docs/ide.md` IDE 鍙傝€冦€?
`docs/spec.md` 璇█瑙勮寖銆?
`docs/openai-sdk.md`銆乣docs/anthropic-sdk.md` SDK 鍙傝€冦€?
`wiki/` 鍙岃 wiki鈥斺€?*宸插喕缁?*銆俙docs/` 鎵嶆槸娲绘枃妗ｃ€傦級

## 娴嬭瘯涓?CI

`cargo test` 璺戠鍒扮娴嬭瘯濂椾欢鈥斺€旂紪璇戝櫒宸ヤ綔鍖?**999 涓?*锛坧ipeline 154銆佺紪璇戝櫒
鍗曞厓娴嬭瘯 18銆佸畨瑁呭櫒 stub 3銆乀ypeScript 鍓嶇 34銆乁I 14銆佸畨瑁呭竷灞€ 6銆丆SS 璧勪骇
21銆丆SS 璧勪骇 v0.36 18銆佺鍙峰鍑?8銆丱penAI SDK 2銆丄nthropic SDK 2锛屼互鍙?
v0.44.0 鏍囧噯搴撴ā鍧楃粍鍚勪竴涓┍鍔ㄦ祴璇曪細hashlib銆乵ath銆乸athlib銆?
containers銆乷s+glob銆乯son 鏂囦欢 I/O鈥斺€斿鍔犺嚜 v0.50.1 璧峰甫涓や釜鐨?`datetime`锛堥┍鍔ㄦ祴璇?
+ 鍙屽钩鍙版椂閽熺鍙烽拤瀛愶級銆乣stdlib_defect_pins`锛坴0.48.0 涓?JSON 鏋勫缓鍣ㄥ爢鎹熷潖涓庝笁澶勪簩娆℃柟寰幆鍔犵殑鍥炲綊濂椾欢锛変笌 `stdlib_perf_pins`锛坴0.50.0 涓?len() 鍗?strlen 涓庡惊鐜嫾鎺ヤ袱鏃忔敼鍐欏姞鐨勯拤瀛愶級锛屽彟鏈?`aoxn-pkg` crate 鐨?94 涓粡 `bash run_pkg_tests.sh` 杩愯鈥斺€?*鍚堣 384 涓?*鈥斺€旀瘡涓?pipeline 娴嬭瘯閮芥槸
.ax 鈫?鍙墽琛屾枃浠?鈫?杩愯 鈫?鏂█ stdout 涓庨€€鍑虹爜銆傚叾涓惈鑷妇鍥哄畾鐐癸細
stage-1 涓?stage-2 缂栬瘧鍣ㄥ鍚屼竴绋嬪簭蹇呴』浜у嚭閫愬瓧鑺備竴鑷寸殑 C 鏂囨湰涓庣洰鏍囨枃浠?
锛堢洰鏍囨枃浠舵瘮杈冧細灞忚斀 clang 鍐欏叆姣忎釜 Windows 鐩爣鏂囦欢鐨?COFF 鏃堕棿鎴筹級銆?
涓ゅ SDK 娴嬭瘯閮戒細鎶?*鐪熷疄鐨?WinHTTP 璺緞**鎵撳埌涓€鍙扮敤 Aoxn 鍐欐垚銆佽窇鍦ㄧ幆鍥?
绔彛涓婄殑 mock API 鏈嶅姟鍣ㄤ笂锛屽洜姝ゆ秷鎭垱寤恒€丼SE 娴佸紡銆佸伐鍏疯皟鐢ㄩ噸缁勩€佸垎椤点€?
`Retry-After` 涓庡悇绫婚敊璇搷搴旈兘鏃犻渶鑱旂綉銆佹棤闇€鍑嵁鍗宠瑕嗙洊銆侷DE 鑷甫涓ゅ
鐙珛娴嬭瘯锛氬懡浠ゅ眰鐨?36 涓?Rust 娴嬭瘯锛坄ide/` 涓?`pnpm test:rust`锛変笌鍓嶇鐨?
62 涓?node:test 鐢ㄤ緥锛坄pnpm test`锛夛紱鐢变簬 `ide/src-tauri` 鏈夋剰鎺掗櫎鍦ㄦ牴
宸ヤ綔鍖轰箣澶栵紝瀹冧滑涓嶄細鍦?`cargo test` 閲岃繍琛屻€?

CI 姣忔 push 璺?*涓や釜浠诲姟**銆俙windows-latest` 璺戝叏濂椾欢锛岀劧鍚庢墦鍑哄崟鏂囦欢
瀹夎鍣ㄣ€佸畨瑁呭埌涓存椂鐩綍骞惰窇 `aoxn doctor` 涓庝竴涓爣鍑嗗簱绋嬪簭鈥斺€斿畨瑁呭潖浜嗕細鍦?
鏋勫缓鏃跺け璐ワ紝鑰屼笉鏄瓑鍒板彂鐗堛€俙ubuntu-latest`锛坴0.46.0锛夊湪 Linux 涓婅窇鍚屼竴濂?
浠垛€斺€旇瘉鏄庣紪璇戝櫒銆佸弻骞冲彴鏍囧噯搴撲笌鑷妇鍥哄畾鐐瑰湪绗簩鍙板涓讳笂鍚屾牱鎴愮珛鈥斺€旈殢鍚庣敤
`-l X11 -l Xft` 鏋勫缓 `examples/ui_probe_x11.ax` 骞跺湪 Xvfb 涓嬮┍鍔?30 涓湡瀹?
甯э紝浣滀负 X11 UI 鍚庣鐨勯摼鎺ヤ笌杩愯妫€鏌ャ€傛墦 tag 鍙戝竷
`Aoxn-<version>-Setup.exe`锛堣
[`.github/workflows/release.yml`](.github/workflows/release.yml)锛夈€?

## 鍖呯鐞?

`aoxn pkg`锛堝強鐩存帴鍒悕 `aoxn init | add | remove | install | update |
outdated | list | freeze | tree | why | publish | yank | audit | trust |
cache | npm-import`锛夐€氳繃 `aoxn.json` + `aoxn.lock` 鎶婁緷璧栬杩?`aox_modules/`锛?
鐢?PubGrub 瀵圭洰褰曘€乬it 鎴栧彧璇?HTTP registry 鍋氳В鏋愶紙beta锛涜
[`crates/aoxn-pkg`](crates/aoxn-pkg) 涓?
[`docs/pkg-manager.md`](docs/pkg-manager.md)锛夈€傝嚜 v0.29.1 璧凤紝瑁稿寘
瀵煎叆鎸夊寘鍐?`aoxn.json` 鐨?`main` / `exports`锛堝惈 `pkg/sub` 瀛愯矾寰勶級/ `types`
瑙ｆ瀽鍏ュ彛锛岃杩涙潵鐨勫寘鍗充究鍏ュ彛涓嶅彨 `index.ax` 涔熻兘 import锛?

```Aoxn
import * from "http"          # 鈫?aox_modules/http/aoxn.json 鐨?main/exports["."]
import * from "http/client"   # 鈫?exports["./client"]
```

娌℃湁 manifest 鐨勫寘鍥為€€鍒版棫鐨?`aox_modules/<name>` 鐩綍鎺㈤拡锛坄<name>.ax` /
`index.ax`锛夈€?

**v0.32.0** 琛ラ綈浜嗗鐓?pip / pnpm 鏃舵渶缂虹殑鏃ュ父鑳藉姏锛?

```bash
aoxn add -D harness        # devDependencies鈥斺€斾笉璇ヨ繘浜х墿鐨勫伐鍏蜂緷璧?
aoxn install --prod        # 鍙杩愯鏃讹紝dev-only 鍖呬細琚鎺?
aoxn install --jobs 16     # 骞惰涓嬭浇 tarball锛堥粯璁?8锛?
aoxn install --json        # 鎶婂畨瑁呮姤鍛婂彉鎴愭満鍣ㄥ彲璇荤殑鏁版嵁
aoxn list                  # 瑁呬簡浠€涔堬細鍚嶅瓧/鐗堟湰/scope/鏉ユ簮
aoxn freeze                # `name==version`锛岀粰 CI 鍩虹嚎鐢?
aoxn audit --fix           # 鍗囧埌鍏憡缁欏嚭鐨勫凡淇鐗堟湰
```

dev 涓?prod 瑙ｆ瀽杩?*鍚屼竴浠?*閿佹枃浠讹紝闈犳瘡涓寘鐨?`dev` 鏍囪鍖哄垎鈥斺€旇繖鏍峰彧鍙戣繍琛屾椂
浜х墿鐨?CI 浠诲姟锛屼粛鐒惰兘浠庝竴浠戒篃閽変綇浜嗘祴璇曞伐鍏烽摼鐨勯攣鏂囦欢澶嶇幇銆?
`"overrides": {"http": "1.4.2"}` 鍒欐槸鍦ㄤ緷璧栧浘浠讳綍浣嶇疆寮哄埗璇ュ寘鐨勭増鏈姹傘€?

registry 杩樺彲浠ユ槸**绛栧睍**鐨勨€斺€斾竴涓粨搴撳悓鏃惰鐫€鍖呫€乣trust.json` 璇勫璁板綍鍜?
`advisories/`锛?

```bash
aoxn trust bootstrap https://github.com/AlonechatWorkspace/Aoxn-trusted-third-party-package
aoxn trust check http --tier audited   # CI 闂ㄧ
```

瀹夎鏃朵細瀵圭瓥灞?registry 涓病鏈夎瘎瀹¤褰曠殑鍖呯粰鍑哄憡璀︼紱瀵逛笉鍋氫俊浠诲０鏄庣殑 registry
鍒欎繚鎸佹矇榛樸€備俊浠绘竻鍗曟槸绛栧睍淇″彿锛屼笉鏄鍚嶁€斺€旂湡姝ｇ殑瀵嗙爜瀛﹂敋鐐逛粛鏄?`aoxn.lock` 閲?
閽変綇鐨?manifest 鍝堝笇銆傜洰褰曠粨鏋勪笌 schema 瑙?
[`docs/trusted-registry.md`](docs/trusted-registry.md)銆?

npm 妗ユ帴锛坄aoxn npm-import`锛屼互 npm CLI 涓轰紶杈撳眰锛夋妸鍙戝竷鍒颁换鎰?npm 鍏煎 registry
鐨?Aoxn 鍖呭鍏?`vendor/<name>/` 骞剁櫥璁颁负璺緞渚濊禆锛涚函 JavaScript 鍖呬細琚槑纭嫆缁濄€?

## CSS 璧勪骇

`import` 杩涙潵鐨?`.css` 鏄?*鏋勫缓璧勪骇锛屼笉鏄簮鐮?*锛氬畠浼氳鎵撳寘锛坄@import` 灏卞湴
鍐呰仈锛夈€佸帇缂┿€佹寚绾瑰寲鍚庡祵鍏ヤ簩杩涘埗鈥斺€斾笉闇€瑕佽緭鍑虹洰褰曪紝涔熶笉闇€瑕佺害瀹氭枃浠跺竷灞€锛?

```Aoxn
import * from "./style.css"

def main() -> int:
    print(styles())               # 鏁翠唤浜х墿
    print(styles_fingerprint())   # "18603d4d4686e2cc.css"
    return 0
```

鍘嬬缉鍙幓娉ㄩ噴涓庡啑浣欑┖鐧斤紝鍒绘剰涓嶅仛鍒殑鈥斺€斾笉鍚堝苟閫夋嫨鍣ㄣ€佷笉閲嶆帓鈥斺€斿洜姝や骇鐗?CSS
鏄簮鏂囦欢鐨勭函鍑芥暟銆?

**CSS Modules銆?* `*.module.css` 鐨勭被鍚嶄細琚綔鐢ㄥ煙鍖栵紙浠ユ枃浠惰嚜韬矾寰勪负绉嶅瓙锛夛紝
骞剁敓鎴愪竴涓彇鐢ㄥ嚱鏁帮紱瀹冪殑瑙勫垯涓嶈繘鍏ㄥ眬鍖咃紝杩欐鏄綔鐢ㄥ煙鍖栨湁鎰忎箟鐨勫墠鎻愶細

```Aoxn
import * from "./page.module.css"
    print(page_class("title"))     # -> "title_87b780ec"
```

鏀瑰啓鍣ㄨ瘑鍒笂涓嬫枃锛氬瓧绗︿覆閲屻€乣@media` 鍓嶅閲屻€佸０鏄庡€奸噷鐨?`.` 閮戒笉鏄€夋嫨鍣紝
鍘熸牱淇濈暀銆?

**Tailwind** 浠ラ鐢熸垚浜х墿鎺ュ叆锛岃€岄潪浣滀负渚濊禆鈥斺€斿畠鏄函 JavaScript 鐨?npm 鍖咃紝
`aoxn npm-import` 鎸夎璁℃嫆缁濊繖绫诲寘锛涜€岃皟鐢ㄥ畠鐨?CLI 浼氭妸 Node 鍙樻垚鏋勫缓鍓嶇疆銆?
鍙互鑷杩愯 `npx tailwindcss -o generated.css` 鍚?import 缁撴灉锛屼篃鍙互璁╃紪璇戝櫒
鐢熸垚涓€浠芥湁鏂囨。鐨勫伐鍏风被瀛愰泦锛?

```sh
aoxn build app.ts --tailwind
```

瀹冨彧鎵弿 `class`/`className` 灞炴€э紙缁濅笉浠庢暎鏂囦腑鎸栨帢绫诲悕锛夛紝骞朵笖瀵逛笉瑕嗙洊鐨?
宸ュ叿绫?*鎸囧悕鎶ュ憡**锛岃€屼笉鏄潤榛樹涪寮冦€?

**浠庣鐩樻彁渚涜祫浜с€?* `--emit-assets <dir>` 鎶婃寚绾瑰寲鐨勪骇鐗╁寘銆佹瘡寮犳牱寮忚〃鍜屽叏閮?
`url()` 鐩爣鍐欏埌鍙墽琛屾枃浠舵梺锛屽苟鎶?`url()` 鏀瑰啓鎴愪骇鐗╁悕銆傜▼搴忔棤闇€鐭ラ亾浠讳綍
鏋勫缓鏈熻矾寰勫嵆鍙壘鍒板畠浠細

```Aoxn
print(asset_path(styles_fingerprint()))   # 鈥assets\82b4fb25鈥?css
```

瀹屾暣璇存槑涓庨檺鍒惰 [`docs/css-assets.md`](docs/css-assets.md)銆?

## 现状

**v0.51.2** · **Python 语法糖第二批：字符串和容器有了真正的索引、切片和成员判断** ·
427 测试全绿（编译器工作区 327 + `aoxn-pkg` 100；IDE 另有 36 个 Rust + 62 个前端测试）·

`s[i]` 是**一个字节**的字符串，`s[a:b]` 支持留空和负数边界（两端都会夹住），
`x in c` / `x not in c` 按**右边**的容器类型分别回答**子串**、**元素**或**键**，
`for c in s` 走字节。每个运行时的值都与 CPython 3 一致。

**两处有意为之的偏差，都有测试钉住。** 越界索引返回 `""` 而不是读过终止符——
字节归调用方时无检查的 C 风格是对的，在编译器自己的 helper 里则是**我们**去读
别人的缓冲区。`not a in b` 保持 C 的优先级，所以不加括号的写法是**类型错误**
而不是悄悄的错答案：把 `not` 提到 Python 的层级会**静默改变**每一个已有的
`not b == c`，那是对已发布运算符的语义变更，不是新增语法。

**用不到的���式完全不影响不用它的程序。** 生成的 C 逐字节相同（已在四个现有示例上
与 `main` 对比验证），所以自举不动点不需要 `selfhost/` 镜像，和 v0.51.0 的
语法糖同等待遇。`in` 的辅助函数按**排序**而非哈希顺序建表，并且有一个测试让编译器
在五个独立进程里对同一份源码跑五遍——因为在同一个进程里分配器恰好会掩盖这类 bug。

**v0.51.1** · 审计提出的 `crates/aoxn-pkg` 四条与 IDE 三条全部关闭，见
[SECURITY.md](SECURITY.md) ·

**v0.51.0** · Python 语法糖第一批（默认参数值、条件表达式、`**`、多参数 `print`）
外加**标准库六条安全修复** ·

**v0.50.10** · **`aoxn c` 不可复现：数组复制辅助函数拿堆地址当名字** ·

**璺戝叚娆★紝鍑哄叚涓笉涓€鏍风殑鏂囦欢銆?* `[X] * N` 鐢熸垚鐨?`static` 濉厖杈呭姪鍑芥暟锛屽叾 C
鏍囪瘑绗︽槸鐢?`Expr` 鐨?*鍫嗗湴鍧€**鎷肩殑锛屼簬鏄湴鍧€鐩存帴杩涗簡鐢熸垚鐨?C銆傝緟鍔╁嚱鏁拌〃涔熺敤杩欎釜
鍦板潃鍋氶敭锛屾墍浠ヨ繛瀹氫箟**鍑虹幇鐨勯『搴?*閮芥槸闅忔満鐨勨€斺€斿嵆浣垮彧鏈変竴涓珯鐐癸紝闈炵‘瀹氭€т篃浼?
杩涘埌杈撳嚭閲?路

**涓や釜鍚庣鍦ㄥ懡鍚嶄笂浠庢潵灏辨病涓€鑷磋繃銆?* `selfhost/codegen.ax` 涓€鐩寸敤鏅€氳鏁板櫒缂栧彿
锛坄g.repn` 鈫?`ax_rep_0`銆乣ax_rep_1`锛屾寜棣栨浣跨敤椤哄簭锛夛紝鑰?Rust 鍚庣鍦ㄧ紪鍦板潃銆?
鎵€浠?*浠讳綍鐢ㄤ簡鏁扮粍澶嶅埗鐨勭▼搴忛兘涓嶅彲鑳介€氳繃鍥哄畾鐐?*鈥斺€斿彧鏄粠鏉ユ病浜哄啓杩囪繖绉嶇▼搴忋€?
鐜板湪绔欑偣鐢?`lit_id`锛坧arser 鎸夋簮鐮侀『搴忓垎閰嶇粰瀛楅潰閲忕殑 id锛夋爣璇嗭紝骞舵寜棣栨浣跨敤椤哄簭
缂栧彿鈥斺€旀鏄嚜涓惧彂灏勫櫒鏃╁氨鍦ㄧ敤鐨勬嫾鍐?路

**閭ｄ釜澹扮О瀹堟姢鍚庣涓€鑷存€х殑娴嬭瘯锛屽叾瀹炴槸鍦ㄦ嬁鑷妇鍜岃嚜宸辨瘮銆?*
`selfhost_driver_self_compiles` 鎶婂悓涓€涓?driver 鏋勫缓涓ゆ骞跺姣斿畠浠敓鎴愮殑 C锛?
浣?driver 鏈韩鏄釜 Aoxn 绋嬪簭銆佸鍏?`selfhost/driver.ax`锛屾墍浠?*涓や釜闃舵閮芥槸**
缁忕敱 `selfhost/codegen.ax` 鍙戝皠鐨勩€俙src/codegen_c.rs` 浠庡ご鍒板熬娌¤窡瀹冨姣旇繃銆?
鍦板潃鍛藉悕鐨勮緟鍔╁嚱鏁板氨鏄繖涔堟椿涓嬫潵鐨勩€傦紙涓よ€呯洰鍓嶅湪 `extern def` 澹版槑椤哄簭涓婄‘瀹?
涓嶅悓鈥斺€擱ust 鎸?C 鍚嶅瓧鎺掑簭锛岃嚜涓句晶淇濇寔棣栨浣跨敤椤哄簭銆傚悇鑷兘鏄‘瀹氱殑锛屾墍浠ヨ繖鏄?
"鍥哄畾鐐?*璇佹槑浜嗕粈涔?*"鐨勭己鍙ｏ紝涓嶆槸娲荤殑姝ｇ‘鎬?bug銆傦級路

**鑳芥姄浣忓畠鐨勯拤瀛愬繀椤昏法杩涚▼銆?* 鍚屼竴涓繘绋嬪唴鍒嗛厤鍣ㄤ細鎶婂悓涓€鍦板潃杩樺洖鏉ワ紝鎵€浠ヨ繘绋嬪唴
姣旇緝浼氬湪 bug 杩樺湪鐨勬儏鍐典笅椤哄埄閫氳繃銆傝繖涓祴璇曟妸浜岃繘鍒跺湪浜斾釜鐙珛杩涚▼閲岃窇浜旀锛?
瑕佹眰 C 閫愬瓧鑺備竴鑷?路

**v0.50.9** 路 **閭ｆ牴鎶樺彔閽夊瓙绗簩娆″湪 Linux 涓婅窇涓嶈捣鏉?* 路
瀹冪殑瀛樺湪鏄负浜嗕粠 Windows 涓婃鏌?Linux 杩為€氭€с€備絾鎶樺彔 `target_os()` 鏃跺悗绔敤鐨勬槸
**鏋勫缓涓绘満**鐨勫悕瀛楋紝鑰屼笖鏀惧湪**绗竴涓?*鍙傛暟涓娾€斺€旀墍浠?Windows 涓婃枃鏈槸
`("windows","windows")`锛孡inux 涓婃槸 `("linux","windows")`銆傞拤瀛愭槸鐓х潃 Windows
鏋勫缓鍐欑殑锛屼簬鏄?淇?鎴愪簡琛ヤ笂浣滆€?*鑳芥兂璞″埌**鐨勫彟涓€绉嶆嫾鍐欙紝鑰屼笖鍙ˉ浜嗕竴绉嶃€?
**姘歌繙涓嶈鐚滃彟涓€涓钩鍙颁細杈撳嚭浠€涔堬細鍘绘帹瀵煎畠** 路

**涓婁竴涓噷绋嬬**锛坴0.50.8锛夆€斺€?**v0.50.7 閭ｄ釜銆屽湪 Windows 涓婃鏌?Linux銆嶇殑閽夊瓙鍙湪 Windows 涓婃湁鏁?* 路
386 娴嬭瘯鍏ㄧ豢锛堢紪璇戝櫒宸ヤ綔鍖?293 + `aoxn-pkg` 94锛汭DE 鍙︽湁 36 涓?Rust + 62 涓墠绔祴璇曪級路

**閭ｄ釜鐢ㄦ潵鍦?Windows 涓婃鏌?Linux 鐨勯拤瀛愯嚜宸变篃鏄崐鐩茬殑銆?* 瀹冩妸
`__builtin_strcmp("windows", "windows")` 鏀瑰啓鎴?`"linux"`锛屽苟鏂█杩欎釜妯″紡瀛樺湪鈥斺€?
浣嗗悗绔槸鐢?*鏋勫缓鏃?*鐨?OS 鍚嶅瓧鍐欑浜屼釜鍙傛暟鐨勶紝鎵€浠ュ湪 Linux 鏋勫缓涓婃枃鏈湰鏉ュ氨鏄?
`("windows", "linux")`锛岄拤瀛愬湪閭ｉ噷鐩存帴澶辫触鑰屼笉鏄甯歌繍琛屻€傜幇鍦ㄤ袱绉嶆嫾鍐欓兘琚帴鍙楀苟
褰掍竴鍖栵紱鍦?Linux 鏋勫缓涓婃浛鎹㈡槸绌烘搷浣滐紝鑰岃繖鎭版伆姝ｇ‘鈥斺€旈偅涓洰鏍囨枃浠?*灏辨槸** Linux 鐨?
鐩爣鏂囦欢 路

**銆岃繖涓鏌ユ槸绌虹殑銆嶈繖鏉℃柇瑷€鍊煎洖浜嗘湰銆?* 瀹冪殑瀛樺湪鏄负浜嗛槻姝㈡姌鍙犲啓娉曟敼鍚嶅悗閽夊瓙鍙樻垚
銆岄€氳繃浣嗕粈涔堜篃娌℃祴銆嶇殑涓滆タ锛岃€屽畠鎶撳埌鐨勬鏄繖浠朵簨鈥斺€斾竴涓湪 CI 涓€鍗婄煩闃典笂浼氶潤榛?
澶卞幓鎰忎箟鐨勯拤瀛愩€備负涓€绫诲け璐ュ啓鐨勫畧鍗紝缁撴灉鍦?*鍚屼竴绫?*澶辫触閲屾淳涓婄敤鍦猴紝鏄畧鍗兘鏈夌殑
鏈€濂界粨鏋?路

**涓婁竴涓噷绋嬬**锛坴0.50.7锛夆€斺€?*HTTP 浼犺緭灞傚湪 Linux 涓婇摼鎺ヤ笉閫氾紝鑰岃繖浠朵簨鐜板湪鑳藉湪
Windows 涓婃鏌ヤ簡**鈥斺€?
**涓€澶?use-after-free锛屼節涓け璐ユ爣璁?*鈥斺€斿湪 Linux 涓婂畠涓嶅穿婧冿紝鑰屾槸璇磋皫銆傚甫璇婃柇鐨?
CI 鐩存帴璇翠簡锛歚FAIL chdir-no-move 鈥?sandbox`锛屾剰鎬濇槸 `os_cwd()` 鍦ㄩ偅娆℃湰璇ヤ骇鐢?
`sandbox` 鐨?`chdir` **涔嬪墠**灏卞洖绛斾簡 `.../sandbox`锛屼箣鍚庡張鍥炵瓟浜嗗悓涓€涓瓧绗︿覆銆?
`as_string` 鏄?*寮哄埗绫诲瀷杞崲銆佷笉鏄嫹璐?*锛坄codegen_c.rs` 鍙戝皠 `((char*)(x))`锛夛紝
鎵€浠ュ畠鍒悕鐫€浼犺繘鍘荤殑缂撳啿鍖衡€斺€旇€?POSIX 閭ｄ竴鏀繑鍥?`as_string(p)` 绱ф帴鐫€
`free(buf)`锛坄p` **灏辨槸** `buf`锛夈€俫libc 浼氭妸**鍚屼竴鍧?4096 瀛楄妭鐨?chunk** 鐩存帴
鍙戝洖缁欎笅涓€娆″悓鏍峰ぇ灏忕殑璇锋眰锛屼簬鏄浜屾璋冪敤灏卞湴瑕嗙洊浜嗙涓€娆＄殑绛旀锛氬伐浣滅洰褰曞凡缁?
鍙樹簡锛屼袱涓瓧绗︿覆鍗?*鐩哥瓑**銆備换浣曞厛璁颁笅 cwd銆佺◢鍚庡啀鍥炲幓鐨勮皟鐢ㄦ柟灏辩暀鍦ㄤ簡閿欒鐨?
鐩綍閲岋紝浜庢槸鍏跺悗姣忎竴鏉＄浉瀵硅矾寰勯兘澶辫触鈥斺€攇lob 缁撴灉涓虹┖銆乯son 鏂囦欢 I/O 鍏ㄧ槴锛?
閮藉彧鏄繖琛岀殑涓嬫父鐥囩姸 路
**use-after-free 鍙互璇磋皫鑰屼笉鏄穿婧冿紝鎵€浠ヨ鍦ㄨ繍琛屾椂閽変綇瀹?*銆傞潤鎬佹壂鎻忕湅涓嶈杩欎竴绫伙細
鍒悕缁忕敱涓や釜涓嶅悓鐨勫彉閲忓悕锛坄as_string(p)` 鍒悕鐫€涓€鍧楀悗鏉ヤ互 `buf` 涔嬪悕閲婃斁鐨?
缂撳啿鍖猴級锛岃€岃繖鎭版伆鏄涓€鐗堥潤鎬佹鏌ュけ鏁堢殑鍘熷洜銆傛柊閽夊瓙
`cwd-answers-own-their-buffer` 鏂█鍙瀵熺殑鎬ц川鈥斺€斿す鐫€涓€娆?`chdir` 鐨勪袱娆?
`os_cwd()` 蹇呴』涓嶅悓銆俙stdlib/` 閲?60 澶?`as_string` 宸插叏閮ㄥ璁★紝杩欐槸鍞竴涓€澶勭湡
闂 路
**浠ュ強 v0.50.4 閭ｆ璇婃柇鏀瑰姩鐨勬暀璁?*锛氫竴鍙?`FAIL chdir` 浠€涔堜篃娌″憡璇変綘锛屾妸瀹冩媶鎴?
鏈夊悕瀛楃殑鍑犻」涔嬪悗锛屼笅涓€娆″け璐ュ氨鍙樻垚浜嗕竴琛岀粨璁恒€?*鍘熷洜涓嶆槑鏃讹紝鎶婂姏姘旇姳鍦ㄨ瘖鏂笂锛?
鑰屼笉鏄寽娴嬫€у湴閲嶅啓銆?* 路

**涓婁竴涓噷绋嬬**锛坴0.50.4锛夆€斺€?*鏍囧噯搴撶敤纭紪鐮佺殑 `\` 鎷艰矾寰?*鈥斺€?

**`glob.ax` 涓?`pathlib.ax` 鎶婃瘡涓€鏉℃嫾鍑烘潵鐨勮矾寰勯兘鍐欐垚浜?`"\\"`**锛屼簬鏄?Linux 涓?
宓屽 glob 鍘?walk 涓€涓粈涔堜篃涓嶆寚鐨勮矾寰勶細`glob("sandbox/sub/*.txt")` 璧板埌
`os_exists("sandbox\\sub")`鈥斺€攆alse鈥斺€旇繑鍥炵┖锛涜€?`path_join` 浜ょ粰璋冪敤鏂逛竴涓病鏈?
浠讳綍鏂囦欢浼氬簲绛旂殑瀛楃涓层€傚叡浜斿锛歚glob_join`銆乣glob` 鐨勪袱涓?root 鍒嗘敮銆?
`path_join`銆乣path_with_suffix`銆乣path_with_name`锛屼互鍙?`path_norm` 閭ｄ釜 join
瀛楄妭銆俻athlib 鐜板湪鎻愪緵 **`path_sep()` / `path_sep_byte()`**锛岃€岃繖绉嶄笉瀵圭О姝ｆ槸
璁捐锛歚path_is_sep` 鍦?*杈撳叆**涓婃帴鍙椾袱绉嶅垎闅旂锛圵indows 璺緞鍙兘甯︿换涓€绉嶏級锛?
浣嗗嚱鏁?*浜у嚭**鐨勮矾寰勫繀椤绘寜鏈钩鍙版嫾鍐欒矾寰勭殑鏂瑰紡鎷硷紝鍚﹀垯瀹冨氨鏄彟涓€鏉¤矾寰?路
**鑰屼笖娴嬭瘯褰撴椂閽変綇鐨勬槸 bug銆佷笉鏄姄 bug**鈥斺€擿tests/pathlib.rs` 鏂█
`path_join("a","b") == "a\\b"`锛宍tests/osglob.rs` 鎷?`"sandbox\\a.txt"` 姣斿
glob 鍛戒腑锛氬湪 Windows 涓婂畠浠€氳繃锛岃€屽湪 Linux 涓婂嵆浣夸唬鐮佹敼瀵逛簡涔熶細鍥犱负**鎷煎啓**
鑰屽け璐ャ€備袱鑰呯幇鍦ㄩ兘鐢?`path_sep()` 鏋勯€犳湡鏈涘€硷紝骞舵柊澧?
`cross_platform_modules_never_hardcode_a_path_separator` 閽夊瓙鎵弿鍙屽钩鍙版ā鍧楅噷
杩欑鍐欐硶锛堝厛鍓ユ敞閲婏紱Windows 涓撳睘妯″潡澶╃劧璞佸厤锛屽洜涓哄畠浠彧鍦?`target_os()` 鍒嗘敮
閲屽啓 `\`锛壜?
**浠嶆槸寮€鏀鹃棶棰橈紝宸茬粡鏀规垚鍙瘖鏂€岄潪闈犵寽**锛歚osglob` 鐨?`chdir` 妫€鏌ヤ笌涓夊 json
鏂囦欢 I/O 鍦?Linux 涓婁粛鏈В閲婏紝椹卞姩鐜板湪浼氭姤鍑虹┒绔熸槸鍝竴椤瑰け璐ワ紝鑰屼笉鏄竴鍙?
`FAIL chdir`锛屼笅涓€娆?CI 鍗冲彲瀹氳 路

**涓婁竴涓噷绋嬬**锛坴0.50.3锛夆€斺€?*鏍囧噯搴撴椂閽熷湪 Linux 涓婇摼鎺ラ€氫簡锛屽嵈浠嶅湪鎶?369 骞村悗鐨勬椂闂?*鈥斺€?
浠?`GetSystemTimeAsFileTime` 鍑忔帀 1601 鐨勭邯鍏冨樊锛岃€?`clock_gettime(CLOCK_REALTIME)`
鏈潵灏卞湪 1970 涓婏紝鎵€浠?POSIX 閭ｄ竴鏀棤闇€鍐嶅噺銆倂0.50.1 鎶婂嚱鏁板悕锛坄..._ft`銆?
`FT_UNIX_EPOCH`锛夎鎴愪簡銆屽師濮?FILETIME銆嶏紝浜庢槸**鍙嶈繃鏉ュ姞**浜嗛偅娈靛樊銆傛墍鏈夎皟鐢ㄦ柟閮?
瑕侀櫎杩欎釜鍊硷紝浜庢槸 `time_unix`銆乣time_unix_ms`銆乣datetime_utcnow` 涓?
`datetime_now` 缁熺粺鍋忎簡 11644473600 绉掞細`13436106024` 鑰屼笉鏄?`1791632424`銆?
Windows 閭ｄ竴鏀竴鐩存槸瀵圭殑 路
**绗﹀彿琛ㄦ祴璇曞彧鑳借瘉鏄庛€岄摼鎺ュ緱涓娿€嶏紝姘歌繙璇佹槑涓嶄簡銆屽€煎銆?*鈥斺€攙0.50.1 鐨?CI 閽夊瓙
锛堣鐩爣鏂囦欢鐨勬湭瀹氫箟绗﹀彿锛夊湪妯″潡鎶ュ嚭 `FAIL now-year 2395` 鐨勫悓鏃朵緷鐒堕€氳繃銆傛洿绯熺殑鏄?
`time-unix-ms-agrees` 涔熼€氳繃浜嗭細瀹冩瘮杈冪殑鏄悓涓€涓椂閽熺殑涓ゆ璇绘暟锛屽**浠讳綍**甯搁噺鍋忕Щ
閮芥槸鐩茬殑銆傜湡姝ｆ姄浣忓畠鐨勬槸缁濆妫€鏌?`time-unix-sane`锛岀幇鍦ㄤ篃宸插姝ゆ爣娉?路
**杩欐澶辫触鏄竴娆″濂戠害鐨勮璇伙紝鎵€浠ユ敞閲婃墠鏄湡姝ｇ殑 bug**锛歚time_unix_ft` 鐨勬枃妗ｅ啓鐫€
銆宺aw 100 ns FILETIME ticks since 1601-01-01銆嶏紝鑰岃繖瀵?Windows 鍚屾牱鏄敊鐨勨€斺€旂揣鎸ㄧ潃
鐨勪唬鐮佸氨鍦ㄥ仛鍑忔硶銆傛枃浠跺ご銆佸嚱鏁版敞閲婁笌 `docs/stdlib.md` 鐜板湪閮藉啓鏄庤繖灞傚綊涓€鍖栵紝骞舵妸
銆岃鍙嶄簡瑕佷粯 369 骞淬€嶅啓鍦ㄩ噷闈?路
**鏂板妫€鏌?`now-is-local-and-coherent`**锛歅OSIX `struct tm` 鐨勫瓧娈靛亸绉绘鍓嶆棤浜鸿鐩栥€?
鍋忕Щ璇婚敊浠嶄細寰楀埌涓€涓?*鏍煎紡瀹屽ソ**鐨?`DateTime`鈥斺€旀妸 day 瀛楁鍋忕Щ 9锛宍now-year`銆?
`now-iso-len` 涓?`utcnow-year` 渚濈劧鍏ㄧ豢锛堣繖姝ｆ槸鏂版鏌ヨ鎶撶殑锛夈€傛柊妫€鏌ユ妸
`datetime_now()` 缁?`datetime_to_unix` 寰€杩斿洖鏉ワ紝瑕佹眰瀹冭惤鍦?`time_unix()` 鐨勪竴澶?
涔嬪唴鈥斺€旈偅姝ｅソ鍙槸 UTC 鍋忕Щ閲?路

**涓婁竴涓噷绋嬬**锛坴0.50.2锛夆€斺€?*涓冩潯 Dependabot 鍛婅锛氬叚鏉″凡娓咃紝涓€鏉℃棤鐗堟湰鍙崌**鈥斺€?

**涓ゆ潯 Next.js 缂撳瓨鎶曟瘨鍛婅**锛坴0.50.2锛夆€斺€擲SG/ISR 缂撳瓨鎶曟瘨鍦ㄨ嚜鎵樼搴旂敤閲屼細
鍙樻垚璺ㄧ敤鎴峰唴瀹规浛鎹笌鎸佷箙鎷掔粷鏈嶅姟銆傚熀鍑嗗簲鐢ㄤ粠 15.5.26 鍗囧埌 **15.5.27**锛堝畼鏂?
backport 鐗堟湰锛夛紱鍒绘剰鐣欏湪 15.x 鑰屼笉鏄烦 16.x锛氬熀鍑嗚鎶婂搷搴斾綋涓庡欢杩熷悓 Aoxn 
鏈嶅姟鍣ㄥ鐓э紝涓€娆″ぇ鐗堟湰璺宠穬鍙細鏃犵鎸姩杩欐潯鍩虹嚎 路
**sharp锛坙ibvsvg CVE-2026-96889锛変笌 source-map-js锛堢粡绱㈠紩 source-map 鍖烘
鍋忕Щ閫犳垚鐨勪簨浠跺惊鐜?DoS锛?*锛坴0.50.2锛夆€斺€斾袱鑰呴兘鏄?next 鐨勪紶閫掍緷璧栵紝鑰屽彧鍗囩骇 
next **骞朵笉浼?*绉诲姩瀹冧滑锛歴harp 0.35.4 鈫?0.35.5銆乻ource-map-js 1.2.1 鈫?1.2.2 
闈犵殑鏄?`pnpm update --depth Infinity`鈥斺€斿尯鍒湪浜庛€屽崌绾у憡璀︾偣鍚嶇殑鍖呫€嶄笌
銆岄偅涓寘鐪熺殑钀藉湪鑼冨洿鍐呫€嵚?
**涓ゆ潯 DOMPurify `IN_PLACE` XSS 鍛婅**锛坴0.50.2锛夆€斺€旂Щ闄よ妭鐐圭殑 `afterSanitize` 
閽╁瓙浼氳宸插垎绂诲瓙鏍戠殑浜嬩欢澶勭悊鍣ㄧ户缁瑁咃紝鑰岃寮哄埗绉婚櫎鐨?rawtext 鏍硅妭鐐规枃鏈惡甯?
鏀诲嚮鑰呮爣璁般€佷竴娆＄函 HTML 閲嶈В鏋愬嵆鎵ц銆俤ompurify 3.4.15 鈫?**3.4.16** 璧?
`pnpm.overrides`锛歮onaco-editor 鎶?dompurify 閽夋垚**绮剧‘鐗堟湰**锛岃€?0.57.0 宸叉槸 
monaco 鏈€鏂扮増鈥斺€旀病鏈夌増鏈彲鍗囷紝鍙湁 override 鍙敤 路
**杩樻湁涓€鏉℃病鏈夊彲鍙戠殑淇**锛坴0.50.2锛夆€斺€擱USTSEC-2024-0429锛坄VariantStrIter`
杩唬鍣ㄤ笉鍋ュ叏锛夊彧鍦?glib 鈮?0.20 淇锛岃€?glib 鎶佃揪 IDE 閿佹枃浠剁殑璺緞鏄?
wry 0.57.0 鈫?`gtk ^0.18` 鈫?`glib ^0.18`锛泈ry 涓?tao 閮藉凡鏄悇鑷渶鏂扮増锛屼簬鏄?
`cargo update` 姝ｇ‘鍦版妸 glib 鐣欏湪 0.18.5鈥斺€旀病鏈夌増鏈彲鍔ㄣ€俙ide/src-tauri/audit.toml` 
鎶婅繖娆℃帴鍙楄繛鍚岄€€鍑烘潯浠朵竴璧疯褰曞湪浠撳簱閲岋紱閭ｆ潯涓嶅仴鍏ㄨ矾寰勬槸 Linux 涓撴湁鐨勪紶閫?
webview 渚濊禆锛屾湰搴旂敤浠庝笉璋冪敤瀹冦€?

**涓婁竴涓噷绋嬬**锛坴0.50.1锛夆€斺€旀爣鍑嗗簱鐨勬椂閽熸ā鍧椾笉鍐嶆槸 Windows 涓撳睘锛?
`stdlib/time.ax` 涓?`stdlib/datetime.ax`锛坴0.44.0锛夋鏃犻槻鎶ゅ湴璋冪敤
`GetLocalTime`銆乣QueryPerformanceCounter`銆乣GetSystemTimeAsFileTime`銆?
`GetTickCount64` 涓?`Sleep`锛屼簬鏄换浣?import 瀹冧滑鐨勭▼搴忓湪 `ubuntu-latest` 涓?
閮芥鍦ㄩ摼鎺ユ楠わ紙`undefined reference to 'GetLocalTime'`锛夛紝涓旀病鏈変换浣曡瘖鏂?
鎸囧悜鍘熷洜锛沇indows 涓婂垯瀹屽叏鏃犳劅銆備袱鑰呯幇鍦ㄦ寜 `os.ax` 鑷?v0.46.0 璧风殑鍋氭硶鍙屽钩鍙?
鍖栤€斺€擯OSIX `clock_gettime` / `nanosleep` / `localtime_r` 鐢?`target_os()` 閫夋嫨鈥斺€?
涓斿畧鍗斁鍦ㄥ彂璧疯皟鐢ㄧ殑閭ｄ釜鍑芥暟閲岋紝鍥犱负缂栬瘧鍣ㄤ細鍙戝皠姣忎釜鍑芥暟浣撱€傛柊澧炰袱涓祴璇曪紝
宸ヤ綔鍖虹幇涓?290 涓?路

**涓婁竴涓噷绋嬬**锛坴0.50.0锛夆€斺€?*绗簩娆℃爣鍑嗗簱瀹¤锛歚len()` 鍗?`strlen` 鐨勪竴鏃忔竻鎵畬姣曪紝
浠ュ強鍦ㄥ惊鐜噷鍋氭嫾鎺ョ殑涓€鏃忛殢涔嬫竻鎵?*鈥斺€?

**鍗佷笁涓惊鐜浘閫愬瓧绗﹂噸鎵暣涓?*锛坴0.50.0锛夆€斺€擿len()` 浣滅敤鍦?string 涓婂氨鏄?
`strlen`锛屼笖鍦ㄦ瘡涓娇鐢ㄧ偣鍐呰仈鍙戝皠锛涘彧**璇?*瀛楃涓茬殑寰幆浼氳 clang 褰撴垚寰幆
涓嶅彉閲忔彁鍗囷紝鑰岄€氳繃鍙︿竴涓寚閽?*鍐?*鐨勫惊鐜笉浼氾細 clang 鏃犳硶璇佹槑閭ｇ瑪鍐欏叆涓嶄細
涓庡瓧绗︿覆閲嶅彔锛屼簬鏄暣涓叉瘡瀛楃閲嶆壂涓€閬嶃€傛渶鐙犵殑涓や唤灏卞湪姣忎釜 `os_*` 璺緞璋冪敤涓?
姣忎釜 WinHTTP 瀹藉瓧绗︿覆鐨勮韩鍚庯紙UTF-16 缂栫爜鍣細4096 瀛楃 1950 碌s 鈫?10 碌s锛夛紱
URL 涓庤姹傚ご鎵弿銆丣SON 鏁板瓧璺緞锛? 涓囦釜鏁板瓧 1018 ms 鈫?7鈥?3 ms锛夈€乬lob 涓?
pathlib 鐨勬壂鎻忔槸鍏朵綑 路
**鍗佷釜寰幆鏇剧敤 `out = out + 鈥 澧為暱瀛楃涓?*锛坴0.50.0锛夆€斺€旀嫾鎺ユ瘡涓€姝ラ兘鏂板垎閰?
涓€鍧楃紦鍐插苟涓㈠純鏃х殑锛屼簬鏄竴娆?200 鏉℃秷鎭殑浼氳瘽娓叉煋瑕?2.1 s锛屼笖姣忎竴姝ョ殑涓棿
浜х墿閮藉湪娉勬紡锛堝瓧绗︿覆鏈氨鎸夎璁′笉閲婃斁锛屾氮璐瑰湪姝ユ暟锛夛細鐜板湪绾?3 ms锛屽啓杩?
`json.ax` 鏃㈡湁鐨?`JOut` 涓?`stdlib.ax` 鏂板鐨?`StrBuf`銆傝緭鍑轰竴瀛楄妭鏈彉锛?
姣忓鏀瑰啓閮芥湁閫愬瓧鑺傞拤瀛?路
**鏀瑰啓鏈韩閫煎嚭涓€涓柊鐨勫穿婧冪被**锛坴0.50.0锛夆€斺€旂粨鏋滃彲鑳戒负**绌?*鐨勫瓧鑺?sink锛?
浼氭妸鏀跺熬鐨?NUL 鍐欒繘涓€涓粠鏈垎閰嶇殑缂撳啿鍖猴紙`0xC0000005`锛氫竴涓病鏈?text 鍧楃殑
鍝嶅簲鈥斺€旂函宸ュ叿璋冪敤锛屼竴绉嶆甯稿洖绛旓級銆備笁澶勮繛鎺ュ嚱鏁扮幇鍦ㄩ兘杩斿洖 `""`锛?
`msg-text-empty` 鏄拤瀛?路
**鏂板師璇?*锛坴0.50.0锛夛細`buf_str(p, n)`鈥斺€斿凡鐭ョ墖娈甸暱锛堣В鏋愬櫒 token銆丼SE 涓€琛岋級
鏃剁粫寮€ strlen 鐨勩€屾寚閽?+ 闀垮害銆嶅啓娉曪紱浠ュ強 `StrBuf` / `sb_*`锛屼竴涓皬鍨嬪嚑浣?
澧為暱瀛楄妭 sink銆俙stdlib.ax` 鍦ㄨ嚜涓惧叧閿矾寰勪笂锛屼袱鑰呴兘鏄函 Aoxn锛涘浐瀹氱偣閽堝
鏀瑰姩鍚庣殑鏂囦欢閲嶆柊楠岃瘉 stage-1/stage-2 閫愬瓧鑺備竴鑷淬€?

**涓婁竴涓噷绋嬬**锛坴0.49.0锛夆€斺€擴I 宸ュ叿绠卞墿浣欑殑闈欓粯閿欒涓庤緭鍏ュ懆鏈熺殑鍗佷簲浠?
鎷疯礉锛氱 9 灞傚竷灞€鏇惧脊鎺夌 8 灞傦紙璇ュ抚浣欎笅鎺т欢钀借繘閿欒鐨勭洅瀛愪笖鏃犲鎶ュ憡锛夛紝
`itoa10` 鐨?arena 閫冮€稿湪缁樺埗璺緞鏈熸湜 ASCII 澶勮繑鍥?UTF-16锛坅rena 涓€婊★紝
`ui_label_int` 鍙敾涓€浣嶆暟瀛楋級锛宍utf16_write` 鏄?O(n虏)锛?048 瀛楃涓€娆?
626 碌s 鈫?3 碌s鈥斺€攙0.50.0 闅忓悗鍦ㄥ畠鐨勪袱涓鐢熷厔寮熼噷鎵惧埌浜嗗悓涓€涓櫡闃憋級锛?
X11 瑁佸壀鏍堝彲鑳藉脊鍑轰粠鏈?push 杩囩殑鐭╁舰锛宲ress-claim/click-release 鍛ㄦ湡鐜板湪鏄?
`ui_press` 瀹舵棌銆傛帶浠剁鍚嶄竴涓病鍙?路

**JSON 鏋勫缓鍣ㄥ彲鑳藉湪浣犺剼涓嬫妸宸茶В鏋愯妭鐐圭殑缂撳啿鍖烘敼灏?*锛坴0.48.0锛夆€斺€旇В鏋愬櫒浼?
鎺ョ `Vec` 鐨勫瓙缂撳啿鍖猴紝鍗存妸鑺傜偣鐨勫閲忔Ы鐣欐垚 0锛岃€屾墿瀹归€昏緫鎶?0 璇绘垚銆岄粯璁?8銆嶏紝
缈诲€嶅埌 16锛屼簬鏄妸涓€涓?20 鎴愬憳瀵硅薄鐨?256 瀛楄妭鍧?*鍚戜笅** `realloc` 鍒?128 瀛楄妭锛?
鍐嶅線绗?20 妲藉啓銆備换浣?*鎴愬憳鏁?鈮?6** 鐨勫凡瑙ｆ瀽瀵硅薄鎴栨暟缁勶紝绗竴娆″姞瀛楁灏变細浠?
`0xC0000005` 宕╂簝鈥斺€旇€岄偅鎭版伆鏄?`jb_set_raw` 鐨勭敤閫旓紝涔熸鏄畠闀挎湡娌¤鍙戠幇鐨勫師鍥狅細
SDK 娴嬭瘯鍙瀯寤鸿繃鍏ㄦ柊鐨勮妭鐐广€傚樊鍒嗗疄楠屽潗瀹烇紙8 涓垚鍛樻甯革紝20 涓垚鍛樿秺鐣岋級銆?
鐜板湪鎵╁涓€寰嬩互**瀛樻椿瀛愯妭鐐规暟**涓轰笅闄?路
**骞朵笖 `len()` 浣滅敤鍦?string 涓婂氨鏄?`strlen`锛屼笖鍦ㄦ瘡涓娇鐢ㄧ偣鍐呰仈鍙戝皠**锛?
杩欒涓夊寰幆鍙樻垚浜屾鏂癸細`j_at` 鐨勮緭鍏ョ粨鏉熷垽鏂紙JSON 瑙ｆ瀽鍣ㄥ彧璺戝埌 ~33 KB/s锛?
266 KB 鐨勫搷搴旇 7 绉掞級銆乣jp_str` 鐨勭紦鍐插尯澶у皬锛堟瘡涓覆閮芥寜鏁寸瘒鏂囨。鍒嗛厤锛?
330 KB銆? 涓囦釜涓茬殑杈撳叆宄板€?325.8 MB锛夈€乣b64_decode` 鐨勫惊鐜笂鐣岋紙1.4 MB 鍦?
600 绉掑唴閮芥病璺戝畬锛夈€俙j_dumps` 鍙堝彔鍔犱簡涓€灞傗€斺€斿瓧绗︿覆鎷兼帴姣忔鏂板垎閰嶄竴鍧楀苟涓㈠純
鏃х殑锛屼簬鏄?2 涓囦釜閿殑鍝嶅簲鍚冩帀 **8627.8 MB 鍜?28.4 绉?*銆傚洓澶勭幇宸插叏閮ㄧ嚎鎬э細
266 KB 瑙ｆ瀽 7 ms锛屽悓涓€浠?2 涓囬敭鍝嶅簲搴忓垪鍖?282 ms / 10.2 MB锛宐ase64 澶勭悊
2.8 MB 鏄?3 ms銆傚悓鏃朵慨鎺夛細JSON 鎸囨暟鏃犱笂鐣岋紙`1e999999999` 浼氱┖杞暟鍒嗛挓锛?
瀵逛换浣曚笉鍙椾俊鐨?JSON 閮芥槸鍙揪 DoS锛夈€乥ase64 鐨勬嫆缁濊矾寰勬硠婕忎簡闀垮害鐢卞彂閫佹柟
鍐冲畾鐨勭紦鍐插尯銆佷互鍙?`os_has_env` 璇讳簡闄堟棫鐨?`GetLastError`鈥斺€擶in32 鎴愬姛鏃跺苟涓?
娓呴櫎瀹冿紝浜庢槸浠讳綍涓€娆″厛鍓嶇殑鏌ユ壘鏈懡涓兘浼氳**纭疄瀛樺湪**鐨勫彉閲忚绛旀垚 `False` 路

**涓婁竴涓噷绋嬬**锛坴0.47.0锛夆€斺€擴I 宸ュ叿绠辩粓浜庣敾鍑轰簡瀹冭鐢荤殑涓滆タ锛屼篃涓嶅啀涓烘病浜鸿兘鐪嬭鐨勪笢瑗夸粯璐?
鈥斺€擥DI 鐨勮鍓爤浠庢潵娌¤兘鎭㈠杩囷紙`IntersectClipRect` 鍙細鏀剁獎锛?
鎵€浠ユ棫鐨?pop 鍙細鏀跺緱鏇寸獎锛夛紝浜庢槸**涓€甯ч噷鍙鍑虹幇涓€涓?`ui_textbox`锛屾暣涓?
绐楀彛浠庨偅涓€鍒昏捣灏辫瑁佸埌閭ｄ釜 textbox 鐨勭煩褰㈤噷锛岀洿鍒拌繘绋嬬粨鏉?*鈥斺€擿BitBlt`
涔熶竴鏍蜂腑鎷涳紝鍥犱负 blit 鍙嫹璐濇簮 DC 瑁佸壀鍖哄厑璁哥殑閮ㄥ垎銆俙ui_win.ax` 鐜板湪姣忓眰
淇濆瓨涓€涓?`SaveDC` 鍙ユ焺銆乸op 鏃?`RestoreDC`锛涙帶浠跺眰鍚屾椂淇濈暀鑷繁閭ｄ唤瑁佸壀鏍?
褰卞瓙锛坧op 浜庢槸鑳界湡鐨勮繕鍘熺埗鐭╁舰锛屼篃鑳介『鎵嬪墧鎺夎鍓尯鐪嬩笉瑙佺殑缁樺埗鈥斺€?*-40 %**锛?
鍦烘櫙鏄?3000 px 鏂囨。濉炶繘 200 px 瑙嗗彛锛夛紱`ui_measure` 鐢?64 椤规寜鍐呭鍝堝笇
绱㈠紩鐨勭紦瀛樺洖绛旓紙閲嶅娴嬮噺 **96脳**锛氭瘡 2 涓囨 52.8 ms 鈫?0.55 ms锛夛紱Windows
鍚庣杩樹細璁颁綇 DC 宸查€変腑鐨勭敾绗斻€佺敾鍒枫€佸瓧浣撲笌棰滆壊锛屼笉鍐嶆瘡娆￠噸閫夛紙姣忔濉厖
**-6 %**锛?00 鎺т欢鍦烘櫙鏁翠綋绾?**-9 %**锛夈€傛柊澧炵殑鍍忕礌鍥炶娴嬭瘯
`ui_clip_stack_restores_and_culls` 鍦ㄦ棫鏍囧噯搴撲笂浼氭姤涓冨涓嶇銆傝瘹瀹炴暟瀛楋細
閲嶅満鏅弽鑰?*鍙樻參**浜嗭紙1.8 鈫?3.5 ms/甯э級锛屽洜涓烘棫鐨勯偅涓牴鏈病鐢诲嚭澶ч儴鍒?
甯?路

**涓婁竴涓噷绋嬬**锛坴0.46.0锛夎 Linux 鍥炲綊锛屽苟缁忓悓涓€濂?`plat_*` 濂戠害缁欏伐鍏风
鍔犱笂绗簩涓悗绔€斺€攙0.30.0 鏇炬妸骞冲彴娓呭崟鐮嶅埌鍙墿 Windows锛泇0.46.0 鎶?
Linux 甯﹀洖缂栬瘧鍣紙ELF 鐩爣鏂囦欢銆佽嚜鍔ㄩ摼 `-lm`銆佷笉鍐嶄紶 `/STACK` 鏍囧織锛夈€?
鏁村娴嬭瘯锛堟柊澧?`ubuntu-latest` CI 浠诲姟锛変笌**閲嶅啓鐨?`stdlib/ui_x11.ax`**锛?
Xlib 璐熻矗绐楀彛涓庤緭鍏ャ€乆ft 璐熻矗鍙嶉敮榻?UTF-8 鏂囨湰銆佺灞?Pixmap 鍙岀紦鍐层€?
X11 selection 鍗忚鍓创鏉裤€佹瘡甯т竴娆?`XQueryKeymap` 鎵弿锛堟寜浣忕殑閿笉鍐嶅湪
鑷姩閲嶅鐨?release+press 瀵逛箣闂撮棯鐑侊級锛屽叧闂姹傜粡鍏变韩鍧?824 妲戒紶閫?
锛圓oxn 鎸夊€间紶 `UI`锛宍plat_close` 鏃犳硶鎶婂瓧娈靛啓鍥炲簲鐢ㄥ惊鐜級銆俙target_os()`
鐜板湪鎶樺彔鐨勬槸**缂栬瘧鍣ㄦ瀯寤烘椂**鐨勫涓?OS鈥斺€擿"windows"` 鎴?`"linux"`鈥斺€旇€?
鑷妇缂栬瘧鍣ㄤ笉闇€瑕佽嚜宸辩殑骞冲彴妫€娴嬶細瀹冪殑鍐呭缓鍙戝皠鍣ㄧ洿鎺ヨ皟鐢?`target_os()`锛?
杩欎釜鍊肩敱鏋勫缓瀹冪殑缂栬瘧鍣ㄧ儳瀹氾紝鍥犳鍚屼竴鍙版満鍣ㄤ笂鍥哄畾鐐归摼鏉＄殑姣忎竴绾ф姌鍙犵殑鏄?
鍚屼竴涓涓?OS锛屽浐瀹氱偣鎸夋瀯閫犻€愬瓧鑺傛垚绔嬨€傛爣鍑嗗簱**鍒嗘敮鑰岄潪鍒嗗弶**锛歚os.ax`
鍦?Linux 涓婅蛋绾?libc锛坢kdir/stat/opendir/readdir/getenv/setenv锛実libc
缁撴瀯浣撳亸绉婚噺鍦ㄦ枃浠跺ご娉ㄦ槑锛夛紝`W` 绯诲垪 Win32 璋冪敤缁х画鏈嶅姟 Windows鈥斺€?
鏈寮曠敤鐨?`extern def` 涓嶄骇鐢熺鍙峰紩鐢紝鎵€浠ユ瘡涓钩鍙板彧閾炬帴鑷繁閭ｄ竴渚э紱
Linux 涓?`exe_path()` 缁?`/proc/self/exe` 鍥炵瓟銆備繚鎸?Windows-only 鐨勯儴鍒?
浠ヨ瘹瀹炵殑鏂瑰紡瀹堢潃锛欻TTP 浼犺緭灞傞獞鍦?WinHTTP 涓婏紝Linux 涓婂叆鍙ｈ繑鍥炲共鍑€鐨?
銆屾湰骞冲彴鏆傛棤浼犺緭灞傘€嶉敊璇€屼笉鏄摼鎺ュけ璐ワ紱鍗曟枃浠跺畨瑁呭櫒浠嶆槸 Windows 宸ヤ欢銆?
鍚屼竴鐗堟湰杩樺姞鍏ユí鍚戞粴鍔ㄥ尯鍩燂紙`ui_scroll_begin_h` / `ui_scroll_end_h`锛?
瑁佸壀鏍堝彇浜ら泦锛屾晠绔栧悜涓庢í鍚戝彲宓屽锛変笌绛夊瀛椾綋锛坄ui_font_mono` /
`ui_font_sans`锛壜?

**鍐嶄箣鍓?*锛坴0.45.0锛夋槸瀵?UI 鏈哄埗鐨勬€ц兘涓庢纭€ф暣澶囷細娴眰鍒楄〃瓒呰繃
32 椤圭殑婧㈠嚭閽冲埗銆丱(n) 鏂囨湰瀹氫綅銆佸嚟姣忚妭鐐瑰瓙璁℃暟鍋氬埌 O(1) 鐨?
`tree_has_child`銆佸厜鏍?x 鍋忕Щ缂撳瓨锛坰t 532..535锛夈€佸彲閿洏瀵艰埅鐨勬ā鎬佽彍鍗曘€?
鍚堝苟涓轰竴涓殑 `sb_widget`锛泇0.44.1 淇 `os_getenv` 鎴柇瓒呰繃 2048 瀛楃鐨?
鍊硷紱v0.44.0 钀藉湴鏍囧噯搴撳墠鍗佷笁涓ā鍧楋紙鎸夊悕瀛楀鍏ワ紝娌℃湁涓€涓
`stdlib/stdlib.ax`锛屽浐瀹氱偣涓嶅姩锛夛紱v0.43.0 璁╂ā鍧楁垚涓哄彲鐐瑰悕鐨勫懡鍚嶇┖闂达紱
v0.42.0 鍏抽棴鏃ュ父缂哄彛锛堣繘鍒跺瓧闈㈤噺銆乣\r`/`\0`銆乣assert`/`exit`銆?
`argc()`/`arg(i)`銆乣cc` 闃舵銆佽鍛婂眰锛夛紱v0.41.0/v0.40.1 鍦ㄥ叡浜殑 WinHTTP
浼犺緭涓婅惤鍦?Anthropic 涓?OpenAI 涓ゅ SDK锛泇0.40.0 鍔犲叆鍑芥暟鎸囬拡銆?
`None`/`T | None`銆乣raise`/`try`/`except` 涓?`dict[V]`銆傚畬鏁村巻鍙茶
[`CHANGELOG.md`](CHANGELOG.md) 路

闀挎湡浜嬪疄锛氳嚜涓惧浐瀹氱偣锛堢敓鎴愮殑 C + 鐩爣鏂囦欢閫愬瓧鑺備竴鑷达級路 鏍囧噯搴撳唴缃?
UI 宸ュ叿绠?v3锛圦t 绾э細甯冨眬绠＄悊鍣ㄣ€佹枃鏈緭鍏ャ€佺劍鐐归摼銆?0 浣欐帶浠躲€佹诞灞傝鐩?
鈥斺€擿examples/ui_gallery.ax`锛壜?闆?LLVM 渚濊禆锛欳 鍙戝皠鍚庣鏄敮涓€鍚庣
锛坈lang 缂栬瘧鐢熸垚鐗╋級路 TS-M1 W1 鏀跺畼 路 鍖呯鐞嗗櫒 beta锛圥ubGrub銆佺瓥灞?
registry銆乶pm 妗ユ帴锛壜?鍗曟枃浠跺畨瑁呭櫒锛坄Aoxn-<version>-Setup.exe`锛岄粯璁?
涓嶄笅杞戒换浣曚笢瑗匡級銆?

瀹夎姝ラ瑙?[`docs/install.md`](docs/install.md)锛屽畬鏁磋瑷€瑙勮寖瑙?
[`docs/spec.md`](docs/spec.md)锛屽彂甯冨巻鍙茶
[`CHANGELOG.md`](CHANGELOG.md)銆?

## 璁稿彲璇?

Apache-2.0 鈥斺€?瑙?[`LICENSE`](LICENSE)銆?
