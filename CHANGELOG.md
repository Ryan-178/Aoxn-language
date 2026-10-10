# Changelog

Notable changes to the Aoxn compiler and language. Aoxn follows semver-ish
minor bumps while pre-1.0: each minor version is a language milestone.

## [Unreleased]

## [0.50.1] - 2026-10-10

### Fixed

- **`stdlib/time.ax` and `stdlib/datetime.ax` could not link on Linux.**
  Both modules (v0.44.0) called the Win32 clocks unconditionally —
  `QueryPerformanceCounter`, `GetSystemTimeAsFileTime`, `GetTickCount64`,
  `Sleep`, `GetLocalTime` — so every program importing them failed at the
  link step on Linux (`undefined reference to 'GetLocalTime'`,
  `clang: error: linker command failed with exit code 1`), which is where
  the `ubuntu-latest` CI job stood at `tests/datetime.rs`. Invisible on
  Windows, where those symbols exist. Both modules are now dual-platform,
  the shape `stdlib/os.ax` already used: POSIX `clock_gettime` /
  `nanosleep` / `localtime_r` behind `target_os()`, with the glibc
  `struct tm` offsets documented in the source.

- **The platform guard has to sit in the function that makes the call, not
  only in its caller.** The first version of this fix guarded the six
  public entry points and put the POSIX clocks in shared helpers, and it
  failed on Windows instead (`undefined symbol: clock_gettime`): the
  compiler emits EVERY function body, so an unguarded helper drags its
  platform's symbols into the link no matter which branch reaches it. The
  rule is now recorded in both module headers.

  Why the branch is dropped at all: `if target_os() == "windows"` is
  emitted as `__builtin_strcmp("windows", "windows") == 0`, and clang
  constant-folds that before code generation — so the branch not taken
  never becomes a call, and its symbols never reach the object file. This
  holds at `-O0` as well as `-O3` (verified both with a standalone clang
  probe and by reading the undefined-symbol table of the real generated
  object), which is what makes the "declare both platforms, call one"
  idiom work at all.

- Pinned by `tests/datetime.rs::clock_modules_reference_only_this_platform`:
  it compiles every clock entry point to an OBJECT FILE and asserts the
  object names this platform's clock symbols and NOT the other platform's.
  It is a two-way pin — the same assertion runs on both CI platforms, so
  a missing guard on either side fails the build instead of waiting for a
  user on the other OS.

## [0.50.0] - 2026-10-10

Theme: **the second stdlib audit — the `len()`-is-`strlen` sweep finished,
and the `out = out + ...` sweep that followed it.** v0.48.0 found the trap
three times in the JSON DOM; this pass found it in thirteen more places,
plus a whole second family: loops that GROW a string by concatenation.
One new crash class surfaced during the rewrites, and it is pinned by name
(`msg-text-empty`).

### Fixed: the write-pointer aliasing trap, in thirteen places

`len()` on a string is `__builtin_strlen`, emitted INLINE at every use site
(`codegen_c.rs:1973`). A loop that only READS its string gets that strlen
hoisted out by clang as loop-invariant — which is why `css_has_quote` was
always fast. A loop that WRITES through another pointer, or calls anything
opaque, does NOT: clang cannot prove the store does not alias the string,
so the whole string is re-scanned once per character. Every site below is
one of those:

| site | what it cost before | after |
|---|---|---|
| `os_utf16_write` (os.ax) | 1950 µs/call @ 4096 chars | 10 µs |
| `net_utf16_write` (net/codec.ax) | 2209 µs/call | 12 µs |
| `net_to_wide` (the wrapper both SDKs use) | 1959 µs/call | 9 µs |
| `net_url_encode` | 141 µs/call @ 1024 | 9 µs |
| `an_msgs_json` | 2105 ms @ 200 msgs / 1.6 MB | ~3 ms |
| `oa_body_chat` | 1394 ms | ~8 ms |
| `j_parse` (numbers doc, 149 KB) | 1018 ms | 7–13 ms |
| `datetime_format` | 1552 µs/call @ 2000 | ~240–360 µs |
| `path_norm` | 108 µs/call @ 65 parts | ~39 µs |

`os_utf16_write` and `net_utf16_write` are the twins of the `utf16_write`
fixed in v0.49.0 in `ui.ax` — the same function, deliberately duplicated
(AGENTS.md), fixed in one copy only. `os_utf16_write` sits under EVERY
`os_*` call that takes a path.

The rule, now recorded where it is violated: **hoist `len()` in the source,
once, before the first store** — do not rely on the optimizer's aliasing
proof, because one added pointer-store turns a hoisted strlen back into a
per-character scan with no test noticing.

### Fixed: JSON numbers re-scanned the whole document per token

`jp_num` extracted each number's text with `str_sub(p.src, start, p.pos)`,
and `str_sub` pays `len(p.src)` to clamp bounds that cannot fire inside
the parser. One whole-document strlen per number: a 20 000-number array
cost 1 s. The span is already known, so the extraction is now
`buf_str(as_ptr(p.src) + start, p.pos - start)` — a pointer-plus-length
copy that scans nothing. `j_num_float`'s three loops and the integer
fast path hoist their bounds the same way. 266 KB of mixed JSON is
unaffected; strings were already direct.

### Changed: ten loops stopped building strings with `out = out + ...`

Concatenation allocates a fresh buffer per step and abandons the old one,
so an n-step build costs O(n²) allocations AND O(n²) leaked bytes (strings
leak by design — that is the language's contract, not a bug). Rewritten
into byte sinks that grow geometrically: `an_msgs_json`, `an_blocks_json`,
`an_body*`'s joins, `an_msg_text`, `an_msg_thinking`, `oa_body_chat`,
`oa_body_embeddings`, `oa_responses_text`, `datetime_format`,
`net_url_encode_pairs` and the `path_norm` join. The SDKs route through
`json.ax`'s existing `JOut` (one escaping implementation, already
imported); `datetime` and `pathlib` use the new `StrBuf` in `stdlib.ax`
rather than pulling the JSON DOM — and through it `net/codec.ax` — into
every program that formats a date.

**No output changed.** Every rewrite is pinned byte-for-byte
(`stdlib_perf_pins`, plus new markers in the two SDK suites).

### New: `buf_str(p, n)` and `StrBuf` / `sb_*` in stdlib.ax

`buf_str` is the pointer-plus-length escape from strlen: callers that
already know a span (a parser token, an SSE line inside a bigger buffer)
copy exactly those bytes and scan nothing. `StrBuf` is a small growable
byte sink (`sb_new` / `sb_byte` / `sb_str` / `sb_text`), threaded by value
like `Vec` — the language has no address-of. `net_sse_next` uses `buf_str`
for its per-line extraction, which used to `strlen` the whole network
buffer per line.

`stdlib.ax` is on the self-host critical path, so both additions are plain
Aoxn with no v0.40.0 constructs; the fixed point
(`selfhost_driver_self_compiles`) re-verifies byte-identical stage-1 /
stage-2 output against them.

### Fixed: a sink that never appended crashed on its final NUL

Found by the rewrites themselves, on the FIRST run of `an_msg_text`: the
three joiners whose result can legitimately be EMPTY (`an_msg_text`,
`an_msg_thinking`, `oa_responses_text`) started from
`JOut(buf=0, n=0, cap=0)` and ended with `store_u8(w.buf, w.n, 0)`. A
response with no text block is NORMAL — a pure tool call, an empty
accumulator — and there `w.buf` is still NULL: a write through a null
pointer, surfacing later as `0xC0000005` on some unrelated heap operation.
The old `out = ""` spelling answered `""`. All three now answer `""`
(`msg-text-empty` is the pin; it fails on the buggy rewrite by crashing).

### Tests

- `tests/stdlib_perf_pins.rs` — 16 driver checks: the empty-sink answers,
  byte-identical bodies and formats, `path_norm` root shapes, JSON number
  extraction, percent-encoding, UTF-16 byte counts (both encoders
  byte-equal), `str_sub` clamping, `buf_str`, `StrBuf` growth, the hoisted
  glob matcher, and the SSE line extraction.
- `anthropic_sdk.rs` / `openai_sdk.rs` — new markers
  `msg-text-empty`, `msg-text-two`, `body-chat-empty`,
  `responses-text-empty`, `responses-text-two`.
- The full suite: 289 tests, all green; the self-hosting fixed point
  included.

## [0.49.0] - 2026-10-09

Theme: **the UI toolkit's remaining silent failures, and the fifteen copies of
its input cycle.** v0.47.0 fixed the clip stack and the caching; this one is
about the parts that were wrong in ways no test could see, plus the quadratic
UTF-16 encoder that was hiding in plain sight.

### Fixed: a 9th layout level silently popped the 8th

`lay_push` REFUSED a box past the 8-level cap by returning without pushing —
but `ui_layout_end` decremented the depth unconditionally, so the 9th nesting
level popped its own 8th, and every later `ui_layout_end` unwound one level
too deep. The rest of the frame's widgets landed in the wrong box, with
nothing anywhere reporting it. A refused push now balances its own end
against a separate counter (st 559), so the 8 live levels are untouchable,
and `ui_layout_overflow(c)` / `ui_layout_depth(c)` make it observable.

### Fixed: `itoa10` returned UTF-16 on its escape path

`itoa10` writes plain ASCII digits into the frame arena, which is what the
draw path expects (`plat_text_sub` runs the bytes back through the UTF-8
encoder). Its arena-exhaustion escape returned `ui_utf16(str(v))` — UTF-16,
NUL-interleaved — so **whenever the bump arena filled up, `ui_label_int` drew
a number with a NUL after every digit**, and both `len()` and `as_string()`
stopped at the first one. The escape now returns the Aoxn string itself
(ASCII, immutable, leaks by design like every other one-shot buffer).
Measured before/after on an `itoa10`-shaped call: `1/1` → `12345/5`.

### Fixed: `utf16_write` was O(n²)

`while i < len(s)`, and `len()` on a string is `strlen` **emitted inline at
every use site** — so the encoder re-scanned the entire string once per
character. `utf16_write_sub` already hoisted its bounds; this is its
single-entry twin and it did not. Measured on this box (`--O3`, 20000 calls,
2048-char ASCII string):

| | `utf16_write` |
|---|---|
| before | 626 µs / call |
| after | 3 µs / call |

Cost fell from ~15 650x to ~300x for a 16x length ratio — quadratic to
linear. This is the same trap v0.48.0 found three times in the JSON DOM;
`ui.ax` had its own copy.

### Fixed: the X11 clip stack could pop a rect it never pushed

`plat_clip_push` stored the rect AND incremented the depth inside one
`if depth < 30` branch, but `plat_clip_pop` decremented unconditionally. A
31-deep nest therefore popped a rect that was never pushed and kept going,
walking the stack off its own base. The depth now always advances; only the
storage saturates, and `clip_apply` re-uses the deepest rect it did keep.

### Changed: one press cycle instead of fifteen copies

Fifteen widgets opened with the same eleven lines — read the claim from
`st 0`, take it on the press edge while taking focus, report a click on the
release edge — and the copies had already drifted (some kept a local `act`,
some did not, one forgot the focus). They are one cycle now:
`ui_press`, `ui_press_claim` / `ui_press_release`, `ui_pressed`. The two
load-bearing details are documented where they live, because both are easy to
lose in a rewrite: the claim is only taken when the slot is free, so an
overlay that swallowed the press (`st 0 == -1`) is never stolen by a widget
drawn underneath it; and the slot stays claimed for the whole hold, which is
how the dragging widgets and the pressed-face colours recognise their own
press. Wheel and scroll clamping are shared the same way
(`ui_wheel_scroll` / `ui_clamp_scroll`) — that is where the per-notch rate
differences live (the text edit scrolls 3 notches per event, the scroll area
a third of one) and where the copies had begun to disagree.

**No widget signature changed.**

### Changed: a line height is a font property, not a string property

`ui_textbox` measured its WHOLE field every frame purely to read `ts.h`, for
vertical centring. The v0.47.0 measure cache removed the GDI call but not
the **O(n) content hash** that cache is keyed on, so a several-hundred-char
field still paid a full scan per frame to learn a constant. `ui_font_height`
measures one character, caches it in slot 830, and drops it on a font switch
alongside the measure cache.

### Fixed: stale documentation

- the `plat_*` contract comment claimed `ui_draw.ax` carries headless
  fallback stubs and that a backend's definitions override them — there are no
  stubs in the file, and since v0.46.0 there are two backends, not one;
- `plat_canvas_new/resize/free` named a primitive that does not exist
  (`plat_canvas_resize` does);
- slot 819 was documented as "the textedit anchor of the drag in flight" while
  the code uses it as the TEXTBOX selection anchor. It is now documented as
  what it is, including the consequence: there is one pair, so the caret does
  not survive a focus switch.

### Tests

- `ui_layout_overflow_and_arena_escape` — the layout cap balances against its
  own overflow counter (the `8 0` on the third line is what fails on the old
  code), and `itoa10`'s escape renders the same digits and the same `len()` as
  its arena path, which is precisely what the UTF-16 bug broke.
- `ui_press_claim_cycle` — drives the shared press cycle: claim, hold, a
  second widget unable to steal it, release inside (a click), release outside
  (frees the claim, no click — the case that used to wedge `st 0` forever),
  and the overlay `-1` sentinel surviving. The window smoke tests assert a
  frame does not crash; this asserts a click lands.
- `ui_font_height_is_a_cached_constant` — stable across calls, positive,
  bounded by a real glyph measure, and cleared by `ui_font_mono`.

## [0.48.0] - 2026-10-07

Theme: **a stdlib audit, and the three quadratic loops it found** — one of them
a live heap corruption, two of them the `len()`-is-`strlen` trap that runs the
JSON parser at ~33 KB/s on a realistic API response.

Every defect below was REPRODUCED before the fix (compiled Aoxn probes at
`-O0` and `-O3`, peak working set and wall clock measured) and is pinned by the
new `tests/stdlib_defect_pins.rs`. The pins were checked against the
pre-fix sources and DO fail there — the object/array one dies with
`0xC0000005`.

### Fixed: the JSON builder shrank a parsed node's buffer (heap corruption)

- `jb_grow_arr` / `jb_grow_obj` treated a stored capacity of 0 as "give me the
  default" (8). A node the PARSER made adopts a `Vec`'s buffer but leaves that
  capacity at 0, so the first `jb_set`/`jb_push` on it realloc'd the adopted
  buffer **downwards**: a 20-member object (Vec cap 32, 256 bytes) was
  reallocated to 128 bytes and then written at slot 20. Any parsed object or
  array with **16+ members** crashed with `0xC0000005` the first time a field
  was added — which is exactly what `jb_set_raw`/`jb_push_raw` exist to do
  (the Anthropic/OpenAI tool-schema splice pattern).
  Confirmed differentially: 8 members fine, 20 members access violation.
  Both grow functions now floor the capacity at the live child count
  (`jb_grow_cap`), the one quantity that is always known. Growth doubles from
  there, so the store the caller is about to make always lands inside the
  block. The documented promise ("a parsed object can still be extended")
  now holds.

### Fixed: the JSON parser was O(n^2) in document size

- `j_at` tested end-of-input with `len(p.src)`. **`len()` on a string IS
  `strlen`, emitted inline at every use site** (`codegen_c.rs:1973`), so every
  character of every token paid a full scan of the remaining document.
  Measured at `-O3`, doubling the document quadrupled the time: 13 KB 19 ms,
  30 KB 84 ms, 62 KB 343 ms, 126 KB 1421 ms, 266 KB **7064 ms**. The source
  length is a pure function of the immutable `src`, so `JParse` now carries it
  in a new `sn` field, computed once in `j_parse`/`j_parse_into`.
  266 KB: **7064 ms -> 7 ms**.
- `jp_str` allocated `len(p.src) + 1` **per string**. Safe (escapes only
  shrink) but never freed by design, so a 330 KB document with 20 000 strings
  pushed 6.29 GB of allocator traffic through a **325.8 MB peak at ~33 KB/s**.
  The buffer is now sized from the remaining input and grown geometrically;
  the amortized cost is identical and the peak becomes the sum of the actual
  string lengths. Same document: **325.8 MB -> 4.8 MB**.
- `j_num_float` read the exponent with **no digit bound**, so `1e999999999`
  spun the scaling loop for minutes — a reachable DoS on any untrusted JSON
  (an API response, a user config file). The accumulator is now clamped to
  `J_MAX_EXP() = 400` while parsing, which is past the double range, so the
  answer stays the IEEE one (inf / 0.0) and the loop is bounded. Clamping
  during accumulation also closes an i64 wraparound that could return a
  *small* exponent from a huge one.

### Fixed: `j_dumps` was O(n^2) allocations AND O(n^2) leaked bytes

- The serializer built its result by repeated `out = out + ...`. String
  concatenation allocates a fresh buffer per step and abandons the old one, so
  an n-element container cost n(n+1)/2 allocations with every intermediate
  leaked. A 20 000-key response touched **8627.8 MB and 28.4 s**.
  It is now ONE pass into a byte sink that grows geometrically (`JOut`,
  `jw_*`), threaded by value in the same write-back discipline `JParse` and
  `Vec` use — the language has no address-of, so the cursor is handed back
  rather than pointed at. `j_quote` became a thin pre-reserved wrapper over
  `jw_quote`, so the builder still routes through ONE escaping
  implementation. Same document: **8627.8 MB -> 10.2 MB, 28.4 s -> 282 ms**,
  and the round trip stays byte-identical to the input.

### Fixed: base64 decode was O(n^2), and leaked the buffer it rejected

- `b64_decode`'s loop bound was `len(s)` — a fresh strlen of the whole input
  every iteration and every padding byte. Measured at `-O3`: 8 KB 7 ms,
  16 KB 29 ms, 32 KB 138 ms, 64 KB 608 ms, and **1.4 MB did not finish inside
  600 s**. Hoisting one `sn = len(s)` makes it linear; the encode arm always
  took its length as a parameter and was already linear. 2.8 MB: **3 ms**.
- The invalid-input paths returned `B64(p=out, n=-1)` and abandoned the
  buffer, and `b64_decode_str` returned `""` without freeing it. The length of
  a base64 body is attacker-chosen (an `Authorization` header, an SSE line),
  so that leaked on a security boundary. Both paths now `free` and return
  `p=0`.

### Fixed: `os_has_env` read a stale `GetLastError`

- Win32's last error is thread-sticky and a **successful** call does not clear
  it, so any earlier miss — including `os_getenv`'s own probe of an absent
  name — left `ERROR_ENVVAR_NOT_FOUND` (203) latched, and the next probe of a
  variable that really existed answered `False`. The function exists precisely
  to tell "present but empty" from "unset", and it could not. Now primed with
  `SetLastError(0)` (new extern) before the lookup, so the only way out
  holding 203 is a genuine miss.

### Tests

- `tests/stdlib_defect_pins.rs` — 10 checks across the six fixes above. A
  heap-corruption regression surfaces as an abnormal driver exit, so the
  status assert is load-bearing rather than a formality.

### Also measured, deliberately NOT changed

- `stdlib/net/sse.ax` never writes a NUL after the buffered bytes. The feed
  always reserves one spare byte, so the `strlen` inside `str_sub` stops
  inside the allocation: an uninitialized read, not an out-of-bounds one. Two
  probes built to trigger it did not crash. Left as a hardening item.
- `pathlib.ax`'s `path_norm` re-evaluates `len(p)` per byte (the same shape as
  the base64 bug) but measured 0–1 ms at 8 KB — paths are short, so the
  constant is negligible next to the three above. Left alone.

## [0.47.0] - 2026-10-07

Theme: **the UI toolkit finally paints what it draws, and stops paying
for what nobody can see** — a clip stack that actually restores, draws
culled against the current clip, a text-measure cache, and a Windows
backend that remembers what it already told the DC.

### Fixed: the clip stack never restored anything (Windows)

`plat_clip_push` / `plat_clip_pop` kept a rect stack and re-issued
`IntersectClipRect` on pop. GDI's clip only ever NARROWS a DC's region,
so the pop narrowed it further, and nothing ever reset it: **one
`ui_textbox` anywhere in a frame left the whole window clipped to that
textbox rect for the rest of the process** — including the final
`BitBlt`, which copies only the part of the source the source DC's clip
allows. In `examples/ui_gallery.ax` the Editor tab showed the editor and
silently lost the textbox, three labels and the status line drawn after
it. `ui_x11.ax` had always been correct here (its `clip_apply` re-applies
the top rect, and its comment describes this exact Windows failure); the
Windows backend never got the same treatment.

- `ui_win.ax` now saves and restores: one `SaveDC` handle per level (st
  820/821), `RestoreDC` on pop. `SaveDC`/`RestoreDC` were declared since
  v0.2x and never called.
- `RestoreDC` also rolls back the selected objects and colors, so the pop
  drops the DC state cache introduced below — a stale entry would paint
  in the wrong color.

### Added: clip-aware draw culling (both backends)

`ui_draw.ax` keeps its own shadow of the clip stack (st 827..833: stack
ptr, depth, the effective rect in force, and a "canvas known" flag), so
`rect_visible` / `text_visible` can answer "can the platform see this?"
before asking it. Every draw primitive — fill, frame, round, ellipse,
line, text, text_big, text_sub — skips the platform call when the clip
cannot show a pixel of it. Text is culled WITHOUT measuring: a run of n
bytes holds at most n codepoints and none advances more than 64 px at the
toolkit's font sizes, so the bounds are deliberately loose (culling too
little costs a clipped draw; culling too much would drop visible text).

Consequence worth knowing: `ui_clip_push` now hands the backends the
EFFECTIVE (already intersected) rect, which also fixes X11's nested
pushing — its push replaces the clip rather than intersecting it. Use
`ui_clip_push` / `ui_clip_pop`, not the `plat_*` pair: the shadow is what
the culling reads.

### Added: a text-measure cache

Every button, checkbox, radio, tab, menu title, spin arrow, combo item and
selected text range measured its text every frame, and a measure is the
most expensive thing in a widget frame (UTF-16/UTF-8 conversion,
`lstrlenW`, then `GetTextExtentPoint32W` or `XftTextExtentsUtf8` — an X
round trip). `ui_measure` / `ui_measure_sub` now keep a 64-entry
direct-mapped cache (st 834) keyed by (bounded content hash, byte range),
so two equal strings at different addresses share an entry.
`text_pos_in_range` deliberately calls `plat_measure_sub` directly — its
one-codepoint ranges never repeat and would only flush the cache.

### Added: a Windows DC state cache

The backend re-selected both stock objects and both colors on every
primitive. It now remembers what the DC has selected (st 835..840:
brush, pen, their colors, font, text color) and re-selects only on a
change. The drop list is the thing to keep correct: `RestoreDC` on a clip
pop and the bitmap swap in `plat_canvas_resize`.

### Measured (min of five interleaved runs, `QueryPerformanceCounter`,
i5-1135G7, GDI, 1000x700, against the pre-v0.47.0 stdlib built by the
same compiler)

- 3000 px document in a 200 px viewport, 60 labels + 60 buttons: 79 ms ->
  47 ms per 60 frames (**-40 %**) — culling.
- 20 000 measures of one string: 52.8 ms -> **0.55 ms** (96x); the
  uncached `plat_measure` behind the cache is unchanged (52.6 ms).
- 20 000 10x10 fills: 26.7 ms -> 25.1 ms (alternating colors), 24.8 ms ->
  23.0 ms (same color).
- A 100-widget scene with no clipping widgets (identical pixels in both
  builds): ~9 % faster end to end.
- **A heavy scene got SLOWER on purpose: 1.8 ms/frame -> 3.5 ms/frame.**
  The old number came from a renderer that was clipping most of the frame
  away. What remains per frame is `BitBlt` (0.8 ms), the background fill
  (0.08 ms) and one GDI call per visible widget op; `TextOutW` is ~7 us.
  `GetAsyncKeyState`, `PeekMessageW` and `GetClientRect` measured free
  (10 000 calls, 0 ms), which is why the 256-key pump sweep is untouched.

### Tests

- `ui_clip_stack_restores_and_culls` (Windows) draws known rects in known
  colors and reads the canvas back with `GetPixel`: fill, frame border vs
  inside, a widget after a textbox, a nested push/pop pair, a culled
  draw, two fills across a `RestoreDC`, and the measure cache. It fails
  against the pre-v0.47.0 stdlib with seven mismatches, so it is a
  regression test rather than a smoke test.
- Suite: 284 tests green (Windows, clang present).

## [0.46.0] - 2026-10-06

Theme: **Linux is a supported platform again, and the UI toolkit gains a
second backend through the same `plat_*` contract** — plus two toolkit
capabilities that were on the todo list: horizontal scroll areas and a
monospace font.

### Platform: Linux (compiler + stdlib + suite)

- **The compiler builds and runs on Linux** (v0.30.0 had cut the platform
  list to Windows alone). `src/platform.rs` grows a per-OS answer again:
  ELF objects (`.o`), executables without an extension, no `/STACK` link
  flag (the main thread's stack is governed by RLIMIT_STACK), and `-lm`
  added at link time because Linux keeps C math in a separate library.
  `src/setup/` was already cfg-gated, so `aoxn-setup` builds there as a
  stub that refuses to run — the installer stays a Windows artifact.
- **`target_os()` folds the OS the compiler was BUILT on** — `"windows"`
  on a Windows build, `"linux"` on a Linux build, `"other"` elsewhere. The
  self-hosted compiler needs NO new mirror: `selfhost/codegen.ax` folds
  the builtin by calling `target_os()` itself, a value burned in by
  whatever compiler built it, so every stage of the fixed-point chain on
  one machine folds the same host OS and the fixed point stays
  byte-identical by construction (`docs/spec.md` §Platform query).
- **The stdlib branches, it does not fork.** `os.ax` is now dual-platform
  over one file: mkdir/rmdir/unlink/rename/stat/lstat/getenv/setenv/
  unsetenv/getcwd/chdir/opendir/readdir (with the x86-64 glibc struct
  offsets documented in the header) serve Linux while the `W` Win32 calls
  keep serving Windows — an unreferenced `extern def` emits no symbol
  reference, so each platform links only its own side.
  `stdlib.ax`'s `exe_path()` answers through `/proc/self/exe` on Linux and
  `asset_path()` uses the platform separator.
- **`net/http` and the `openai` transport stay Windows-only, honestly.**
  The transport rides WinHTTP and the language has no TLS stack to replace
  it with; the entry points now return a clean "no transport on this
  platform" runtime error on Linux instead of failing to link.
- **CI gains a Linux job** (`ubuntu-latest`): the whole suite, plus
  `examples/ui_probe_x11.ax` driven for 30 real frames under Xvfb — the
  check that the X11 backend LINKS and its event loop honours close. The
  first run caught two things a Windows-only check never sees and both are
  fixed: `aoxn-setup`'s wizard call is now inside a `#[cfg(windows)]` block
  (the console path serves every other platform), and the two SDK test
  crates — which link `-l winhttp` and `-l ws2_32` — are `#![cfg(windows)]`
  gated so they compile to nothing where those libraries do not exist.
  Cross-checking locally: `cargo check --workspace --tests --target
  x86_64-unknown-linux-gnu` (std-only target, no linker needed).

### Platform: the X11 UI backend (rewritten)

- **`stdlib/ui_x11.ax` is back, rewritten against the v3 widget layer**
  (the v0.29-era file was removed in v0.30.0 and spoke the older
  contract). Xlib for the window and input, Xft for antialiased UTF-8
  text, an off-screen Pixmap for double buffering, the same
  `plat_*` primitive set `ui_win.ax` implements — `tests/ui.rs` now pins
  BOTH backends to that set, so a new primitive cannot land in one only.
  Link with `-l X11 -l Xft`.
- Keyboard state is swept with `XQueryKeymap` at the end of each pump
  (plus KeyPress-event bits for click-short presses), so held keys read
  continuously down instead of flickering at half the frame rate between
  X11's release+press autorepeat pairs — the same observable behaviour as
  the Windows backend's per-frame `GetAsyncKeyState` snapshot.
- The clipboard is the X11 selection protocol done properly: we own
  CLIPBOARD, answer `UTF8_STRING` requests, and honour
  `SelectionRequest`/`SelectionNotify` with the out-param-packed
  `XGetWindowProperty` call (12 arguments, four written through
  pointers).
- The close request travels through shared-block slot **824** (Aoxn
  passes `UI` by value, so `plat_close` cannot set a field the app loop
  would see; the old slot 532 is the v0.45 caret cache). Slots 826/827
  are documented in `stdlib/ui.ax`'s header alongside it.

### UI toolkit

- **Horizontal scroll areas**: `ui_scroll_begin_h(c, x, y, w, h, scroll_x,
  content_w) -> int` + `ui_scroll_end_h(c)` — a bottom scrollbar (drag to
  scroll; wheel support is future work) and a clip that excludes the
  scrollbar row. Nests with the vertical variant: the clip stack
  intersects, so begin+begin_h+draw+end+end_h leaves exactly the content
  region. (`docs/todo.md` item 5.)
- **Monospace font support**: `ui_font_mono(c) -> UI` points the widget
  layer's body font at a monospace face (Consolas on Windows, the
  fontconfig `Monospace` alias on Linux); `ui_font_sans(c) -> UI`
  restores the face the backend opened at init (parked in slot 826).
  Both backends create the face at `plat_init` and free it at
  `plat_fini`. (`docs/todo.md` item 5.)

## [0.45.0] - 2026-10-05

Theme: **the UI toolkit gets a performance and correctness pass.** Same
widget set, same immediate-mode design — the fixes are inside the
machinery.

### Fixed

- **Scrolling a combobox/menu popup past 32 items read garbage pointers.**
  The overlay item slots end at `st 815` (32 entries), but the popup record
  stored the *unclamped* item count, so a long list scrolled into the
  tooltip/layout slots and handed `as_string` a bogus pointer. Both widgets
  now clamp the popup to the first 32 items (the documented limit), and
  `overlay_item_str` itself answers `""` for any index outside `784..815`
  instead of dereferencing whatever lives there.

### Performance

- **Text click/drag positioning is O(n), not O(n²).** `text_pos_from_x`
  re-measured `s[0..j)` for every codepoint, and `ui_textedit` carried two
  inline copies of the same loop — a click into a several-thousand-char
  line measured megabytes of text through GDI. The new
  `text_pos_in_range` measures one codepoint at a time and accumulates;
  the textbox and both textedit call sites share it.
- **`tree_has_child` is O(1).** The `TreeModel` block grew a fourth slot
  per node (child count, maintained by `tree_set_parent` including on
  re-parent), so the tree view stops scanning every node per visible row
  per frame — big trees rendered O(n²) before. `tree_has_child` /
  `tree_nth_visible` moved to `ui.ax` where the other model math lives.
- **The caret x-offset is cached again** (st 532..535: owner id / caret /
  length / width) for the textbox and the textedit blink — the docs have
  claimed this cache since v2 and the v3 rewrite dropped it; an idle
  focused field measures nothing per frame again.

### Added

- **Menu keyboard navigation.** An open menu highlights an item (mouse
  hover or Up/Down, both wrap), Enter picks it, Esc closes — the
  "no keyboard navigation inside an open menu" known limit is gone. The
  open menu is modal for input: it consumes the navigation keys and the
  WM_CHAR queue, so a textbox or button drawn after it does not fire on
  the same frame (call `ui_menubar`/`ui_menu` before the page content).

### Changed

- The five copies of the vertical scrollbar (list/tree/table/textedit/
  scroll area) are one `sb_widget` — same hit column (full widget height),
  same thumb math, same claim/drag/release cycle; `ui_scroll_begin` now
  also drags on the press frame like every other scrollbar.
- `ui_fini` frees `msg`/`pt`/`rect` directly (the old `for i in
  range(0, 1)` wrapper is gone).

### Tests

- `ui_v3_portable` pins the O(1) `tree_has_child` (including re-parent
  bookkeeping), `tree_nth_visible` bounds, and the `overlay_item_str`
  out-of-range clamp; the v2 window smoke now drives a 40-item combobox
  (past the popup clamp) through 60 frames.

## [0.44.1] - 2026-10-05

### Fixed

- **`os_getenv` silently returned `""` for any value longer than 2048
  characters.** The v0.44.0 implementation read the variable into a fixed
  buffer and treated the Win32 "buffer too small" answer (the return value
  then carries the required size) as a failure. Real environments hit this
  immediately — the GitHub Actions Windows runners' `PATH` is longer than
  2048 characters, so the os+glob driver's `env-path` check failed there
  while passing on shorter local `PATH`s. The function now follows the
  two-call Win32 pattern (NULL buffer to learn the required size, then
  allocate and read), so values of any length round-trip. Pinned by a new
  `env-long-value` driver check that round-trips a 3000-character value,
  beside the existing `env-path` canary.

## [0.44.0] - 2026-10-05

Theme: **the standard library starts.** The first thirteen modules of the
stdlib roadmap (`docs/stdlib-todo.md` §6, items 1–6 plus the P1
bisect/heapq pair) land as independent files under `stdlib/`, imported by
name — none of them touches `stdlib/stdlib.ax`, so the self-host fixed
point is untouched. Every module ships with a demo (`examples/*_demo.ax`)
and a Rust-side driver test (`tests/*.rs`); the reference for the whole
batch is the new `docs/stdlib.md`.

### Added

- **`stdlib/math.ax`** — the CRT trig/exp/pow externs under Python's
  `math` spellings, and the §0.6 NaN/Inf toolkit the numeric libraries
  were waiting for: `fdiv` (IEEE-correct guarded division — `/` with a
  zero denominator is emitted raw and is C UB), `NaN()`/`Inf()`/`NInf()`
  (the only sanctioned non-finite sources, via `log(-1.0)`/`log(0.0)`),
  and `is_nan`/`is_inf`/`is_fin`. The capitalised spellings are
  load-bearing: a lowercase `nan`/`inf`/`copysign` *definition* collides
  with UCRT symbols at link time (`lld-link: duplicate symbol`), a trap
  documented in the module header.
- **`stdlib/time.ax`** — `time_now_ns`/`time_now_us` (QueryPerformanceCounter,
  with the seconds/remainder split that keeps the i64 conversion from
  overflowing after ~292 days), `time_unix`/`time_unix_ms`/`time_unix_ft`
  (`GetSystemTimeAsFileTime`), `time_mono_ms`, `time_sleep`/`time_sleep_ms`.
- **`stdlib/datetime.ax`** — `DateTime` + Hinnant civil-date conversions
  with `floor_div` guarding the pre-1970/negative-year branches (pinned:
  `datetime_from_unix(-1)` is 1969-12-31 23:59:59), `datetime_now` (local,
  `GetLocalTime`) and `datetime_utcnow`, weekdays (Monday == 0, Python),
  leap years and month lengths, and a hand-parsed `strftime` subset
  (`%Y %y %m %d %H %M %S %j %A %a %B %b %%`) — `wcsftime` is
  locale-dependent and forbidden by the todo.
- **`stdlib/calendar.ax`** — `cal_monthrange`, `cal_weekday`,
  `cal_monthcalendar` (6×7 grid, 0 = out of month) and the text-month
  renderer `cal_month`, Monday-first like Python's default.
- **`stdlib/pathlib.ax`** — pure path string ops (`path_name/stem/suffix/
  parent/join/norm/is_abs/with_suffix/with_name/split`), both separators
  recognized, no filesystem access.
- **`stdlib/base64.ax`** — RFC 4648 encode (standard + URL-safe) AND the
  decode direction the SDK codec never had: `b64_decode -> B64{p, n}`
  (pointer+length, because decoded bytes may contain NUL; `n == -1` marks
  invalid), liberal in exactly one way (both alphabets everywhere),
  rejecting orphan characters and trailing garbage.
- **`stdlib/hashlib.ax`** — SHA-256, SHA-1 and MD5 as pure Aoxn over the
  v0.38.0 bitwise operators, one-shot (`sha256`/`sha1`/`md5` + `_str` +
  `_raw`) and incremental (`*_new/_update/_final/_digest`) forms. Pinned
  by the FIPS 180 / RFC 1321 vectors including the multi-block case.
  Two uint32-on-i64 rules are documented in the header: mask before any
  left shift (a 32-bit value `<< 32` overflows i64) and keep everything
  under 2^32 so `>>` stays logical; the context block reserves **80**
  schedule words because SHA-1 needs them (64 was a heap-corruption bug
  caught by the sha1 vector).
- **`stdlib/hmac.ax`** — RFC 2104 over the `*_raw` one-shots through a
  **function pointer** (v0.40.0); the one module in the batch marked
  `selfhost 尚不可编译`. Long keys are hashed first (RFC 4231 case 6,
  cross-checked against node:crypto); binary keys/messages go through the
  `(ptr, len)` core.
- **`stdlib/os.ax`** — the Win32 filesystem core, all `W` APIs with UTF-8 ↔
  UTF-16 conversion (helpers deliberately duplicated from `net/codec.ax`):
  `os_mkdir/rmdir/remove/copy/rename`, `os_exists/isdir/isfile/islink`
  (the INVALID sentinel compares as 4294967295 — the int-return
  zero-extension trap), `os_getenv/has_env/setenv/unsetenv` (deletion is a
  NULL value, not the empty string), `os_cwd/chdir`, and `os_listdir`.
- **`stdlib/glob.ax`** — a backtracking `glob_match` (`*`, `?`, `[a-z]`,
  `[!a-z]`) and the sorted `glob(pattern) -> Vec` walk over `os_listdir`
  with Python's dotfile rule; `**` stays unsupported (documented).
- **`stdlib/json.ax`** — the JSON DOM promoted from `stdlib/net/json.ax`
  (the OpenAI/Anthropic SDKs import it at the new home; provider-neutral
  all along), gaining the file-I/O pair: `j_read_file` (err 2 = I/O
  failure, 1 = parse error, distinguishable via an fopen probe) and
  `j_write_file`.
- **`stdlib/bisect.ax`** — `bisect_left/right` (insertion positions),
  `insort_left/right` (write-back handles), `bisect_find`, plus the `_f`
  family holding f64 elements in Vec slots through `store_f64`/`load_f64`.
- **`stdlib/heapq.ax`** — a min-heap over an int `Vec`: `heap_push`,
  `heap_pop` (result in the `Heap.res` slot), `heap_peek`, `heap_from_vec`,
  `heap_sorted`.
- **`docs/stdlib.md`** — the batch's reference: per-module APIs, the
  Python-parity deltas, and the traps (UCRT symbol collisions, the 80-word
  schedule, the zero-extended sentinel). `docs/stdlib-todo.md` statuses
  updated to match.

### Changed

- The stdlib resolution docs now describe the promoted `stdlib/json.ax`;
  `stdlib/net/codec.ax` keeps `net_b64_encode_*` (it predates base64.ax
  and stays put). AGENTS.md pointers updated.
- The "Vec[T] 基座" item resolves as a **slot-view discipline** rather
  than a new type: f64 through `store_f64`/`load_f64` views, strings via
  `as_ptr` — the worked examples are the bisect/heapq `_f` families
  (stdlib-todo §0.3 updated). Generic struct containers remain impossible
  (no address-of-struct); heterogeneous records still go through the
  tagged-slab pattern.

## [0.43.0] - 2026-10-05

Theme: **modules stop being a global.** The gap analysis (`docs/language-gaps.md`
B4) ranked "no namespaces" first for a year: every `import` merged into one
namespace, a duplicate top-level name was a hard error, and every stdlib module
hand-prefixed all fifty-odd of its functions because it had no choice. That is
fixed. A module can be bound as a namespace and reached through it, and a named
import finally means what it says.

The design constraint that shaped everything else: **the generated C of a
program that compiled before must not change by one byte.** A name keeps its
bare spelling unless two or more modules provide it, and such a program never
compiled before — so `codegen_c.rs` needed no change at all, the self-hosting
fixed point still holds untouched, and the C-emitting backend keeps being a
pure function of the source.

### Added

- **Module namespaces**: `import util` binds the module, and `util.f(...)`,
  `util.Point` (including construction, `util.Point(x=1, y=2)`, and a
  `util.Point` type annotation) reach across files. Two modules may now both
  define `f`: `import a` / `import b` / `a.f()` / `b.f()` is legal, and the
  only programs that cannot be written are ones that used to be hard errors.
  `import util as u` renames the binding, not its members.
- **The Python import spellings**: `from util import *`,
  `from util import a, b`, `from util import a as b`, `import util as u`.
  A specifier may be written bare and dotted (`stdlib.net.json` is
  `stdlib/net/json`) or quoted (`"stdlib/net/json"`). An unquoted specifier
  also probes the importing file's own directory, so `import util` finds the
  `util.ax` beside it the way Python finds a sibling module; the quoted
  spelling resolves exactly as it always has, untouched.
- **`import "path"` is back.** W1-S3 removed it; it is revived as equivalent
  to `import * from "path"`, which also lets un-migrated sources load.
- **Dotted paths in more places**: `a.b.c.f()`, `a.b.c()`, and `x: a.b.Type`.

### Changed

- **A named import now really filters.** `from util import a` brings in `a`
  and nothing else; referring to `b` is `call to undefined function or struct
  'b'`. Before this the list was parsed and then ignored — the whole module
  merged either way, as `docs/spec.md` used to say ("M1 merges all names").
  This is the one place an existing program can stop compiling: one that wrote
  `import { a } from "m"` and then called something it had not listed. No
  source in this repository does.
- **Star-importing the same name from two modules is a named diagnostic**
  rather than a duplicate-definition error: "name 'f' comes from two modules;
  import the modules and qualify the use, or rename one side with `as`".
  A star import still re-exports, so the diamond case (`a` and `b` both
  star-import `d`) keeps resolving `d`'s names in the program.

### Fixed

- `Aoxn symbols` (and therefore the IDE outline, Ctrl+click and Ctrl+Shift+O)
  names a module's declarations the way its SOURCE spells them. Resolution
  prefixes them when they are contended; an editor must not show the prefix.
- The build-cache dependency scan understands every import spelling, including
  a bare one, so editing a module imported as `import util` can no longer
  serve a stale executable.

### Internal

- `src/resolve.rs` is the new layer: it builds each module's binding table
  and rewrites references to the flat name the whole program shares. The
  loader keeps module identity (canonical path) instead of appending into one
  list, and `scan_imports` now reports whether a specifier was quoted.
- `selfhost/parser.ax` mirrors the new grammar. The self-hosted loader still
  merges modules flat and has no binding layer, so it accepts the forms that
  MEAN a whole-module merge and **refuses** selective imports rather than
  silently merging them — the stage-1 compiler filters, so merging here would
  make stage-2 accept a program stage-1 rejects. Same status as dict / `None`
  / function pointers / `raise`.
## [0.42.0] - 2026-10-04

Theme: **the everyday gaps close.** Four batches of small, high-frequency
language and toolchain work that the gap analysis ranked as P0/P1: the
spelling layer (radix literals, `\r`/`\0`), program I/O (`assert`, `exit`,
command-line arguments), the toolchain's escape hatches (`--clang-arg`, a `cc`
diagnostic stage), and a **warning tier** in the diagnostics contract. No
existing program changes meaning; the generated C for a program that uses none
of the new surface is byte-identical (the self-host fixed point still holds).

### Added

- **Radix integer literals**: `0x`/`0X` hexadecimal and `0b`/`0B` binary.
  Before this, `0x10` lexed as `0` followed by the identifier `x10` and the
  type checker reported `unknown variable 'x10'` — an error that pointed at
  nothing. A malformed literal now names the offending digit (`invalid digit
  'Z' in hexadecimal literal`), a bare `0x` says it needs a digit, and an
  out-of-range value names the maximum in its own radix. Mirrored in
  `selfhost/lexer.ax` (`parse_radix` / `is_radix_digit`) and pinned by a new
  parity test against the Rust lexer.
- **`\r` and `\0` string escapes** (so six: `\n \t \r \0 \\ \"`). `\r` replaces
  the `store_u8(s, i, 13)` dance every HTTP/SSE layer had been doing — the
  AGENTS notes record that trap being hit in three separate sessions. `\0`
  spells a NUL for byte buffers; it does **not** make a string binary-safe
  (`len()` is `strlen`), which the spec now states where the escapes live.
- **`assert(cond)` / `assert(cond, message)`**: a runtime check that prints
  `assertion failed at line N: message` and exits 1. It is not catchable
  (unlike `raise`), does not affect the all-paths-return rule, and bakes the
  source line into the C text. This is the assertion the 46 planned stdlib
  modules had no way to write.
- **`exit(code)`**: terminate with an explicit status. Until now a program
  could only return from `main` or raise (which always exits 1).
- **Command-line arguments: `argc()` and `arg(i)`** — the oldest item on the
  language-gaps list (D1). `argc()` counts the arguments the program was
  invoked with, excluding the program path; `arg(i)` is 0-based and yields `""`
  past the end rather than raising. `Aoxn run app.ax -- a b` now reaches the
  program (the CLI had been forwarding those arguments all along; `int
  main(void)` simply could not read them).
- **`main` signature validation**: `main` takes no parameters and returns
  `int` or `void`. Declaring `def main(argc: int)` used to pass the type
  checker and then die inside clang as an `internal` error; it is now a
  `type` diagnostic that names `argc()`/`arg(i)`. (The TS front end's
  `function main(): number` keeps its own contract and is exempt.)
- **`--clang-arg <flag>` (repeatable) and `-g`**: forward flags to every clang
  invocation, compile and link alike — debug info, `-fsanitize=undefined`,
  `-fno-strict-aliasing`. `AOXN_CLANG_ARGS` is the env-var form. The flags join
  the cache key, so switching them rebuilds instead of reusing a stale exe.
- **`--cc-warnings`** (or `AOXN_CC_WARNINGS=1`): drop the hard-coded `-w` and
  show clang's warnings about the emitted C. A wrong `extern def` signature
  now says so (`incompatible redeclaration of library function 'strlen'`)
  instead of failing mysteriously at the call site.
- **The `cc` diagnostic stage**: a C-compiler failure is `[cc]`, not
  `[internal]` — it is nearly always a user-written `extern def` that
  contradicts a C declaration, and the message keeps clang's own text plus the
  path of the retained `.c` file.
- **A warning tier**: `Diag` carries a `severity` and an optional stable
  `code`; warnings never fail a build. `W001` reports a local that is bound and
  never read. Text mode prints them on stderr; `--json` returns
  `{"ok":…,"errors":[…],"warnings":[…]}` **on success too**, so a caller never
  has to parse human output. The report also gained `severity`/`code` fields.
  Running the new lint over the repository found and removed one real dead
  variable (`stdlib/ui_draw.ax`'s `sel_y`, written twice and never read).

### Changed

- `aoxn check`/`c`/`build`/`run` print collected warnings on success (stderr;
  stdout stays the program's or the C text's). A cache hit re-prints nothing:
  the warnings belong to the compile that produced them.
- `docs/spec.md` gains the literal/escape/builtin/`main`/tooling sections and
  its header finally matches the version. The diagnostics-stage list adds
  `asset` (missing since v0.34.0) and `cc`.
- `crates/aoxn-pkg`'s version moves to 0.42.0: it had drifted to 0.40.0 while
  the compiler reached 0.41.0, which quietly falsified the
  `IndexVersion.aoxn` minimum-compiler-version field it records.

### Fixed

- `def main(argc: int)` no longer reaches clang.

## [0.41.0] - 2026-10-05

Theme: **the Anthropic SDK lands in the stdlib, and the transport becomes
shared.** `stdlib/anthropic/` — three modules, ~1.7k lines of plain Aoxn —
covers the Messages API including **tool use** end to end and its
name-and-payload SSE stream. The four provider-neutral modules that shipped
inside `stdlib/openai/` in v0.40.1 moved up to `stdlib/net/` so both SDKs
ride one transport instead of two.

No language change was needed: everything below is stdlib work.

### Added

- **`stdlib/anthropic/blocks.ax`** — content-block constructors for the
  whole request-side union (text, image base64/url/file, document,
  thinking, redacted_thinking, tool_use, tool_result, server_tool_use,
  web_search_tool_result) plus `AnMsgs`, the message list. A block is a
  **JSON text string**, not a struct: that is what lets the streaming
  accumulator rebuild a Message and have the ordinary accessors read it.
- **`stdlib/anthropic/tools.ax`** — tool definitions, a JSON-Schema builder
  (`an_schema_str/int/num/bool/arr/prop/raw` + `an_schema_required`), the
  four `tool_choice` forms, and the thinking configs.
- **`stdlib/anthropic/client.ax`** — `AnClient` (key / auth token / base
  URL / betas / timeouts / retries, the reference's defaults and env
  variable names), `AnOpts` as the stand-in for fifteen keyword arguments,
  the request/retry core, resources (messages, count_tokens, models, files
  incl. a multipart upload, message batches incl. the absolute
  `results_url`), default-safe response accessors, `an_status_class` /
  `an_err_type` as the reference's exception taxonomy turned into data,
  `AnStream`, and `AnAcc` — the accumulator that reassembles a streamed
  message, **including a tool call whose arguments arrive only as JSON text
  fragments**.
- **`tests/anthropic_sdk.rs`** — a pure driver (headers, blocks, message
  list, schema builder, every body, pagination, multipart, raw-header
  parsing, the JSON builder, the status table, accessors, the named-event
  SSE filter, the accumulator over a canned transcript) and an END-TO-END
  driver against a mock Anthropic server written in Aoxn: message creation,
  a full SSE stream reassembled into a message with a tool call, token
  counting, a model list with a query string, a 429 honored through
  `Retry-After`, and 401/404 bodies.
- **`examples/anthropic_chat.ax`** — a message, a stream, a tool loop, a
  token count and a model list; without a key it prints what is missing and
  exits 0.
- **`docs/anthropic-sdk.md`** — the SDK reference.
- **A JSON builder in `stdlib/net/json.ax`** (`jb_obj`, `jb_arr`, `jb_set`,
  `jb_push`, the typed `jb_set_*` leaves, `jb_set_raw` / `jb_push_raw`).
  Tool schemas and tool inputs are nested structures, and hand-escaping
  them into string literals is how a missing backslash becomes a 400.
  `j_parse_into(dom, src)` parses a fragment into an existing slab, so a
  pasted schema nests with correct child indices and no remapping.

### Changed

- **`stdlib/openai/{codec,json,http,sse}.ax` → `stdlib/net/`**, prefix
  `oa_` → `net_`, types `Oa*` → `Net*`. They were provider-neutral from the
  start; v0.40.1 kept them in the OpenAI directory only because there was
  one SDK. The OpenAI SDK's **public surface is unchanged** — only its
  internal names moved — and `tests/openai_sdk.rs` passes without edits
  beyond the renames.
- **`net_http_request` / `net_http_stream_open` gained a User-Agent
  parameter** so each SDK identifies itself (`aoxn-openai/…`,
  `aoxn-anthropic/…`), the way the Python clients do.
- **SSE frames now carry their `event:` name** (`NetSse.event`). OpenAI
  sends data-only frames and is unaffected; Anthropic names every frame.
  A frame with a name but no data line is dropped rather than dispatched.
- **`net_url_split` no longer discards the query string.** The object name
  `WinHttpOpenRequest` takes includes it, which is the normal WinHTTP
  spelling. Harmless while the only caller was the OpenAI SDK (none of its
  paths carry one); silently wrong the moment a paginated endpoint needed
  it — `an_batches_list(c, 20, "")` would have fetched the whole list.
- **Response headers are read with `WINHTTP_QUERY_RAW_HEADERS` (22), not
  `WINHTTP_QUERY_CUSTOM` (81).** The by-name query returns
  `ERROR_INVALID_PARAMETER` (87) on this platform for every name tried, so
  the transport takes the whole header block once and `net_header_value` /
  `net_header_int` scan it — which also makes the parsing pure and
  offline-testable. `request-id` and `Retry-After` now actually arrive.
- **JSON DOM accessors tolerate a negative index.** `-1` is the module's
  "absent" answer everywhere, and `j_dumps(dom, j_obj_get(dom, i, "input"))`
  is the SDK idiom — unguarded, that read 40 bytes *before* the slab.
- `net_retry_delay` also retries **409**, which the Anthropic client does.
- `net_http_request_bytes` / `net_http_stream_open_bytes` send a body by
  pointer and length. `len()` on an Aoxn string is `strlen`, so a body with
  a NUL byte — every multipart upload of a binary file — cannot travel as
  a string.

### Fixed

- **`Retry-After` is honored by the OpenAI SDK.** v0.40.1 read the header
  into the response record and then always passed 0 to the retry policy.
- A JSON fragment spliced with `jb_set_raw` / `jb_push_raw` used a **stale
  slab pointer**: parsing reallocs, and the old `dom.slab` was then used
  for the next write (heap corruption). Both now continue from `p.dom`.

### Deliberate limits (v0.41.0)

- `temperature` / `top_p` / `top_k` are not sent — the vendored reference is
  a fork whose `messages.create` body does not contain them.
- The `client.beta.*` and organization-admin surfaces are not ported
  (dozens of endpoints each); `an_beta()` sends the header and
  `an_request` reaches the endpoints.

## [0.40.1] - 2026-10-05

Theme: **the OpenAI SDK lands in the stdlib.** `stdlib/openai/` — five
modules, ~2.6k lines of plain Aoxn — gives the language a real API client:
JSON with a proper DOM, HTTPS transport over WinHTTP, SSE streaming, and
the core OpenAI resources, tested end-to-end against a mock server written
in Aoxn itself.

### Added

- **`stdlib/openai/codec.ax`** — base64 (RFC 4648, `=` padding), percent-
  encoding (RFC 3986 unreserved set), and UTF-8 ↔ UTF-16LE conversion.
  The UTF-16 half is deliberately duplicated from `stdlib/ui.ax` so the
  SDK does not pull the UI toolkit (and its 1024-slot heap block) just to
  talk to WinHTTP.
- **`stdlib/openai/json.ax`** — a JSON DOM that fits the language: no dict,
  no `None`, no `sizeof`, no address-of-struct, so values are fixed 40-byte
  nodes in one slab and arrays/objects hold parallel buffers of child slab
  indices (`Vec` buffers adopted at parse time). Recursive-descent parser
  (write-back discipline: `p = jp_value(p)`, extras in `p.res`/`dom.last`)
  with full string escapes incl. surrogate pairs, manual int/float number
  paths, compact serializer, and typed getters with defaults that tolerate
  a failed parse (an empty DOM reads as null — never a NULL deref).
- **`stdlib/openai/http.ax`** — the WinHTTP transport: one-shot requests
  (`oa_http_request`) and streaming ones (`oa_http_stream_open/read/close`),
  URL splitting, the header block, the retry policy table
  (transport/429/5xx, `Retry-After` up to 120 s), and WinHTTP error names.
  WinHTTP's wide strings cross the boundary as raw pointers declared `int`
  — declaring them `string` would promise UTF-8 semantics for UTF-16 data.
- **`stdlib/openai/sse.ax`** — a pull-style SSE filter: `oa_sse_feed`
  appends network bytes, `oa_sse_next` extracts one `data:` payload per
  call (CRLF/LF, `:` comments, multi-line data joined, `[DONE]` sets the
  done flag). Consumed bytes compact to the front with `memmove`
  (`memcpy` would be UB on overlap).
- **`stdlib/openai/client.ax`** — `OaClient` (key, base URL, org/project,
  timeouts, retries — the reference SDK's defaults and env variable names),
  the request/retry core, resources (`chat.completions`, `responses`,
  `embeddings`, `models`, `moderations` — blocking and streaming), response
  accessors (`oa_chat_text`, `oa_usage_*`, `oa_responses_text`,
  `oa_data_*`, `oa_embedding_*`), and `OaStream` (feed → pull loop →
  close; error bodies drained and reported before the loop starts).
- **`tests/openai_sdk.rs`** — two drivers: the pure layers (RFC 4648
  vectors, JSON round-trips/escapes, URL split, headers, retry table, SSE
  incl. split feeds and CRLF, bodies, accessors) and an END-TO-END round
  trip: a mock OpenAI server written in Aoxn (`web/sock_win.ax`, ws2_32)
  spawns on a loopback port and the real WinHTTP path performs a chat
  completion, consumes a full SSE stream and walks a 401 error body.
- **`examples/openai_chat.ax`** — the live demo (blocking + streaming +
  models list); without a key it prints what is missing and exits 0.
- **`docs/openai-sdk.md`** — the SDK reference.

### Changed

- **`str_sub` / `str_remove` / `str_insert` moved from `stdlib/ui.ax` to
  `stdlib/stdlib.ax`** (v0.40.1): the OpenAI client's JSON/SSE layers need
  a substring without pulling the UI toolkit. Caret movement stayed in
  `ui.ax` (a UTF-8 UI concern); `ui.ax` re-exports through its stdlib
  import, so UI sources are unaffected.

### Fixed

- **JSON serializer emits its quotes.** The first `j_quote` escaped the
  content but never wrapped it, so `j_dumps` produced invalid JSON
  (`{model:gpt-4o}`) and any round-trip through the serializer failed.
- **Serializing a failed parse no longer crashes.** An empty DOM
  (`slab == 0`) read as node 0 through a NULL slab — the accessors now
  return null/defaults when the slab is empty.
- **Float formatting without printf.** `snprintf` is variadic; reaching it
  through a fixed-arity extern leaves the float varargs in register slots
  the caller never filled (garbage like `2.47e-323` for `3.25`). `j_fmt_f`
  formats with integer math: 15 significant digits, `%g`-style exponent
  spelling below 1e-15 / above 1e18, trailing zeros trimmed.


## [0.40.0] - 2026-10-04

Theme: **the language grows its fourth data axis.** Function pointers,
nullability, exceptions, and a dictionary — the four things the web platform
work kept reaching for and not finding. Shipped in three phases on one date;
this entry covers all of them.

### Added

- **Function pointers.** A bare function name in value position *is* its
  address (`cb = handler`), `fn(int, string) -> int` annotates the type, and
  `cb(21)` calls through it. `as` re-interprets an `int` address as a callable
  pointer and back — the escape hatch a FFI binding needs (a COM vtable slot,
  a Win32 callback). `to_int(f)` is the raw address; extern defs reject
  fn-pointer parameters. Pointers are ordinary values: they copy, and they
  live in arrays and struct fields (signature-distinct).
- **`None` and `T | None`.** `None` is a keyword and `T | None` is the only
  union form. Widening `T -> T | None` (and `None -> T | None`) is implicit;
  nothing else is. A nullable carries a presence tag; `is None` /
  `is not None` narrow a bare variable per branch, an always-returning branch
  keeps the narrowing, and an assignment inside the branch re-narrows the
  view — in the checker and the code generator alike. Array literals infer
  through it (`[1, None, 3]` is `[int | None; 3]`); `print` and operators
  reject a nullable until it is narrowed.
- **`raise` / `try` / `except`.** The error channel becomes an exception one,
  overriding the v0.39.0 "no exceptions on purpose" decision (user
  instruction, 2026-10-04). `raise <string>` stores the message and unwinds
  to the nearest enclosing handler, or out of the function (`except as e:`
  binds it as a `string`); a raise inside a try **body** is caught by that
  try; an uncaught exception prints one report and exits 1; `raise` counts as
  an exit for the all-paths-return rule. The runtime is two statics (a
  pending flag + the message), a `goto` to the handler label or the frame's
  unwind label, and a slot check after every statement that called a raiser —
  a may-raise fixpoint over call names drives the checks, and indirect calls
  through a function pointer always count. The v0.39.0 `Err`-value channel
  stays: it is how a *library* reports failure to a caller that inspects it;
  `raise` is how a *program* aborts a flow it cannot continue.
- **`dict[V]` — string-keyed maps.** `{"k": v, ...}` literals, `d["k"]` reads
  and writes, `len(d)`, `dict_has(d, k)`, `dict_del(d, k)`, and
  `for k in d` walking keys in insertion order. A missing key **raises**
  through the new error slot, so `dict_has` guards speculative reads and
  try/except catches the rest. Growth is doubling over parallel arrays
  (keys are `char*`), which keeps inserts amortized O(1) while the linear
  scan keeps lookup honest for the config/settings shape a dict is for.

### Fixed

- **`d: dict[int] = {}` — the form the error message recommended did not
  work.** `check_expr` has no expected type to hand an empty dict literal, so
  it errored before the Let's annotation was ever consulted. The one case
  that needs the expected type is resolved in the Let arm; codegen followed
  the same path twice over and gets the binding type instead of inferring
  from a first entry that does not exist.
- **Braces join lines** (v0.40.0): `{"k": v}` spans lines like any other
  bracket, which also gives the TS front end's object literals the standard
  rule.

### The dict is a handle, and why

The first spelling passed the 4-word struct (`keys`, `vals`, `len`, `cap`) **by
value**, and it is unsound twice over: `len`/`cap` live in the copy, so a
callee's growth or deletion was invisible to the caller — and the caller's
stale `len` then walks off the end of a buffer the callee reallocated. It
read fine at `-O0` and faulted at `-O1` and above (verified directly with
clang on the same generated C: correct at `-O0`, a segfault at `-O1`/`-O2`,
a hang at `-O3` — UB in the generated C, not a compiler bug).

v0.40.0 emits the dict as a **heap handle** (`struct ax_dict_T*`), the shape
the UI toolkit's `TableModel` and the web server's `FileTable` already use:
copying the handle shares the dict, mutation through any copy is visible to
all of them, and `set`/`get`/`del` are plain calls with no write-back. The
self-hosted mirror does not emit dicts yet (`selfhost/codegen.ax` has no
`dict`), so the fixed point is unaffected — porting it is future work, along
with the other v0.40.0 features.

### Known wart (pinned, deliberately unfixed this release)

A raise takes effect at the next propagation check, and a statement's check
runs **after** the statement — so `print(f(x))` where `f` raises prints the
unwound frame's zero return value before hopping to the handler. Fixing it
means hoisting raising arguments out of the side-effecting call, in
`codegen_c.rs` and its selfhost mirror; `tests/pipeline.rs` pins the current
behavior with a comment so the change is deliberate when it comes.

## [0.39.2] - 2026-10-03

Theme: **the self-hosting heap corruption is root-caused.** v0.39.0 recorded
it as an open trap — "keep `vec_push`'s growth inline until someone
root-causes this" — and this release does the root-causing, fixes the actual
bug, and hands `vec_push` back its one-line delegation.

### Fixed

- **`struct_cycle` read freed memory (`selfhost/typecheck.ax`).** The struct
  cycle detector threaded its DFS path as a `Vec` **by value**, pushed onto
  it, and recursed. Pushing can realloc, and realloc frees the block that
  every other copy of the handle still points at — so when the callee
  returned, the caller's `path` dangled and the next sibling branch walked
  freed memory (0xC0000374 in the fixed-point build).

  It hid because the inline growth reserved 8 slots up front, so the first
  eight pushes of any path never reallocated and the dangling handle still
  pointed at a live block. `vec_push(v, x) = vec_reserve(v, v.len + 1)`
  grows 1, 2, 4, …, reallocating on nearly every push, which turned the
  latent bug into a certain crash — the delegation was never the cause.

  The fix is the write-back discipline the rest of the self-hosted code
  already follows (`ls = load_file(ls, …)`): the callee returns the live
  handle with its answer, and returns `vec_pop(mine)` so the caller's
  ancestor chain is restored *and* points at the current buffer. Returning
  the extended path instead is wrong in the other direction — the DFS path
  must be the ancestor chain, not every node ever visited, or a node reached
  twice through different branches reads as a cycle (that mistake produced a
  false "recursive struct 'LexState'" until it was caught).

### Changed

- **`vec_push` delegates its growth again**
  (`v = vec_reserve(v, v.len + 1)`), the shape v0.39.0 simplified away to
  dodge the bug. The growth policy now lives in exactly one place
  (`vec_reserve`); small vectors pay a few more `realloc` calls
  (1 → 2 → 4 slots instead of one 8-slot allocation), which is the cost of
  the simplification.

### Documentation

- `docs/spec.md` — the aliased-`Vec` clause gained its missing half. It
  warned about double free; the sharper hazard is **reallocation
  invalidating the other handle**, with the fix stated as a rule: when a
  callee may grow a `Vec` it was handed, the caller must get the handle
  back — which is why a function that pushes and then recurses cannot return
  a bare `int`.
- `AGENTS.md` — the "self-host trap" note is replaced by the root cause.

### Tests

- The fixed-point test (`selfhost_driver_self_compiles`) is the regression
  guard: it fails with a heap abort under the delegating form before the
  fix and passes after. Root suite 228/228, aoxn-pkg 94/94.
- Cycle detection verified to still reject a real recursive struct on both
  compilers (`aoxn check` and the self-hosted checker).

## [0.39.1] - 2026-10-03

Theme: **catching the tooling up to the language** — the IDE's editor now
knows the v0.38.0 operators and the v0.39.0 stdlib vocabulary, and the
installer fixes that landed between releases get their version entry.

### Fixed
- **The IDE highlighted the language of two versions ago**
  (`ide/lib/monaco.ts`). The Monarch grammar still carried "the language has
  no bitwise operators, a `|` is only ever a malformed `or`" — stale since
  v0.38.0 added `& | ^ ~ << >>` — so none of them were tokenized as
  operators, and the doubled forms had to move ahead of the
  single-character class to match at all. The builtin list also gains the
  v0.39.0 stdlib vocabulary: the `err_*` / `out_*` error-channel idiom, the
  `vec_*` / `vecvec_*` container functions, and the `ERR_*` codes, so the
  documented failure pattern reads like `print` instead of plain text.
  `AGENTS.md`'s UI-toolkit section said the same stale sentence and is
  corrected the same way.
- **The installer window aborted about a second after it appeared**
  (`src/setup/ui.rs`). Each owner-drawn control was created and only then had
  its id written with `SetWindowLongPtrW` — but Win32 delivers `WM_PAINT` to a
  visible child while `CreateWindowExW` is still on the stack, so the handler
  read an id of 0, `slot(0)` underflowed to a huge index, and the panic fired
  inside `CallWindowProcW`, where a panic cannot unwind: the process aborted.
  The id now travels in `lpParam` and is claimed in `WM_NCCREATE` (the first
  message a window receives), and `slot()` is total — an unexpected id returns
  `None` instead of underflowing.

- **A console install never exited.** The window's start closure borrowed the
  channel's sender and outlived the console branch, so the channel never
  closed and `rx.recv()` blocked forever after the worker finished: the
  install completed, printed its last line, and hung. The console path now
  spawns the worker directly and drops its own sender, which is what lets
  `recv()` observe the close. Found by driving the failure path end to end.

- **The failure page showed one line of the error.** It kept only the last
  non-empty log line in a fixed-height box, so an error that spanned several
  lines — the antivirus message names a cause and a remedy across five — lost
  everything but a fragment. The page now prints the whole log, measures the
  text and grows the window to fit it.

## [0.39.0] - 2026-10-03

Theme: **the two things a language needs before it can hold real data** —
a way to report failure, and a way to hold "however many" of something.
Neither required touching the compiler; both are stdlib plus a documented
convention.

### Added

- **The error channel (Tier 0.2).** Aoxn has no exceptions, and this is the
  replacement: a failure is a *value*. A function that can fail returns an
  `Err{code, message}`; the caller tests it with `err_is_ok`. Nothing unwinds,
  nothing is caught.

  The interesting part is the payload. A function returns exactly one value and
  structs are copied on return, so a result **cannot** ride along with the
  error — mutating a struct field inside a callee is discarded by the caller.
  The value therefore comes back through an **out-slot the caller owns**:

  ```
  p = out_new()
  e = parse_port("8080", p)
  if err_is_ok(e):
      port = out_get_i(p)
  out_free(p)
  ```

  That is the only shape that survives value semantics, and it is now the
  documented pattern rather than something every author re-derives. A call
  with no payload just returns the `Err` — that is the whole "Result carrying
  no value" case and needs no slot.

  The stdlib ships the vocabulary: `err_ok` / `err_new` / `err_is_ok` /
  `err_is_err` / `err_code_name` / `err_or_int` / `err_or_str`, the standard
  codes `ERR_NONE` … `ERR_AGAIN`, and `out_new` / `out_set_i` / `out_get_i`
  with float and string siblings. The codes are zero-argument functions
  because Aoxn has no module-level bindings.

- **Variable-length containers (Tier 0.3).** `[T; N]` is fixed at compile
  time, so anything that must hold "however many" items needed the heap.
  `Vec` grows properly now (`vec_reserve` / `vec_truncate` / `vec_clear` /
  `vec_len` / `vec_cap` / `vec_last` / `vec_index_of`).

  **Strings are elements now** — `vec_push_str` / `vec_get_str` /
  `vec_set_str` / `vec_index_of_str` / `vec_contains_str`. A `string` *is* a
  pointer, so this is a slot rather than a copy, and the bytes behind it are
  immutable, which is what makes it safe. It removes the `as_ptr` /
  `as_string` dance from every call site that was storing strings in a `Vec`.

  **`VecVec`** is the case that motivated the whole exercise: a vector whose
  elements are themselves vectors, each stored as three consecutive slots
  (data, len, cap). That is the shape a document format needs — a JSON array
  of arrays, a table of variable-length rows — and nesting it again gives
  arbitrary depth with no new machinery.

### Notes

- **No compiler change.** Both items are stdlib plus a documented convention.
  The compiler does not know `Err` exists, and a program may use any struct of
  the same shape.

- **What containers deliberately do not give you.** There is no `sizeof` and
  no address-of-struct (`as_ptr` works on a `string` only), so a container can
  only move 8-byte slots. "A list of `Point`" must be a list of slot-tuples or
  a vector of pointers to individually allocated records. That cost is now
  written down in `docs/spec.md` instead of being discovered later.

- **Aliased `Vec` handles are a sharp edge, now documented.** `vec_truncate`
  and `vec_clear` return a new value over the *same* buffer, so after
  `short = vec_clear(v)` the two are two handles on one allocation. Pushing to
  either is fine and invisible to the other; **freeing both is a double free**
  that corrupts the heap somewhere unrelated later. This was found by a test
  that did exactly that, which is the best possible way to find it.

- **A self-hosting trap, documented rather than root-caused.** The self-hosted
  compiler heap-corrupts compiling a `stdlib.ax` in which `vec_push` delegates
  its growth to another function — verified with the callee defined before and
  after the caller, and a plain forward reference on ints works fine, so it is
  this delegation in the stdlib context specifically. The Rust compiler
  accepts it; only the self-hosted one breaks, and the fixed-point test is
  what catches it. `vec_push` keeps its growth inline until someone
  root-causes this. The note lives in AGENTS.md so it is not reintroduced
  blind.

Tests: `stdlib_error_channel_and_out_slots`,
`stdlib_variable_length_containers` (and the pre-existing
`stdlib_vec_grow_and_slots`, which covers the `vec_push` refactor).

## [0.38.0] - 2026-10-03

Theme: **Aoxn grows the six operators it was missing** — bitwise and shift.
They are the first language change in a while that is not about a new front
end or a new asset type, and they exist because a from-scratch rewrite of the
OpenAI agents SDK needs them everywhere.

### Added

- **Bitwise and shift operators: `&`, `|`, `^`, `~`, `<<`, `>>`.** Aoxn had
  none of them. SHA-256 round functions, the TLS 1.3 key schedule, HTTP/2
  frame fields, HPACK varint decoding, UTF-8 continuation bytes and
  byte-buffer slicing all need them, and without them the only way to express
  any of that is arithmetic on masks — slow and unreadable.

  They are **int-only**, exactly like `%`. Aoxn has no implicit int/float
  conversion and this adds none: promoting a mask to `double` silently would
  defeat the purpose. `&` on a `float` is a type error, as is `~` on a
  `string`.

  A single `&` or `|` is now the bitwise operator rather than a lex error
  that suggested `&&`. `&&` and `||` are unchanged.

  Precedence is C's, and the one genuinely surprising part is now pinned by a
  test: **in C `==` binds TIGHTER than `&`**, so `a & b == c` groups as
  `a & (b == c)`. That is why every real codebase writes `(a & b) == c`, and
  why `x & 0xFF == 0` is a famous C bug. Aoxn inherits it on purpose.

  There are **no augmented bitwise forms** (`&=`, `<<=`, …); write
  `x = x & y`. The existing `+= -= *= /= %=` family is unchanged.

  `docs/spec.md` carries the updated grammar; the self-hosted compiler
  mirrors all five sites so the fixed point holds.

### Notes

- Nothing was broken — this is an additive change. The full suite is
  226 passed / 0 failed, and the one test that failed on the way here was
  mine, not the compiler's: it asserted `&` binds tighter than `==`, which is
  backwards.

## [0.37.0] - 2026-10-03

Theme: **the installer is rewritten around the Python installer's shape** —
three phases, one primary button, nothing on disk until you click — and it
stops downloading things behind your back.

### Added

- **The installer window is rebuilt** (`src/setup/ui.rs`, ~1100 lines, still
  zero external crates). The old one was a single screen: a progress bar, a
  scrolling log, an "Installing…" button that started disabled, and a Cancel
  beside it. The new one has the structure people already know from the Python
  installer for Windows:
  - **Welcome** — mark, headline, one large primary button ("Install Now") over
    a secondary one ("Customize installation"), and the options as checkboxes.
  - **Progress** — the current stage, one bar, and Cancel in the same place the
    primary button was.
  - **Done / Failed** — what happened, where it went, and Close.
  The primary button is *reused* across phases rather than swapped, so the eye
  stays in one place. Everything is owner-drawn in GDI (flat buttons with
  hover/press/focus, checkboxes, the bar) — standard controls cannot produce
  that look without a theme and a manifest, and a themed progress bar looks
  like nothing else.

- **`-InstallClang`** downloads LLVM via winget when no clang is found.

### Fixed

- **The install started before the window did.** The worker thread was spawned
  next to the channel, so the toolchain was unpacked into the user's disk while
  the window was still being created — with nothing on screen to explain it.
  `main.rs` now hands the UI a closure and the worker starts when the user
  clicks. Console mode (`-Console`, CI) still starts at once, since there is
  nobody to click.

- **The installer downloaded LLVM behind a progress bar that said nothing
  about it.** On a machine with no clang on PATH it ran `winget install
  LLVM.LLVM`, turning a ten-second install into a multi-minute one that reads
  as *hung*. The installer now **downloads nothing by default**: it unpacks,
  sets PATH, and runs `aoxn doctor`, which names anything missing. The old
  behavior is one flag away. CI gets faster and more deterministic for the same
  reason.

- **A use-after-free in class registration.** `WNDCLASSEXW::lpszClassName` was
  pointed at a temporary `Vec<u16>` that died at the end of the statement,
  while Win32 keeps the pointer for the lifetime of the class. The names are
  now three leaked UTF-16 buffers.

- **`src/setup/main.rs` contained a literal NUL byte** (the payload magic), so
  every tool that sniffs encoding — grep, ripgrep, `git diff` — treated the
  file as binary. The magic is now `b"AOXNSFX\x00"`, an escape rather than a
  byte, and the file reads as text again.

- **Closing the window mid-install orphaned a worker** still unpacking into the
  user's disk; closing now cancels.

### Changed

- **Editing the user's PATH is a choice.** It was step 3, unconditional. The
  window offers it as a checkbox (on by default), and `install()` honours both
  it and the clang download rather than assuming.
- `InstallOptions { prefix, add_to_path, install_clang }` is now shared between
  `main.rs` and the window, so the checkboxes and the command line drive the
  same code path.
- `-NoClang` is the default now; it stays for compatibility.

### Tests

- Root **222**, aoxn-pkg 94: unchanged and green, including the three
  antivirus-race tests around `confirm_present`.
- Verified end to end on this machine: `dist/package.ps1` -> 4.18 MB Setup.exe
  -> `-Console -Prefix ...` -> 23 files installed, clang found, `aoxn doctor`
  status ok, smoke test ok -> `aoxn run examples/stdlib_demo.ax` produces the
  expected output from the *installed* toolchain.

## [0.36.0] - 2026-10-03

Theme: **the CSS pipeline finishes what v0.34.0 started** — assets on disk,
`url()` rewriting, `styles.title`, CSS-in-Aoxn, and a built-in Tailwind
generator — plus a fix for a TS import form that never worked.

### Added

- **`--emit-assets <dir>`** (`src/main.rs`) writes fingerprinted CSS and every
  `url()` target to disk, so a server can serve them over `<link>` instead of
  inlining. A relative path resolves against the *output executable's*
  directory, so `aoxn build app.ax --emit-assets assets` means the same thing
  from any cwd. Emitted beside the executable, never into the build cache:
  `prune_cache` walks a flat directory and only removes 16-hex-named entries,
  so anything written there would leak forever. Runs on the cache-hit path too
  — the cached executable is byte-identical, but the output directory is not
  something the cache knows about.

- **`url(...)` rewriting** (`src/assets.rs`). A local target is fingerprinted
  (`logo.png` -> `logo.0d6b17e8.png`), emitted next to the stylesheet, and the
  CSS is rewritten to the new name. Left untouched: `data:` URIs, absolute
  URLs, protocol-relative URLs, and `#fragment` references. A target that does
  not resolve **passes through with a warning** rather than failing — a
  stylesheet may legitimately reference a CDN font or a file another tool
  copies in, and erroring would reject valid CSS over an asset the compiler
  was never asked to manage.

- **`styles.title` dot access.** `import styles from "./page.module.css"` now
  binds `styles` to a synthesized struct with one field per class, so
  `styles.title` is a typed string read. Aoxn has no function pointers, so a
  namespace object was impossible; the binding is rewritten at the use site
  into a call on the generated accessor, and the struct type
  (`<stem>_Classes`) is distinct from the accessor function (`<stem>_module`)
  because the two share one namespace.

- **`exe_path()` / `exe_dir()` / `asset_path()`** (`stdlib/stdlib.ax`) — a
  compiled program can finally learn where it lives. `main` takes no arguments
  and the language has no argv/cwd builtin, so `GetModuleFileNameA` (already
  reachable through `extern def`, kernel32 being linked via the CRT) does it.
  `asset_path(styles_fingerprint())` is the exact file `--emit-assets` wrote.
  Deriving the path at runtime rather than baking it in keeps the executable
  relocatable.

- **CSS-in-Aoxn** (`css_decl_1/2/3`) — a checked inline-`style` builder for
  values computed at run time. A value containing a quote is dropped rather
  than escaped, so a broken attribute cannot be produced silently.

- **`--tailwind`** (`src/tailwind.rs`) generates a documented utility subset
  by scanning `class`/`className` attributes. Only a class attribute is mined:
  an earlier "any string shaped like a class list" heuristic was tried and
  rejected, because prose such as `"the red-500 of it is p-4"` passes the same
  shape test a real attribute does and silently generated CSS. Unsupported
  utilities are reported by name at build time rather than dropped.

- **`symbols`** (`src/symbols.rs`) and **`aoxn check`** — already in 0.35.0;
  unchanged here.

### Fixed

- **The TS front end rejected every `import * from "p"`.** The arm meant to
  handle the whole-module merge returned an error on seeing `*`, and its own
  message recommended the very form it refused — so no `.ts` file could import
  anything at all, including a stylesheet. `import * as ns` still needs
  namespace objects and still errors; the two are now told apart by peeking
  one token further.

- **`styles_fingerprint()` returned a name nothing wrote.** It appended
  `.css` to a value that already ended in it, and the emitted bundle is named
  without a stem. A `<link href>` built from it could not have matched any
  file. `bundle_fingerprint()` is now the literal emitted name, and a test
  asserts the two agree.

- **An empty asset set emitted a stray file.** A program importing nothing
  dropped a zero-byte `<hash>.css` into the asset directory.

- **`src/setup/ui.rs` imported `InstallStep` without using it** — a dead import
  that made every release build emit a warning.

### Notes

- **Assets still enter codegen as ordinary `FnDecl`s**, so `codegen_c.rs` has
  no asset dispatch, `selfhost/codegen.ax` needs no mirror, and the bootstrap
  fixed point holds by construction. That is why this release could add an
  emitted directory, a JIT generator and a stdlib extension without touching
  the C emitter.

- **Emitted names are compiler-generated** (`<stem>.<16 hex>.<ext>`) and are
  re-checked at write time against a plain-name predicate. A stylesheet can
  influence the *text* of a generated string constant and nothing else — never
  a path the compiler writes to.

- **Not implemented**: JSX, `import * as ns` namespace objects (TS-M2),
  `url()` resolution for protocol-relative or cross-origin assets, Tailwind
  config files, plugins, arbitrary values (`bg-[#abc]`), and the full Tailwind
  palette. The generator is a subset by design and says so.

### Tests

- Root **222** (lib 17 + assets 21 + **assets_v36 18** + install 6 + pipeline
  106 + symbols 8 + ts_lex 14 + ts_parse 20 + ui 9 + setup 3), aoxn-pkg 94.
- `selfhost_driver_self_compiles` and `c_text_is_opt_level_independent` green.
- Four bugs the new tests caught during development: side-prefixed utilities
  (`px-4`) never matched, prose was mined for class names, the empty asset set
  emitted a file, and a missing `url()` target failed the build instead of
  warning.

## [0.35.0] - 2026-10-03

Theme: **the IDE learns the language**. The compiler already knew where
every function was declared and which file it came from, and already
emitted machine-readable diagnostics; the IDE was recovering both by
scanning text with a regex. This version has the compiler export its
symbol table, has the IDE ask for it, and has the packages panel read the
lockfile instead of the manifest's wishes.

### Added
- **`aoxn symbols <file.ax> [--json]`** (`src/symbols.rs`) — the
  language-service export. Walks the AST `load_program` just built and
  reports every top-level `def` and `struct` with parameter types, return
  types, `extern` marking, and a source range, across the entry file AND
  every module it imports. Runs the loader and the parser and stops —
  **no typecheck, no codegen, no clang** — because an editor re-asks this
  on every file switch and a check that shells out to a C compiler would
  be far too slow. Scope is deliberately top-level only: a local lives
  inside a body and means nothing outside it. A file that does not parse
  exits 1 with a diagnostic rather than returning half an outline, since
  a partial list sends the reader to a declaration that is not there.
  Zero external dependencies, like the rest of `src/`.
- **Structured diagnostics in the IDE** — `check`/`build`/`run` now run
  with `--json` and `ExecResult` carries `diags: Vec<DiagInfo>` BESIDE the
  raw `output`. The editor's markers, the problem count and the status bar
  read the structured side, where every field is exact; the output panel
  still shows the compiler's own words verbatim, including clang's and a
  program's own prints. `lib/diagnostics.ts` keeps its text scanner as the
  fallback for an older compiler or a command that died before printing.
- **Outline panel** (v0.35.0) — the current file's declarations in source
  order, one click to any of them, built from `aoxn symbols`. The table
  spans the whole program, so the outline filters to the current file:
  drawing all of it would put the stdlib's several hundred declarations
  under every file the reader opens.
- **Go to definition** — Ctrl+click resolves the identifier under the
  caret against the same symbol table and opens its declaration **across
  an `import`**. An `extern def` is a valid target (declared here,
  implemented in C). A local is not — Aoxn exports no position for one —
  and the status bar says so instead of doing nothing. This handler was an
  empty function from v0.31.0 until now.
- **Symbol search** — Ctrl+Shift+O, ranked exact → prefix → word-initials
  → substring, case-insensitive, with the origin `file:line` beside each
  row. Ties break deterministically, because a list that reshuffles
  between two identical keystrokes makes the arrow keys lie about what
  Enter will open. Searching from a file finds its imports' declarations.
- **Resolved package versions in the IDE** — `ide_pkg_report` runs
  `aoxn pkg list --json` and `aoxn pkg outdated --json` (both already on
  the whitelist, both read-only — this widens what the panel can SHOW,
  not what it can DO) and the panel shows the **resolved** version next
  to the manifest's requirement, with its scope and source registry and
  an `old → new` marker for anything behind. Neither report is fatal: a
  missing lockfile or an unreachable registry costs one section and is
  reported, so a failed `outdated` never reads as "everything is
  current".

### Fixed
- **A successful install could leave no compiler to use.** Windows Smart
  App Control and Defender routinely quarantine a freshly written, unsigned
  executable AFTER its first run, and `aoxn-setup` unpacks exactly such a
  binary. The installer ran `aoxn doctor` (which succeeded, smoke test and
  all), reported **Done**, and never looked again — so the very next command
  failed with PowerShell's `"The term '…\bin\aoxn.exe' is not recognized as
  a name of a cmdlet"`, an error that blames the shell and names neither the
  cause nor the remedy. `confirm_present` now re-checks the binary after the
  doctor run, waits out a short antivirus hold, and if the file is genuinely
  gone it fails the install with the reason (antivirus / Smart App Control)
  and the fix (exclude `%LOCALAPPDATA%\aoxn` from real-time scanning).
  `release.yml` checks the path *before* invoking it and says the same thing,
  so the CI smoke test reports the real problem instead of a shell error.
  Three tests cover both directions of the race — present, absent, and
  appearing during the wait — and run in milliseconds because the retry
  budget is a parameter.
- **One spelling for every path** — `load_file` registered the path it was
  *called with* while import resolution canonicalized, so the same file
  appeared as `stdlib\ui.ax` from one route and `\\?\D:\...\stdlib\ui.ax`
  from another. A diagnostic about an imported file and a symbol exported
  from it could therefore not be matched against each other, which is what
  made cross-file navigation impossible. The registry now takes the
  canonical path with the verbatim prefix stripped (`paths::strip_verbatim`
  promoted from private to shared).
- **Ctrl+Shift+P never opened the command palette.** The `mod && key ===
  'p'` test also matches Ctrl+Shift+P — `event.key` is `'P'` there and
  lowercasing makes it indistinguishable — so the plain test came first
  and the command palette was unreachable since v0.31.0. Shift variants
  are now tested first.
- **`extern def` signatures dropped their return type** in the symbol
  export: `extern def GetTickCount()` read as returning nothing when the
  AST said `int`. A caller reading an outline needs to know the shape of
  the call, and an extern that returns `int` is exactly that case.

### Tests
- Root 204 (was 172): 11 unit tests for the symbol extraction (through
  the real loader, with real imports and a file that does not parse), 8
  CLI tests driving the built binary — the command line, the exit code
  and the JSON shape an editor consumes, which a unit test on `collect`
  would pass while the command printed something unusable — 3 for the
  installer's post-doctor binary check, and 10 for the path spelling.
- IDE Rust 36 (was 22): JSON extraction from the merged stream including
  braces and escaped quotes inside a message, a top-level array (what
  `outdated --json` emits), pretty-printed JSON, an unclosed document, the
  package-report parsers against malformed input, and a pin that the
  report readers only ever shell out to whitelisted read-only
  subcommands.
- IDE frontend 62 (was 26): 21 for the language service (outline scoping,
  jump preference, ranking order, path normalization), 9 for the package
  report, 6 for the structured diagnostics. The ranking and jump logic
  lives in pure functions precisely so it is testable without a render.
- Verified in a browser against `pnpm dev`: the outline lists a fixture
  file's two declarations, symbol search finds `clamp_i` in `util.ax`
  while `main.ax` is on screen (the cross-file case), and the packages
  panel renders `1.4.0 → 1.6.0`. `pnpm build` clean.

## [0.34.0] - 2026-10-03

Theme: **`.css` becomes a first-class build input** — decision B1 phase 1 of
[`docs/web-platform-plan.md`](docs/web-platform-plan.md) §7, with CSS Modules,
and Tailwind entering as pre-generated CSS rather than as a dependency.

### Added

- **CSS asset pipeline** (`src/assets.rs`, zero external crates). A `.css` file
  reached through `import` is now a build asset instead of source. `@import
  "x.css";` is inlined recursively in place (order preserved, so the cascade
  matches a browser's; a cycle is an error, a repeat contributes once),
  comments and redundant whitespace are removed, and the result is
  fingerprinted. Two functions are generated: `styles() -> string` (the whole
  bundle) and `styles_fingerprint() -> string` (a `<16 hex>.css` name for
  `<link>` cache-busting). Reference: [`docs/css-assets.md`](docs/css-assets.md).

- **CSS Modules** — a `*.module.css` file has its class names hashed (seeded by
  the file's own path, so the same name in two modules differs) and generates
  `<stem>_class(name: string) -> string`. A module's rules are deliberately
  **not** joined into the global bundle; that is what makes the scoping mean
  anything.

- **`asset` diagnostic stage** for stylesheet problems, carrying the `.css`
  file's own name and the line of the offending `@import`, e.g.
  `[asset] style.css:3:1: @import './missing.css' does not resolve to a file`.

- **Tests** (`tests/assets.rs`, 21): `@import` inlining order, cycle and
  missing-import rejection, non-CSS `url()` pass-through, minifier idempotence
  and its whitespace rules, class hashing, build-cache participation, and
  end-to-end `styles()` / `<stem>_class()` runs.

### Fixed

- **A `.css` import used to fail with a misleading diagnostic.** Module
  resolution's exact-path short-circuit (`complete_module_path`, `src/lib.rs`)
  accepted `./styles.css`, and the file was then handed to the **Aoxn lexer**,
  which reported `unexpected character '{'` — nothing in that message pointed
  at the real problem. `load_file` now diverts `.css` to the asset pipeline
  before any front end sees it.

- **A `.css` edit did not invalidate the build cache.** `dependency_files`
  (`src/lib.rs`) walked only `import` declarations, so a stylesheet was
  invisible to `cache_key` (`src/main.rs`) and an edited `.css` silently served
  a stale executable. Stylesheets and their `@import`ed partials are now part
  of the dependency set, hashed like any other input.

### Notes

- **Minification is deliberately conservative**: comments and whitespace only —
  no selector merging, no rule reordering, no dropping the final `;`, no empty
  rule elision, no color shortening. Each can change meaning in some CSS corner
  and the payoff is cosmetic, so the emitted text stays a pure function of the
  source. This mirrors the `c_text_is_opt_level_independent` discipline. A
  comment counts as whitespace, so `a/**/b` never collapses into `ab`.

- **Class rewriting is context-aware.** A `.` inside a string literal
  (`content: ".x"`), inside an `@media` prelude (`(min-width: 30rem)`), or
  inside a declaration value (`1.5rem`) is not a class selector. A naive text
  replacement gets all three wrong and silently changes rendering; each case is
  pinned by a test.

- **Assets enter codegen as ordinary `FnDecl`s**, with compile-time string
  bodies, reusing the `TS_RUNTIME_SRC` injection technique. `src/codegen_c.rs`
  is therefore untouched and `selfhost/codegen.ax` needs no mirror of the
  asset pipeline — the self-hosted fixed point
  (`selfhost_driver_self_compiles`) is unaffected by construction, not by
  testing luck.

- **Tailwind is integrated as pre-generated CSS, deliberately.** Tailwind is a
  plain-JavaScript npm package with no `aoxn.json`, and `aoxn npm-import`
  rejects those by design (`crates/aoxn-pkg/src/npm.rs`, pinned by
  `plain_js_package_is_rejected`) — Aoxn links Aoxn packages, not JavaScript.
  Shelling out to the CLI the way the compiler shells out to clang would make
  Node a build prerequisite, which the one-click Windows install avoids. Run
  `npx tailwindcss -o generated.css` yourself and import the result.

- **Not implemented, on purpose**: a Tailwind JIT scanner (a much larger piece
  of work; a "Tailwind-compatible subset" would be a long maintenance
  liability), an output directory for fingerprinted files (the compiler passes
  no argv/cwd/environment to the program it builds, so there is no channel for
  a program to locate an asset directory), `url(...)` rewriting, CSS-in-TS, and
  `styles.title` dot access (that one needs namespace objects, i.e. TS-M2).

### Tests

- Root **182** (lib 6 + assets 21 + install 6 + pipeline 106 + ts_lex 14 +
  ts_parse 20 + ui 9), aoxn-pkg 94: all pass.
- `selfhost_driver_self_compiles` and `c_text_is_opt_level_independent` both
  green — the direct evidence that the asset pipeline does not disturb the
  fixed point or the level-independent C.

## [0.33.0] - 2026-10-02

Theme: **the IDE learns the package manager** — a Packages view wired to the
real `aoxn pkg` — plus the fix for the CI smoke test that v0.30.0's second
binary silently broke.

### Added
- **Packages panel in the IDE** (`ide/src-tauri/src/pkg.rs` + a sidebar
  view): the workspace's `aoxn.json` (name/version, dependencies and
  devDependencies, `aoxn.lock` presence) alongside what is unpacked in
  `aox_modules/`, with buttons for the whitelisted verbs — Init, Add,
  Install, Update, Outdated, Tree, Audit, Why, Remove. Every command runs
  the real `aoxn pkg` from the opened folder and its output reaches the
  output panel verbatim; mutating subcommands re-read the manifest and the
  explorer when they finish. **The whitelist is the security boundary**:
  publish, yank, cache, trust bootstrap and npm-import are refused in Rust
  (`pkg::ALLOWED_SUBCOMMANDS`), never merely hidden in the UI, and package
  names typed into the panel are validated on both sides (no flag-shaped
  strings, paths, or whitespace reach clap).
- **`aoxn doctor` in the IDE** (`ide_doctor`) — the status bar's "compiler
  not found" (and a new "clang not found") button run the toolchain's own
  self-check into the output panel instead of just pointing at config docs.
- The browser-mode fixture ships an `aoxn.json` + `aoxn.lock`, so the
  packages panel has something real to show in `pnpm dev`.

### Fixed
- **The CI smoke test** ("could not determine which binary to run"):
  v0.30.0 added the `aoxn-setup` bin, after which bare
  `cargo run -- run examples\hello.ax` had no default binary to pick and
  every documented command of that shape failed — the smoke test's
  `.\primes.exe` was only the visible symptom (it had never been built).
  `default-run = "aoxn"` in the root manifest restores the bare form
  everywhere (README, AGENTS.md, `web-bench.yml` included).

### Tests
- IDE Rust 22 (`cargo test --manifest-path ide/src-tauri/Cargo.toml`): six
  new tests pin the pkg surface — the whitelist (publish/yank/cache and
  friends refused), mutating-vs-readonly classification, package-name
  validation (flag/path/whitespace shapes refused), tolerant manifest
  reading (missing, broken, full), and `aox_modules/` listing.
- IDE frontend 26 (`pnpm --dir ide test`): five new tests for
  `lib/pkg.ts` — manifest normalization against junk, dependency
  formatting, and the TS name validator that mirrors the Rust one.
- Root 161, aoxn-pkg 94: unchanged pass.

## [0.32.0] - 2026-10-02

Theme: **the package manager, measured against pip and pnpm**. Aoxn had
a compiler, an IDE, a UI toolkit and a package manager — but the package
manager was the one you could not run a real project through, because
`devDependencies` was not an unsupported feature: it was a parse error.
This is that gap closed, plus five bugs found while measuring against the
tools it is meant to stand next to.

### Package manager — measured against pip and pnpm

The package manager had the right bones (PubGrub, a content-addressed
cache, three registry backends) and a real hole where a day-to-day feature
should have been: `devDependencies` was not merely unsupported, it was a
parse error. That and the bugs found along the way are this half of the
release. Reference layout for the curated registry:
[`docs/trusted-registry.md`](docs/trusted-registry.md).

#### Fixed
- **The HTTP registry backend could not download anything.** It verified
  `sha256(received bytes)` against `checksum` — but `checksum` is the
  *manifest hash* over the unpacked file tree, a completely different
  quantity. Every download failed with a bogus integrity error. The index
  now carries a separate transport digest (`tarball_sha256`, sha256 of the
  tarball bytes) which the backend checks at download; when a registry
  predates it the wire check is skipped rather than failing a good
  transfer. Integrity is not weakened either way — install still re-verifies
  the extracted tree against the manifest hash. The old test passed only
  because it had written the tarball digest into the manifest-hash field,
  which is exactly the bug it should have caught.
- **`aoxn add` silently unbound a dependency from its registry.** Both
  `add` and `update` wrote `registry: None` on the way back to the
  manifest, so a dependency added against a second source quietly moved to
  the default one. `add --registry <name>` now binds one explicitly, and
  an existing binding is preserved.
- **Workspace lockfile fingerprints collided.** The requirements
  fingerprint was keyed by bare dependency name, so two members pinning
  the same package at different ranges overwrote each other. Keys are now
  `"<owner>/<dep>"`. The value no longer embeds the declaring manifest's
  absolute directory either — moving or renaming the project used to force
  a full re-resolve every time.
- **The resolver could attach the wrong checksum.** When finishing a
  resolution it scanned its index cache for "some index whose name
  matches"; a same-named package in two registries produced the wrong
  metadata, silently. Lookups are now keyed by `(registry, name)`.
- **A fresh tarball temp file could collide** between concurrent installs
  of the same package (pid-only naming); the thread id is in the name now.

#### Added
- **`devDependencies`** — `aoxn add -D`, `remove -D`, `update -D`,
  `install --prod` / `--dev-only`. Dev and prod resolve into **one**
  lockfile with a per-package `dev` flag; `--prod` materializes only the
  closure reachable from production roots. A package named in both tables
  is production, so `--prod` cannot break a build that needs it. `lockfile_version`
  stays 1 — every added field defaults, so older lockfiles still load.
- **`overrides`** — `{"overrides": {"http": "1.4.2"}}` replaces every
  requirement the graph places on that package (pnpm `overrides` / pip
  constraints in one line). An unsatisfiable override fails loudly instead
  of falling back. A bare version is a caret range; write `=1.4.2` to pin.
- **Curated registries** — a registry may now carry `trust.json` (a review
  record per package, tiers `unreviewed` / `community` / `audited`) and
  `advisories/*.json`. `aoxn trust bootstrap <url>` wires the default
  registry, the advisory database and the trust index up from one URL;
  `aoxn trust list` shows the records; `aoxn trust check <pkg> --tier`
  is a CI gate. Install warns about packages a curated registry has no
  reviewed record for — and stays silent for registries that make no trust
  claims, because a warning that fires everywhere is one nobody reads.
- **`aoxn list` and `aoxn freeze`** — a table of what is installed
  (name/version/scope/source, `--json`) and pip's `name==version` output for
  CI baselines and diffing.
- **`aoxn audit --json --audit-level <level> --fix`** — machine-readable
  findings, a severity floor (`low`/`medium`/`high`/`critical`), and an
  automatic bump to the advisory's patched version. `--fix` only *tightens*
  a constraint that already admits the patch; it never widens a range
  someone wrote on purpose, and says so when it cannot help.
  Audit now falls back to the default registry's `advisories/` when no
  advisory source is configured — previously, an unconfigured setup made
  the feature unusable rather than merely empty.
- **`aoxn install --json`** — the install report as data (the shape pip
  calls `--report`), for CI to consume.
- **Parallel downloads** — tarballs fetch `jobs` at a time per registry,
  default 8, via `--jobs` or `AOXN_JOBS`; `--jobs 1` is strictly serial.
  Each worker gets its own forked registry handle. Progress reports once
  per fetch phase instead of per package, since the per-package step line
  rewrites one terminal row that several threads would fight over.
- **Minimum compiler versions are enforced.** `IndexVersion.aoxn` has been
  parsed by every registry backend since v0.29.0 and consulted by nobody;
  a publish records the publishing compiler as the package's floor, and
  install now refuses a package that needs a newer one — with every
  offender listed — instead of failing somewhere deep in a compile.
- `aoxn list`, `aoxn freeze` and `aoxn trust` are also direct `aoxn`
  aliases, alongside the existing ones.

#### Tests
- 94 aoxn-pkg tests, up from 48: transport digest on both paths (verified
  when the index publishes one, skipped when it does not) and an explicit
  regression that the two digests differ — the old bug was invisible
  precisely because its test made them equal; prod/dev closure partitioning
  including a dev-only transitive subtree and a cycle; owner-qualified
  fingerprints; engines enforcement; overrides in the resolver; trust tier
  parsing and `freeze`/`list` output. Full suite: 161 workspace + 94
  aoxn-pkg = **255 green**.


## [0.31.1] - 2026-10-02

Theme: **continuing the IDE** — the fixes its own changelog promised, plus
the first round of editor conveniences. Also fixes a version drift: v0.31.0
shipped with the Cargo manifests still saying `0.30.0` (`aoxn version` lied);
both manifests now say `0.31.1`.

### Added
- **`aoxn check <file.ax>`** — runs the pipeline up to codegen and prints
  only the diagnostics (`--json` supported). This is what the IDE's Check
  button drives; `aoxn c` would have printed the whole generated C program
  into the output panel. A good file exits 0 with no stdout, a bad file
  exits 1 with the usual `[type] file:line:col:` lines on stderr.
- **New file / New folder in the IDE explorer** (`ide/src-tauri`
  `ide_new_file` / `ide_new_dir` + a prompt dialog in the workbench). The
  name is relative to the selected folder, nesting works in one step
  (`src/util.ax`), a created file opens immediately, and both the frontend
  (`lib/paths.ts`) and the backend refuse to escape the workspace or clobber
  an existing entry. The command returns the refreshed tree, so the explorer
  is never a round trip behind.
- **Auto-check on save** — saving an `.ax` file in the IDE runs `aoxn check`
  and refreshes the editor markers (skipped while a manual command owns the
  toolchain, when no compiler is found, or for non-Aoxn files).
- **`ide_root` command** — the explorer now learns the open folder from the
  backend instead of inferring it from the first tree row, which made an
  empty-but-open folder read as "no folder open".

### Fixed
- **The Monaco theme race** (v0.31.0's own "first thing to do next"): the
  Aoxn language and theme are registered in the Editor's `beforeMount`,
  strictly between Monaco's load and the first model's creation; the
  page-level registration could lose to the editor's mount and leave the
  light default theme showing.
- **Ctrl+S inside the editor silently did nothing in the native app.** The
  save and cursor callbacks passed `model.uri.toString()` as the file path;
  the browser preview's `/preview/...` paths happen to survive that
  round-trip, but a Windows path does not, and the workbench's document map
  never matched. Both callbacks now pass the document's real path (the same
  identity `onChange` already used).
- **Windows paths in the IDE are now spelled `D:\proj` instead of
  `\\?\D:\proj`** — `fs::canonicalize`'s verbatim prefix is stripped
  (`fsops::pretty`) before a path reaches the tree, the compiler, or the
  log, so echoed diagnostics compare equal to tree paths again.
- Opening a file no longer force-closes the explorer sidebar.
- The output-panel Check verb now reads `aoxn check …`, and a build or run
  refreshes the explorer so the produced executable shows up.

### Tests
- Frontend (`pnpm --dir ide test`): 21 node:test cases — the compiler-output
  parser (10) plus the new prompt-validation (6) and fixture-tree (5) suites.
- IDE Rust (`cargo test --manifest-path ide/src-tauri/Cargo.toml`): 16 tests,
  adding create-file/create-dir round-trips, clobber refusal, workspace
  escape through creation, and the no-verbatim-prefix rule (Windows).
- Root suite: 161 tests, unchanged pass.


## [0.31.0] - 2026-10-02

Theme: **the Aoxn IDE** — an official editor for the language, in `ide/`.
Aoxn has had a compiler, a package manager, a UI toolkit and a web benchmark
harness, but no tool you edit code *in*. This adds one.

### Added
- **The Aoxn IDE (`ide/`)** — a native workbench built on Tauri 2 with a
  Next.js + Monaco frontend: project explorer, tabbed editing with an Aoxn
  syntax grammar, an output panel, a status bar, and one-key Check /
  Build / Run. Diagnostics from the compiler become editor markers, and a
  diagnostic line in the output panel is clickable: it opens the file it
  names and puts the caret on the offending line.
  - **It drives the real compiler.** `ide_check` / `ide_build` / `ide_run`
    shell out to the same `aoxn` a user would type and capture its output
    verbatim; nothing about the build is reimplemented, so a build inside
    the IDE cannot disagree with a build in a terminal. `AOXN_IDE_CC`
    overrides the compiler; `AOXN_CLANG` is forwarded to it.
  - **Monaco is bundled, not fetched.** `@monaco-editor/react` loads Monaco
    from a CDN by default, which is fine for a website and wrong for a
    desktop app — an IDE has to work on a machine with no network.
  - **The workspace is a boundary, not a suggestion.** Every path crossing
    from the webview is canonicalised and refused if it resolves outside
    the folder the user opened, including the sibling-directory case a
    string-prefix check lets through (`C:\proj` vs `C:\proj-evil`). There is
    no general read-any-path command and no shell plugin; see
    `ide/src-tauri/capabilities/default.json`.
  - **Browser mode.** `pnpm dev` runs the whole workbench against an
    in-memory fixture folder, so layout, theme and interaction can be
    worked on without a native rebuild. The build/run buttons say plainly
    that there is no toolchain behind them rather than pretending.
- **`ide/src-tauri` is its own Cargo workspace** (`exclude` in the root
  `Cargo.toml`): a plain `cargo test` at the repo root must not compile a
  Tauri application.

### Known limitations
- No language server, so no IntelliSense or type-aware completion. The
  Monaco features that would call one are switched OFF rather than left
  spinning — a half-configured service would report a wall of red
  squiggles for perfectly valid Aoxn.
- A program is launched with `aoxn run`, so its output goes to the output
  panel rather than to an attached console; an interactive program that
  reads stdin will not see it.
- The project tree is capped at 8000 nodes and 8 levels deep.
- The Monaco colour theme is registered in `app/page.tsx`, which races the
  editor's own mount. In practice the editor shows Monaco's light default
  until that effect lands; `registerTheme` in the Editor's `onMount` is the
  fix and is the first thing to do next.

### Tests
- 12 Rust tests (`cargo test --manifest-path ide/src-tauri/Cargo.toml`):
  workspace escape (including the sibling-directory prefix trap), sorted
  tree with dependency directories skipped, depth reporting, read/write
  round-trip, `which` PATH resolution, exit-code and output capture.
- 10 tests for the compiler-output parser (`pnpm --dir ide test`), led by
  the two cases that actually break naive parsers: a Windows drive letter
  (`C:\src\main.ax:12:9:`) must not be read as the filename, and a
  successful run must produce zero markers.

## [0.30.0] - 2026-10-02

Theme: **one file installs everything, and that file is for Windows**. The
release artifact is a single `Aoxn-<version>-Setup.exe` carrying the compiler,
the standard library, the UI toolkit and the examples; double-clicking it
installs them through a native window and proves the result. In the same
release Aoxn became a **Windows-only** language: one platform, one installer,
one CI job, one UI backend.

### Added
- **`Aoxn-<version>-Setup.exe`** — the single-file installer. The
  `aoxn-setup` stub (`src/setup/main.rs`) with a stored archive of the
  toolchain appended after its PE image (`[image][payload][u64 len]["AOXNSFX\0"]`);
  `dist/package.ps1` builds it. No dependencies, no decompressor: the archive
  is stored, so the zero-external-crate rule holds.
- **A native installer window** (`src/setup/ui.rs`) — hand-rolled Win32, no
  `.rc` resource and no GUI framework: title, determinate progress bar, a
  scrolling log, Install→Finish button. The installation runs on a worker
  thread and the UI drains a channel on a timer, so a slow winget download
  never freezes the window. `-Console` / `-Quiet` runs it headless for
  scripts and CI; `-Prefix`, `-NoClang`, `-Uninstall`, `-?` are the rest.
- **`aoxn doctor`** (`src/doctor.rs`) — reports the install root, the stdlib
  location and its files, the resolved clang and its version, the build
  cache, then compiles and runs a one-line program that imports the stdlib.
  Exit code 0 means `aoxn run` works. `--json` for scripts and agents,
  `--no-smoke` to skip the compile step.
- **`aoxn version` / `aoxn --version`** — the manifest version, without
  starting the compile pipeline.
- **`src/paths.rs`** — install-layout discovery: the toolchain root
  (`$AOXN_HOME` or the parent of the executable's `bin/`), the stdlib
  directory (`$AOXN_STDLIB`, `<root>/lib/stdlib`, `<root>/stdlib`, or the
  checkout in dev builds) and a bundled `toolchain/bin/clang`.
- **`.github/workflows/release.yml`** — a tagged `v*` builds the installer
  on windows-latest, installs it into a scratch prefix, runs `aoxn doctor`
  and a stdlib program, and attaches the exe to the release.

### Changed
- **Aoxn is Windows-only.** Removed, not deprecated: `stdlib/ui_x11.ax` and
  `examples/ui_probe_x11.ax` (the X11 backend that served Linux and macOS),
  `web/server_posix.ax` and `web/sock_posix.ax`, the POSIX link flags
  (`-Wl,-rpath`, `-lm`) in `src/lib.rs`, the Linux and macOS CI jobs, the
  cross-platform release matrix, and `dist/install.sh` /
  `dist/package.sh`. `src/platform.rs` keeps its helpers — each now has one
  answer. See `docs/platform-support.md` for what a port would need.
- **The standard library resolves by name.** `import * from "stdlib"` and
  `import * from "stdlib/ui_win"` resolve against the installed stdlib when
  no local package of that name matches. A project-local `aox_modules/<name>`
  package still wins; relative (`./`, `../`) and absolute paths still bypass
  the search entirely.
- **`examples/` import the stdlib by name** (`"stdlib"`, `"stdlib/ui_win"`),
  so the examples inside the installer run from wherever it was unpacked.
- The Windows CI job now also packages the single-file installer, installs it
  into a scratch prefix and runs `aoxn doctor` plus a stdlib program — a
  broken installer fails the build, not the next release.
- `find_clang()` also looks in `<root>/toolchain/bin` and `<root>/LLVM/bin`
  after `AOXN_CLANG` and `PATH`: a portable LLVM dropped into the install
  makes the toolchain self-contained without touching the environment.

### Docs
- **`docs/install.md`** rewritten around the single exe (English + 中文).
- **`docs/platform-support.md`** replaced: what "Windows only" means, what
  was removed and why, and what porting back would take.
- README, SECURITY, spec, UI reference, CONTRIBUTING and AGENTS regenerated
  in both languages. The historical reports (`docs/llvm-independence-report.md`,
  `docs/web-benchmark.md`) keep their old numbers with a note marking the
  parts that are no longer reproducible.

### Tests
- **`tests/install.rs`** (6): stdlib-by-name resolution from an unrelated
  directory, `AOXN_STDLIB` redirection (and that the same import fails
  without it), the bundled examples resolving by name, `doctor` in text and
  JSON form, `--no-smoke`, and `version`.
- `tests/ui.rs`: the X11 backend tests are gone with the backend; the
  neutrality and `plat_*` coverage tests now pin the single Win32 backend.
- 211 green (163 workspace + 48 aoxn-pkg).

### Notes
- The compiler is still not statically self-contained — it emits C and shells
  out to clang. On Windows the linker additionally needs the MSVC Build
  Tools, which is exactly what the installer's smoke test detects.
- The self-hosted loader (`selfhost/load.ax`) stays repo-bound and resolves
  relative paths only, so `selfhost_frontend_handles_imports` now feeds it a
  fixture whose stdlib import is rewritten to a relative path.
- The wiki is frozen since v0.29.3 and still describes the multi-platform
  matrix.

## [0.29.7] - 2026-10-02

Theme: **surface-syntax parity with Python**, batch 1. Four Python forms that
Aoxn was missing, all implemented as *parse-time desugarings* so that the type
checker, the C backend, and the self-hosted compiler keep seeing exactly the
AST they already knew. `src/ast.rs` is untouched by this release.

### Added
- **Augmented assignment: `+= -= *= /= %=`.** On every target Python allows -
  a plain name (`x += 1`), an array slot (`arr[0] += 10`), a struct field
  (`p.x += 1`). Each desugars to the plain assignment `x = x + 1`, so the
  operand rules are unchanged: `+=` on a string concatenates, and an
  augmented form on a never-bound name is an error because the expansion
  reads the name first. `x //= n` deliberately does not exist (`//` is a
  division, not an assignment).
- **`//` integer division.** Python's spelling; a synonym of `/` on two
  `int`s, which already truncates. Documented deviation: both truncate toward
  zero, so `-7 // 2` is `-3` where Python floors to `-4`.
- **Unary `+`.** The identity on a numeric operand, as in Python.
- **Chained comparison: `a < b <= c`.** Means `(a < b) and (b <= c)` and
  short-circuits like a plain `and`; each link is type-checked on its own.
  Middle operands are evaluated twice by the desugaring - observable only if a
  middle operand is a side-effecting call.

### Changed
- **`docs/spec.md` is no longer the v0.9 document.** It had drifted from the
  implementation on exactly the points that matter when checking parity: it
  still claimed `while` was "the only loop (no `for` yet)", described the
  LLVM-era pass pipeline (`AOXN_PASSES`, `AOXN_DUMP_IR`, O0 fast-isel) that
  has been dead since v0.29.0, said `extern def` could not take `string`,
  described the UI toolkit as two Windows-only files, and its grammar omitted
  arrays, indexing, field access, f-strings, and struct construction. All
  corrected; the grammar now carries `//`, unary `+` and the augmented
  assignment forms, and every intentional deviation from Python (`//`
  truncation, no implicit int/float mixing, value semantics) is labelled as a
  deviation rather than left implicit.
- The roadmap is reordered by Python-parity cost, and the entries that are
  real language design rather than syntax (`None`, `try`/`except`, `with`,
  generators, closures, classes, dict/set literals) are collected at the end,
  so the real gap to Python is visible instead of implied.
- **The self-hosted compiler learned the same four forms** (`selfhost/
  lexer.ax`, `selfhost/parser.ax`): the new tokens `//` and `+= -= *= /= %=`,
  a `dup_expr` helper (the arena AST has no shared nodes, so a chain or an
  augmented assignment must copy an operand it uses twice), and the matching
  desugarings. The byte-identical fixed point still holds.

### Tests
- Six new tests in `tests/pipeline.rs` covering the four new forms: augmented
  assignment across all three target kinds (plus its undeclared-name error),
  `//`, unary `+`, chained comparison (including that the chain really
  short-circuits past a call, and that a badly typed link is still caught),
  and a clang-free check that a chain folds to `&&` in the emitted C.

## [0.29.6] - 2026-10-02

### Added
- **X11 UI backend (`stdlib/ui_x11.ax`) — the toolkit now runs on Linux and
  macOS.** Programs switch OS with one import line: `ui_win.ax` (Win32/GDI)
  or `ui_x11.ax` (X11 + Xft). Link with `-l X11 -l Xft`; on macOS run under
  XQuartz. Every v3 widget (layout managers, text selection + multi-line
  editing, clipboard, focus chain, menus, tree/table model+view, signal-slot
  events) works unchanged on both.
- `examples/ui_probe_x11.ax`: a self-closing X11 smoke probe (used by CI).

### Changed
- **The UI toolkit is now three files instead of one.** The 1,900-line
  widget layer moved out of `ui_win.ax` into a new platform-neutral
  `stdlib/ui_draw.ax`; `ui.ax` stays the portable core. The widget layer
  declares no platform externs and never tests `target_os()` — it talks to
  the windowing system only through a documented `plat_*` primitive
  contract that each backend implements. Windows rendering and behaviour
  are unchanged (the emitted C differs only by dead `target_os()` guards
  that clang already folded away, and the `plat_*` indirection).
- `ui_clip_get`/`ui_clip_set` take the `UI` context (`ui_clip_get(c)`), which
  X11 needs to reach the display connection.
- `ui_alert` is routed through `plat_alert` (on X11 it prints, since a modal
  dialog would need a nested event loop).

### Tests
- Five new tests in `tests/ui.rs`, all of which run with **no clang and no
  display**: both backends typecheck and emit C, the widget layer is proven
  free of Win32/Xlib symbols and of `target_os()`, both backends implement
  the same `plat_*` set, and every primitive the widget layer calls exists in
  both. The first three real defects found (a stale `plat_font`, two
  private helpers wrongly named `plat_*`) came straight out of these.
- The six pre-existing runnable UI tests now skip cleanly when clang is
  absent instead of failing (matches the self-hosting tests in
  `tests/pipeline.rs`).
- CI: the Linux job installs `libx11-dev libxft-dev xvfb`, links the gallery
  against `-l X11 -l Xft` and runs the probe under `xvfb-run`; the macOS job
  installs XQuartz and links the same gallery.

## [0.29.5] - 2026-10-02

**Package manager W2 step 3: the npm bridge — a one-shot import tool**
(`aoxn npm-import`). The roadmap's open decision ("一次性导入工具 vs 注册表
代理层") is resolved for the import-tool shape: the **npm CLI is the
transport** (`npm view` / `npm pack`), so auth, https and private
registries come from the user's `.npmrc` while the crate stays TLS-free
(the zero-build-script dependency constraint forbids an HTTPS client
here — a registry proxy would need one; revisit if that ever changes).

### Added
- **`aoxn npm-import <spec>…` / `--from package.json`**
  (`crates/aoxn-pkg/src/npm.rs`): imports an Aoxn package published to any
  npm-compatible registry (npm, Verdaccio, GitHub Packages). The npm
  tarball's sha512 (`dist.integrity`) is verified before unpacking; only
  packages with an `aoxn.json` in their root are accepted — plain
  JavaScript packages are rejected with an explanation (Aoxn cannot link
  JavaScript). Imported packages land in `vendor/<name>/` and are recorded
  in `aoxn.json` as path dependencies, reusing the whole install pipeline
  (materialization shims, manifest-hash integrity). Re-import overwrites
  the vendored tree. `--from` enumerates `dependencies` +
  `devDependencies` of an npm `package.json` (ranges resolved via
  `npm view`, newest match wins). `AOXN_NPM` overrides the npm binary.
- **`docs/pkg-manager.md`** — the package manager's topical document
  (quick start, manifest/lockfile/materialization, resolution, all three
  registry backends, npm bridge, cache, security model).

### Changed
- **`vendor/` is excluded from package tarballs** like `aox_modules/` —
  vendored npm imports are local working state, not publishable content.
- Obsolete pre-beta stashes `wip-pkg-all` / `wip-pkg-2` dropped.

### Tests
- 8 new tests (`npm::`): spec parsing (incl. scoped `@scope/pkg@^1`),
  base64 sha512 vectors, vendor + path-dep recording, re-import
  overwrite, plain-JS rejection, dry-run isolation, integrity mismatch,
  `package.json` enumeration — all against in-process npm-layout
  tarballs, no network. Full suite 48/48.

## [0.29.4] - 2026-10-02

**Package manager W2 step 2: HTTP registry backend (read-only).** The
registry abstraction gains a third backend: the same
`packages/<name>/…` tree the git and dir backends use, served over plain
HTTP/1.1 — a static file server or mirror is enough to host a registry.

### Added
- **`HttpRegistry`** (`crates/aoxn-pkg/src/registry/http.rs`): `index` /
  `all_names` / `fetch_tarball` over GET; `publish` / `yank` are hard
  read-only errors (publish through the git or dir backend that owns the
  tree). Registered automatically for `http://` URLs and explicitly via
  `{"kind": "http"}` in a `aoxn.json` `registries` entry.
- **Zero-dependency HTTP/1.1 client** on `std::net::TcpStream` (the
  crate's dependency set must stay free of build scripts, which rules out
  every TLS-capable HTTP crate): `Content-Length` and `Transfer-Encoding:
  chunked` bodies, 3xx redirect following (absolute / root-relative
  `Location`), 10 s connect / 30 s IO timeouts, `Connection: close`
  request style.
- **Checksum verified at transport**: a downloaded tarball's sha256 is
  checked against the index checksum before it enters the cache — a
  truncated or tampered transfer fails at download, not at extraction.
- `names.json` (`GET {base}/packages/names.json`) replaces the directory
  listing the git/dir backends enumerate; a static registry mirror must
  generate it (it feeds the typosquat guard).
- `https://` registry URLs fail early with a message explaining the
  TLS-free design (put a local reverse proxy in front of a remote
  registry, or use the git backend).

### Tests
- 8 new tests (`registry::http`): an in-process static HTTP server
  (`std::net::TcpListener`) exercises index/names/tarball round-trips +
  cache reuse, `PackageNotFound` on 404, integrity failure on a tampered
  tarball, offline refusal, read-only publish/yank, chunked decoding,
  https rejection, URL-parse errors. Full suite 40/40.

## [0.29.3] - 2026-10-01

**UI toolkit v3: text selection, multi-line editing, menus, model/view and
signal-slot events.** Fills the v2 gap list toward Qt: the editors now do
real selection (Shift+arrows / drag / Ctrl+A/C/X/V) with clipboard, a
multi-line editor joins the single-line field, the app gets a menu bar,
tree and table views backed by heap `TableModel`/`TreeModel` objects, and
the language's missing function pointers are answered with an integer-channel
signal-slot event bus. Also: `AGENTS.md` is now a published repo file and
the wiki is frozen (no more per-session wiki updates).

### Added
- **Text selection** (`ui.ax`, portable): `Sel{anchor, caret}` byte-offset
  model; `edit_type`/`edit_backspace`/`edit_delete`/`edit_left/right/up/
  down/home/end` are pure functions (UTF-8 aware, shift extends). Both
  widgets handle Shift+arrows, mouse drag, Ctrl+A, and Ctrl+C/X/V copy/cut/
  paste via `ui_clip_get`/`ui_clip_set` (Win32 CF_UNICODETEXT).
- **Multi-line editor** `ui_textedit` (→ `EditView{text, anchor, caret,
  changed, scroll}`): Enter inserts a newline, up/down move by line, wheel +
  scrollbar scroll, caret blink; line boundaries come from a cached
  line-start table (`lines_sync`) in the heap block.
- **Menus** `ui_menubar` + `ui_menu` (→ `MenuBar`/`MenuPick`): Qt's QMenuBar
  shape — hover switches open menus, outside click / Esc closes, items draw
  as a floating overlay on top via `ui_present`.
- **Model/view without interfaces**: heap-backed `TableModel` (rows×cols +
  headers + per-column widths) and `TreeModel` (parent/expanded/label)
  feed `ui_table` / `ui_tree`. Models are pointer structs so views mutate
  them in place (tree expander toggles the model directly); unset cells
  return `""`.
- **Signal-slot without function pointers**: `ui_connect(c, signal, slot)` +
  `ui_emit(c, signal, kind, a, b)` queue `Ev{slot, sig, kind, a, b}`; the
  app drains one `switch` on `ev.slot` per frame (32-event ring, reset each
  `ui_frame`). `ui_slot_of` reports the current binding.
- **Zero-alloc selection rendering**: `ui_measure_sub` / `ui_draw_text_sub`
  measure/draw a byte range through the per-frame arena — no substring is
  built, so a focused/selected editor allocates nothing per frame.
- `examples/ui_gallery.ax` gains an Editor page, a Tree & Table page and a
  menu bar with a signal-slot dispatch demo (5 tabs total).

### Fixed
- **NULL model cells crashed the tree/table views** — a `TableModel` cell or
  `TreeModel` label never set held a NULL string pointer, and drawing it
  (`len` on NULL) segfaulted. Getters now return `""` for unset slots.

### Changed
- **`AGENTS.md` is now a published, tracked repo file** (it was gitignored
  as local session context). The commit-policy section reflects this.
- **The wiki is frozen** (user instruction): sessions no longer update
  `wiki/` pages; `docs/` remains living documentation.

### Notes
- UI tests 4 → 6 (`ui_v3_portable`, `ui_window_v3_widgets_smoke`).
- No language or compiler changes; the self-hosting fixed point is
  unaffected (the UI toolkit is not on the selfhost path).

**UI toolkit v2: the Qt-grade widget library.** `stdlib/ui.ax` +
`stdlib/ui_win.ax` grow from 10 widgets with absolute coordinates into a
Qt-flavored toolkit: layout managers, real text input, keyboard focus, 20+
widgets, floating overlays and 16-role themes. The whole change lives in
the stdlib + tests + docs — no language or compiler changes, and the
self-hosting fixed point stays byte-identical.

### Added
- **Layout engine** (`ui.ax`, portable): `ui_vbox_begin`/`ui_hbox_begin`/
  `ui_grid_begin` + `ui_v_item`/`ui_h_item`/`ui_v_item_p`/`ui_h_item_p`
  (permille stretch) + `ui_grid_row`/`ui_grid_cell`/`ui_spacer`, margins
  and spacing, up to 8 nested boxes — the Qt `QVBoxLayout`/`QHBoxLayout`/
  `QGridLayout` counterpart for immediate mode. Handed out as `Rect`s;
  widgets keep taking explicit coordinates.
- **Text input**: `ui_textbox` (single-line field with caret, click-to-
  place, Backspace/Delete, UTF-8-aware arrow/Home/End movement, Enter
  reporting via `TextEdit{text, changed, enter}`) fed by a real `WM_CHAR`
  queue (surrogate pairs merged to UTF-8). Editing primitives
  (`str_sub`/`str_insert`/`str_remove`/`caret_left`/`caret_right`) live in
  the portable half and are tested on every platform.
- **Keyboard focus chain** (Qt's focus model): widgets register per frame
  in call order, Tab/Shift+Tab rotates the chain, an accent ring marks the
  focused widget, Enter/Space activates buttons/toggles/checkboxes/radios,
  arrows step sliders and spin boxes.
- **New widgets**: `ui_toggle`, `ui_radio` (exclusive groups),
  `ui_spinbox`, `ui_combobox` (floating drop-down list), `ui_listbox`
  (scrollable single-select, `ListPick{cur, scroll}`), `ui_tabs`,
  `ui_groupbox`, `ui_scroll_begin`/`ui_scroll_end` (clipped viewport +
  wheel + draggable scrollbar), `ui_tooltip` (~0.55 s hover delay).
- **Disabled mode**: `ui_begin_disabled`/`ui_end_disabled`/`ui_disabled` —
  Qt's `setEnabled` pattern; groups draw dimmed and ignore input.
- **Wheel input**: `WM_MOUSEWHEEL` deltas are accumulated per frame
  (`c.wheel` / `ui_wheel`) and scroll lists, drop-downs and scroll areas.
- **Overlays drawn on top**: combo popups and tooltips are recorded during
  the frame and rendered by `ui_present` after every widget; while a popup
  is open the click is swallowed (menu behavior) via an active-slot
  sentinel.
- **16-role palettes**: `sel sel_text disabled_face disabled_text
  tooltip_bg tooltip_text` join the original 10 (light + dark).
- **Caret cache** (`ui_textbox`): caret x is memoized on
  (widget id, caret, length) so an idle focused field allocates nothing
  per frame (the steady-state-no-leak discipline holds).
- `examples/ui_gallery.ax` — a three-page Qt-style widget gallery built on
  the layout engine (controls / input / containers).
- Tests: `ui_layout_text_focus_portable` (all platforms: layout rect math,
  editing primitives, UTF-8 caret boundaries, char queue, tab order,
  disabled counter, palette, overlay slots, hover timing) and
  `ui_window_v2_widgets_smoke` (Windows: every v2 widget in a real window,
  bounded + self-closing). UI tests 2 → 4; suite total 175 (pipeline 99 +
  lib 6 + TS 34 + UI 4 + aoxn-pkg 32).

### Fixed
- **`DC_PEN` is stock object 19, not 20** — `GetStockObject(20)` is out of
  range so `SelectObject` failed silently and pen-only drawing
  (`ui_frame_rect`, checkbox ticks, button/slider borders, focus rings)
  never reached the bitmap (it shipped this way in v0.27.0; found by
  pixel-inspecting rendered frames in v0.29.2). All widget outlines now
  render.
- The scroll-area scrollbar drew inside its own content clip region and
  was clipped away; it is now drawn before the clip is pushed.

### Removed
- **macOS Intel (x86_64) support** — the `macos-x86_64` / `macos-13` CI job is
  dropped and Intel Macs are removed from the platform tables, the issue
  template and the docs. This restores the v0.27.1 decision (the job had
  crept back in with the v0.28.0–v0.29.0 work). Tier 2 is Linux x86_64 +
  macOS arm64; the CI matrix is three jobs.

## [0.29.1] - 2026-10-01

**Package manager W2 step 1: manifest entry resolution lands on the compiler
side.** A bare package import (`import * from "http"`) now resolves its
entry through the package's `aox_modules/<name>/aoxn.json` manifest —
`main`, `exports` (incl. `pkg/sub` subpaths), and `types` — so an installed
package whose entry is not literally `index.ax` is finally importable. This
is the compiler-side half of the W2 package-management milestone (spec
§4); `aoxn pkg` itself gained the matching `exports`/`types` manifest
fields. No language or backend change; the C-emitting pipeline is
untouched. 173 tests (pipeline 99 + lib 6 + TS 34 + UI 2 + aoxn-pkg 32).

### Added
- **`src/pkg_manifest.rs`** — a zero-dependency JSON value parser
  (hand-rolled; `src/` keeps its zero-external-crate security property) that
  reads `aox_modules/<pkg>/aoxn.json` and resolves the entry source file
  for a (sub)path. `exports` values may be a plain path string or a
  conditional object (`{ "default": "...", "types": "..." }`), mirroring
  the npm convention closely enough for Aoxn's needs.
- **`exports` and `types` fields** on the `aoxn-pkg` `Manifest`
  (`crates/aoxn-pkg/src/manifest.rs`); `entry_file()` now prefers
  `exports["."]` then `main` then the legacy probe order.
- **Subpath package imports**: `import * from "pkg/client"` resolves via
  `exports["./client"]`.
- **BOM tolerance** in the manifest reader (a PowerShell-written
  `aoxn.json` with a UTF-8 BOM no longer breaks resolution).

### Changed
- **`resolve_import`** (`src/lib.rs`): bare identifiers consult the
  package manifest first; a package without `aoxn.json` (or one whose
  manifest has no resolvable entry for the subpath) falls back to the
  legacy `aox_modules/<name>` directory probe, keeping pre-manifest
  packages working.
- **`complete_module_path`**: switched `base.exists()` → `base.is_file()`
  (and the per-extension probes likewise). A directory at `base` no longer
  short-circuits the probe, so `index.<ext>` inside a directory-named
  package/module is actually reached — a latent bug that was masked because
  every call site passed paths with an explicit extension.

### Tests
- `tests/pipeline.rs`: `pkg_manifest_main_entry_resolution`,
  `pkg_manifest_subpath_exports`, `pkg_no_manifest_falls_back_to_probe`.
- `src/pkg_manifest.rs`: 6 unit tests (main, exports subpath, conditional
  object, missing-subpath-None, no-manifest-None, BOM).
- `crates/aoxn-pkg/src/manifest.rs`: `parses_exports_and_types`,
  `exports_roundtrip_omits_empty`, `deny_unknown_fields_still_holds`.

## [0.29.0] - 2026-10-01

**LLVM independence Phase 2 complete: the LLVM dependency is gone.** The
C-emitting backend (v0.27.1's `--backend c`) is now the compiler's only
backend. `src/llvm.rs` (the hand-written LLVM-C FFI), `src/codegen.rs` (the
LLVM-IR codegen) and `build.rs` (LLVM probing and linking) are deleted, and
`cargo build` no longer searches for, links, or ships any LLVM library. What
remains is clang — the C toolchain that compiles the generated C and performs
the final link; it was always required.

### Removed
- **The LLVM backend** (`src/llvm.rs`, `src/codegen.rs`) and the LLVM
  probing/linking in `build.rs` (the build script is gone entirely;
  `Cargo.toml` no longer declares one). Building the compiler now needs only
  the Rust toolchain; compiling and running programs needs only clang.
- **`aoxn ir`** (the optimized-IR dump) — there is no IR anymore. The new
  **`aoxn c`** prints the generated C text; the old `ir` spelling is kept as
  an alias that forwards to it.
- **`AOXN_PASSES`, `AOXN_BACKEND` and `--backend llvm`** — the pass-pipeline
  override and the backend switch have nothing left to select. `--backend c`
  is still accepted (it is the default and only backend); any other value is
  rejected with a pointer at v0.29.0. `AOXN_DUMP_IR` became `AOXN_DUMP_C`.
- **`platform::llvm_dir_candidates` / `platform::llvm_link_name[_in]`** and
  the self-host tests' `-l LLVM-C -L ...` link flags.

### Changed
- **The self-hosted compiler emits C text** (`selfhost/codegen.ax` rewritten
  from an LLVM-C driver into a C emitter mirroring `codegen_c.rs`): structs
  are C structs, arrays are wrapped in single-field structs
  (`typedef struct { T data[N]; }`) for value semantics, and the self-hosted
  driver writes the C next to the object and shells out to
  `clang -O3 -w -c`. `emit_ir` became `emit_c` (writing `stdlib_use.c`
  instead of `stdlib_use.ir`), `gen_ir_text` became `gen_c_text`, and
  `gen_dispose` is gone (no handles left to dispose).
- **The self-hosting fixed point is now C-text based** and therefore no
  longer Windows-only: the stage-1 (Rust-built) and stage-2 (Aoxn-built)
  compilers must emit byte-identical C for the same program — and, because
  clang is deterministic for identical input, byte-identical objects too.
  The old skip (`C:/Program Files/LLVM/lib/LLVM-C.lib` must exist) is gone;
  the self-host tests now skip only when clang itself cannot be found.
- **Optimization levels** now select the clang `-O` level used to compile the
  generated C; the C text itself is level-independent (asserted by the new
  `c_text_is_opt_level_independent` test, which replaces the distinct-IR
  test). `--O0` no longer means "fast-isel": it means `clang -O0`.
- **CI** installs clang only: Windows keeps the winget LLVM package purely
  for its clang, Linux apt-installs `clang`, and macOS uses the preinstalled
  Apple clang — no `llvm-18-dev`, no `brew install llvm@18`, no
  `AOXN_LLVM_DIR` anywhere.

### Fixed
- **Byte-exact self-hosting off Windows** (the last open item in
  docs/platform-support.md §7): without the LLVM-C library requirement, the
  fixed-point test no longer depends on a Windows-only library path and runs
  on every Tier-1/Tier-2 platform with clang.
- **The fixed-point object comparison masks the COFF `TimeDateStamp`**
  (bytes 4–8 of every Windows object): clang stamps each object with the
  wall-clock time of its run, so two compiles of identical C never matched
  byte-for-byte and `selfhost_driver_self_compiles` failed on Windows despite
  a correct compiler. The generated-C comparison is still exact.


## [0.28.0] - 2026-09-29

### Fixed
- **POSIX link line passes `-lm`** — the TS `%` lowering calls C `fmod`
  (`__ts_mod`), and on Linux/macOS C math lives in a separate libm while the
  Windows CRT link covers it; Linux CI failed with "undefined reference to
  `fmod'". Both link paths get the flag: `src/lib.rs` `link_opts` (after the
  user `-l` list so `--as-needed` toolchains still resolve) and the
  self-hosted driver's clang command.


**TS-M1 W1 complete** — the TypeScript front end lands its type layer (S2b)
and the module system (S3), and the whole repository migrates off the legacy
`import "path"` syntax in one switch. Self-hosting fixed point (byte-identical
IR + COFF) re-verified unchanged.

### Added (S2b type layer)
- **f64 numeric tower**: `number` is double everywhere; numeric literals are
  JS numbers (f64), integer positions (indexes, lengths) convert via the new
  internal `Expr::Cast`; mixed int/float arithmetic auto-shapes literals to
  the other side's width. New **conversion builtins** `to_int(x)` /
  `to_float(x)` (Aoxn language, spec.md updated) back the tower.
- **JS bit semantics**: `& | ^ ~ << >> >>>` lower to `__ts_*` runtime helpers
  (injected Aoxn source: ToInt32 patterns, floor-shift, 32-bit wrap); `%` is
  fmod. `console.log` and template substitutions print numbers in JS form
  (`__ts_num`: integral values without `.000000`, trailing zeros trimmed) and
  `console.log(a, b)` joins space-separated like JS.
- **Unions + null narrowing**: `T | null | undefined` erases to `T` with
  sentinel null values (`""` / `0` / `false`); `x == null` / `x != null`
  compare against the sentinel. Known M1 deviation (documented): the sentinel
  is indistinguishable from a legitimate zero-ish value.
- **`any`/`unknown`** (64-bit int boxes, narrowed with `as`), `never`,
  **optional/default parameters** (`x?: T`, `x: T = e` — omitted call sites
  fill the sentinel/default through an AST post-pass, hoisting-safe),
  **tuple types** (`[A, B]` → synthesized value struct with `_0..` fields,
  read via `t[0]`), **`as`/`!` assertions** (checked scalar casts; `!` is
  identity in M1's value model).

### Added (S3 modules)
- **Module forms**: `import * from "p"` (whole-module merge), `import { a, b }
  from "p"`, `import d from "p"`, side-effect `import "./p"`; `export
  function`/`export interface` pass through as ordinary declarations. The TS
  front end now emits real `ImportDecl`s into the shared pipeline.
- **Loader resolution**: `./`/`../` specifiers complete with `.ax`/`.ts`/
  `.tsx` and `index.<ext>`; bare names probe `aox_modules/<pkg>` (full
  manifest resolution is W2's `aoxn pkg`). `scan_imports` (build-cache
  dependency walk) understands every form.
- **The bare `import "path"` form is removed** (diagnostic points at the new
  syntax). All of `stdlib/`, `selfhost/`, `examples/`, `web/` and the
  embedded test fixtures migrated in the same change; the self-hosted
  `parser.ax` accepts the new forms and rejects the old one, `load.ax`
  gained `.`/`..` path normalization so its include-once/cycle keys stay
  stable — the byte-identical fixed point passes unchanged.

### Known limits (tracked in docs/ts-m1-spec.md §0.1)
- multiple independent array-length parameters per function (the
  monomorphizer still carries one length parameter);
- `import * as ns`, `export default`, re-exports, top-level module
  statements — later slices; `typeof`/classes/closures — TS-M2.

## [0.27.1] - 2026-09-30

**Experimental C-emitting backend** (`--backend c` / `AOXN_BACKEND=c`) — the
first deliverable of the
[LLVM-independence investigation](docs/llvm-independence-report.md): instead
of building LLVM IR, codegen emits ISO C99 text and hands it to the same clang
toolchain that already does the final link. Everything else is unchanged: the
typechecked AST, the clang link step, the `run`/`build` cache, and the
self-hosted compiler are untouched. The default backend stays LLVM; the C
backend is opt-in.

### Added
- **`src/codegen_c.rs`** (~900 lines) — walks the same AST as `src/codegen.rs`
  and emits C. Mapping highlights: structs map to C structs; arrays wrap in
  single-field structs (`typedef struct { T data[N]; }`) for C value
  semantics matching the language spec; `nsw`/`inbounds` UB maps to plain C
  signed arithmetic and indexing; raw-memory builtins go through `memcpy`
  helpers (strict-aliasing safe); no C standard header is included — the
  runtime surface uses `__builtin_*` forms and every other C function is an
  Aoxn `extern def` declaration in the same shape the LLVM backend uses.
- **`--backend <llvm|c>` CLI flag** plus the `AOXN_BACKEND` env var (what the
  test suite uses to run everything through one backend); the build cache key
  includes the backend.
- **`c_backend_matches_llvm_backend` test** — runs three multi-feature
  examples through both backends via the real CLI and asserts byte-identical
  stdout and exit codes (suite: 127 → 128).

### Measured (2026-09-30, this machine, interleaved min of 5–7 runs)
- Output parity: all 11 runnable `examples/` byte-identical between
  backends; the full suite (pipeline 97 + TS 28 + UI 2) passes under
  `AOXN_BACKEND=c` as well.
- Runtime: within ±10% of the LLVM backend on the example benchmarks
  (both are `clang -O3`-optimized native code).
- Compile time end-to-end: `examples/hello.ax` 836 → 884 ms; the ~7k-line
  self-hosting input 4000 → 4226 ms (+6%). The feared C-front-end regression
  did not materialize at O3.

### Notes
- The C backend is experimental: `aoxn ir` still prints LLVM IR (the C
  backend has no IR stage), `AOXN_PASSES` does not apply, and C's unspecified
  evaluation order between operands means expressions with multiple calls
  may order side effects differently than the LLVM backend (the spec does not
  pin an order either).
- No language, semantic, ABI, or self-hosting changes; the byte-exact
  self-hosting fixed point is unaffected.

## [0.27.0] - 2026-09-29

**UI standard library** — a Qt-flavored, immediate-mode GUI toolkit written
100% in Aoxn on top of raw Win32/GDI FFI (`stdlib/ui.ax` + `stdlib/ui_win.ax`,
`docs/ui.md`, `examples/ui_demo.ax`). No compiler, language or codegen
changes: the byte-exact self-hosting fixed point and the full suite pass
unchanged.

### Added
- **`stdlib/ui.ax` — the portable half** (compiles and links on every
  platform, no platform externs): UTF-8 → UTF-16LE conversion with
  surrogate pairs (`utf16_write`/`ui_utf16`), COLORREF packing (`ui_rgb`),
  position-derived widget ids (`ui_wid_id`), little-endian i32 assembly
  (`i32_at`), the `UI`/`Palette`/`TextSize` value types, light + dark
  palettes, heap-state accessors (`st_get`/`st_set`, documented slot
  layout) and the per-frame bump arena (`utf16f`/`itoa10`) that keeps
  steady-state frames allocation-free — string concat leaks by design, so
  per-frame UI text is the discipline that needs it most.
- **`stdlib/ui_win.ax` — the Windows backend** (the web suite's
  `sock_win.ax` pattern: the backend is its own file; switching backends
  later = changing one import line):
  - lifecycle `ui_init` / `ui_frame` / `ui_present` / `ui_close` /
    `ui_fini` / `ui_alert` — DPI-aware window with an exact client size
    (`AdjustWindowRect`), `FreeConsole()` to drop the console-subsystem
    terminal, CS_OWNDC + NULL background brush double-buffered GDI
    rendering, per-frame resize detection, `MsgWaitForMultipleObjects`
    frame cap (default 15 ms) so idle windows cost ~0% CPU;
  - immediate-mode widgets: `ui_button` (press+release click, hover/press
    faces, centered label), `ui_checkbox` and `ui_slider` (caller owns the
    value; drag continues outside the track via a single press-claim
    slot), `ui_progress`, `ui_label`/`ui_label_dim`/`ui_label_int` (int
    rendering without per-frame string building), `ui_title`,
    `ui_separator`, `ui_panel`;
  - primitives `ui_fill_rect` / `ui_frame_rect` / `ui_draw_line` /
    `ui_draw_text(_big)` / `ui_measure`, polled input
    (`c.mx/c.my/c.dn/c.dn2`, `ui_mouse_in`, `ui_key_down`,
    `ui_key_pressed` with a 256-key snapshot and per-frame edge
    detection);
  - **no callbacks anywhere**: the window procedure IS DefWindowProcW
    (address via GetProcAddress — the language has no function pointers),
    everything is polled per frame; `GetAsyncKeyState` bit 15 is OR-ed
    with the bit-0 "pressed since last call" latch so sub-frame presses
    are never lost, with exactly one call per key per frame;
  - all Win32 constants are hand-summed decimal literals (the language
    has no hex literals and no bitwise operators); a real W-suffix trap
    is recorded inline: `TranslateMessage` has no `W` export (its `MSG*`
    is charset-neutral) — `TranslateMessageW` does not exist in
    user32.lib.
- `examples/ui_demo.ax` — every v1 widget, light/dark palette switch,
  drawing primitives, UTF-8/emoji text, Esc-to-close. Run:
  `aoxn run examples\ui_demo.ax -l user32 -l gdi32`.
- `tests/ui.rs` — `ui_pure_helpers_utf16_rgb_ids` (all platforms: encoding
  incl. surrogates, rgb packing, ids, palettes, negative i32 reads) and
  `ui_window_selfclose_smoke` (Windows: builds a real window, ~90 bounded
  frames, self-closes; skips with exit code 3 on headless sessions).
- `docs/ui.md` — design rationale, full API reference, platform matrix,
  limits and roadmap.

### Docs & wiki
- **`README.md` rewritten** against the current tree — it still claimed
  "v0.7 · 70/70 tests" and pre-fixed-point self-hosting. Now: v0.27.0 status,
  127 tests, the self-hosting fixed point, the UI toolkit quickstart, the TS
  front end in the layout table. Bilingual (English first, 中文 second).
- **`SECURITY.md` rewritten bilingually** (English first, 中文 second);
  the supported-version table now covers 0.27.x, the cache-poisoning entry
  covers `build` as well as `run`, and the UI toolkit's raw FFI is called out
  in the out-of-scope section.
- `wiki/Standard-Library.md` gains the UI section in both languages;
  `wiki/Home.md`/`wiki/Roadmap.md` move the verified version to 0.27.0;
  `wiki/Testing-and-CI.md` test counts refreshed (127 total).

## [0.26.3] - 2026-09-27

**Compile-speed follow-through** — `aoxn build` joins the `run` content-hash
cache, and the front-end hot spots found by a fresh audit are fixed. No
language or codegen changes: the byte-exact self-hosting fixed point
(`selfhost_driver_self_compiles`, IR + COFF object) passes unchanged.

### Added
- **`aoxn build` reuses the content-hash cache** (shared key with `run`, so a
  `run` followed by a `build` of the same program shares one entry): a
  repeated build of unchanged sources copies the cached executable instead of
  recompiling and re-linking — measured `aoxn build examples/hello.ax -o
  out.exe` ~0.8–1.6s → ~0.2s on the dev machine (every `build` previously
  always paid full compile + link). Same invalidation semantics as `run`:
  any source, option, `-l`/`-L`, or compiler-binary change is a miss;
  `AOXN_NO_CACHE=1` / `AOXN_CACHE_DIR` apply unchanged; concurrent
  invocations publish through the same `<key>.<pid>` temp + rename.

### Changed (performance)
- Front-end micro-optimizations from a fresh hot-spot audit (all IR-invariant,
  verified by the fixed-point test; effects are individually below the dev
  machine's noise floor since the front end is <1% of a large build, but they
  remove real algorithmic and allocation costs):
  - the monomorphization instance queue is a `VecDeque` — the old
    `Vec::remove(0)` drain was O(m²) in the instance count;
  - typecheck struct-field lookup is O(1): `StructTable` values are now a
    `StructInfo { fields, index }` (mirroring codegen's `struct_fields` map),
    covering struct literals, `Point(...)` construction and field access;
    duplicate/missing-field diagnostics keep byte-identical messages and
    error positions;
  - `type_size` memoizes per `Type` — thousands of aggregate copies no longer
    re-walk the same LLVM types through `LLVMStoreSizeOfType`;
  - direct call sites no longer clone the callee's whole `params: Vec<Type>`;
    only the per-parameter compound flag survives the borrow;
  - generic call sites no longer clone the callee signature pieces before the
    dedup check (cloning happens only when a new instance is created);
  - `AOXN_TC_TRACE` / `AOXN_CG_TRACE` are read once per compile instead of
    once per function; `printf` declaration goes through the shared `externs`
    cache like every other C helper.

### Verified not actionable
- **F5 (link-layer probe caching) re-examined with data and closed**: the
  report's premise was that clang's ~136ms MSVC detection is skippable.
  Measured on clang 23.1.0 / Windows: `clang --version` (pure driver startup
  floor) costs 156–189ms **with and without** `INCLUDE`/`LIB` set — there is
  no environment fast path, and link timings with/without env are
  indistinguishable within the machine's noise. Combined with the report's
  earlier finding that spawning `lld-link` directly is a wash (§四), F5 stays
  skipped — now with evidence instead of an open question. The remaining
  link/startup costs are DLL-load floors (LLVM-C.dll, clang's own DLLs).

### Docs/community (e99a3b0, landed with this cycle)
- CODE_OF_CONDUCT.md, CONTRIBUTING.md, SECURITY.md and GitHub issue/PR
  templates added; CONTRIBUTING.md now states the aggregate-ABI codegen
  invariants inline (they previously lived only in gitignored AGENTS.md).

## [0.26.2] - 2026-09-27

**Compile-time work from `docs/optimization-report.md` + Tier-2 CI fixes** —
the front end was already thin (<1% of a 7k-line build); these four items
attack the fixed costs around it. Default codegen is unchanged (still
`default<O3>`), so the "parity with `clang -O3`" promise and the self-hosting
fixed point are untouched (F1/F2 only add opt-in paths; F3 sits outside the
compiler). The same release makes the Linux/macOS CI matrix green for the
first time (see **Fixed**).

### Added
- **`--O1` fast-compile level (F1)**: runs the `default<O1>` pipeline. On the
  7k-line self-host input the pass pipeline drops from ~2.4s to ~1.1s
  (≈-50% compile time); recommended for iteration and compile-time-sensitive
  CI. Measured cost: recursion/inlining-heavy code (fib) runs ~38% slower,
  loop-shaped code (primes) is unaffected — which is why O3 stays the default.
  `--O2`/`--O3` are also accepted for symmetry, and at most one level flag may
  be given (otherwise exit code 2). `AOXN_PASSES=<pipeline>` still overrides
  the pipeline text at any level > 0.
- **`Aoxn run` build cache (F3)**: the executable is cached in
  `target/cache/` keyed on the content hash of the entry file *and all
  transitive imports*, plus the compiler binary's identity (size + mtime) and
  every codegen-affecting option (level, `AOXN_CPU`, `AOXN_PASSES`, `-l`/`-L`,
  resolved clang path). An unchanged re-run skips compile + link: measured
  `Aoxn run examples/hello.ax` 1113ms cold → ~85ms warm. `AOXN_CACHE_DIR`
  relocates the cache, `AOXN_NO_CACHE=1` disables it, and the cache is bounded
  to 64 entries (oldest-first, hits refresh the mtime).
- **`aoxn::dependency_files`**: public helper returning the entry file plus
  every transitively imported source file (`None` when a file is unreadable),
  so the cache key covers the whole program instead of just the entry.
- **Tests**: `optimization_levels_agree_on_program_output` (O0..O3 produce
  identical program behavior), `optimization_levels_produce_distinct_ir`
  (O0/O1/O2/O3 really select different pipelines),
  `dependency_files_follows_import_chain` (transitive imports are found, a
  missing file disables the cache), and
  `llvm_link_name_probe_covers_platform_layouts` (Windows/macOS/Ubuntu LLVM
  library layouts). Suite: 93 → 97 integration tests.

### Changed
- **`--O0` now really is O0 (F2)**: `--O0` used to skip only the IR pipeline
  while the target machine still ran at `CodeGenOptLevel` 2. It now creates
  the target machine with `LLVMCodeGenLevelNone`, so instruction selection
  takes LLVM's fast-isel path. (`CODEGEN_LEVEL_NONE`/`CODEGEN_LEVEL_LESS`
  added to `src/llvm.rs`.)
- **Optimization level plumbed through the API**: `opt: bool` became an
  `opt_level: u8` internally; every existing `bool` entry point is retained as
  a forwarding wrapper (`true` = O3, `false` = O0), so `tests/pipeline.rs` and
  the self-host drivers are unaffected. New `*_lvl` entry points
  (`build_paths_opts_lvl`, `compile_paths_to_ir_lvl`, …) take the level.
- **Dev profile compiles optimized (F4)**: `[profile.dev] opt-level = 1` in
  `Cargo.toml`. The compiler is ~8x slower at codegen when built unoptimized,
  which made every `cargo run` iteration pay for it. Benchmark with
  `--release` as before.

### Fixed
- **Tier-2 CI (Linux / macOS) had never actually run green** — the v0.26.1
  platform matrix failed on `linux` and `macos-arm64` with 5 failures each;
  all of them were on the *test / self-hosted* side (the Rust compiler itself
  was already platformized). See `docs/platform-support.md` §7:
  - **`-lLLVM-C` does not exist on Linux**: Debian/Ubuntu put the C API inside
    a versioned `libLLVM-<N>.so`, so the self-host tests died with
    `cannot find -lLLVM-C`. New `platform::llvm_link_name()` probes the install
    (`LLVM-C` → newest `libLLVM-<N>` → unversioned `libLLVM`); `AOXN_LLVM_LIB`
    overrides it. Covered by `llvm_link_name_probe_covers_platform_layouts`.
  - **Apple Silicon**: the self-hosted code generator registered only the X86
    backend, so arm64 hosts failed with `no available targets are compatible
    with triple arm64-apple-darwin`. `selfhost/codegen.ax` now registers
    AArch64 too (the self-host counterpart of the Rust-side B3 fix).
  - **PIE on Linux**: `selfhost/codegen.ax` created its target machine with
    `RELOC_DEFAULT`; it now uses `CG_RELOC()` (0 on Windows, 2 = PIC
    elsewhere), mirroring `platform::is_windows()`.
  - **POSIX PATH separator**: the self-host tests built `PATH` as
    `"{llvm_bin};{PATH}"`, which on POSIX collapses to one nonexistent
    directory — the drivers then could not find `clang`. They now use
    `std::env::join_paths`.
  - **Loader path for linked libraries**: `link_opts` passed `-L` but no
    rpath, so an executable linked against a non-default LLVM dir (Homebrew's
    `libLLVM-C.dylib` re-exports `@rpath/libLLVM.dylib`) could not start; each
    `-L` dir now also gets `-Wl,-rpath,<dir>` on POSIX (Windows linkers reject
    `-rpath`, and `build.rs` already did this for the compiler itself).
  - **`system()` semantics**: the stdlib exposed the raw C `system()`, whose
    POSIX return is a wait status (`exit 7` → 1792) while Windows returns the
    exit code. New `system_exit_code(cmd)` normalizes both (a signal-killed
    process is reported shell-style as 128 + signal); `stdlib_system_spawn`
    uses it and asserts `7` on every platform.
- Local verification: 97/97 integration tests green on Windows, including the
  self-hosting fixed point and the self-hosted codegen/loader parity tests.

### Notes / follow-ups
- Not done (deliberately, from the report): link-layer micro-tuning (already at
  the lld-link floor, ≤130ms/call) and delay-loading the 73MB `LLVM-C.dll`
  (F5/F6 — low value, Windows-specific).
- `docs/spec.md` "Tooling contract" documents the level flags, `AOXN_PASSES`,
  and the run cache.

## [0.26.1] - 2026-09-27

**Cross-platform migration (Linux x86_64, macOS x86_64/arm64)** — Aoxn is no
longer Windows-only. The compiler builds and passes the full suite on four
platforms; CI runs the matrix.

### Added
- **`target_os() -> string` builtin**: compile-time platform query returning
  `"windows" | "linux" | "macos" | "other"`, folded to a module-internal
  string constant (same value from both the Rust and self-hosted compilers on
  the same host). This is the minimal platform-awareness facility Aoxn
  programs need to branch on OS-specific code.
- **`src/platform.rs`**: central platform abstraction (exe/obj extensions,
  stack-link flag, target-OS name, LLVM lib candidates). `lib.rs`,
  `main.rs`, `codegen.rs`, `build.rs` all route through it.
- **`build.rs` platform portability** (B1): probes for `libLLVM-C` /
  `libLLVM-XX` / `libLLVM` (Linux), `libLLVM.dylib` (Homebrew), or
  `LLVM-C.lib` (Windows); emits an rpath so `aoxn` finds libLLVM at runtime;
  accepts `AOXN_LLVM_DIR` (with a `AXON_LLVM_DIR` legacy alias).
- **AArch64 backend registration** (B3): `LLVMInitializeAArch64{TargetInfo,
  Target,TargetMC,AsmPrinter}` registered alongside X86 (they don't conflict;
  `LLVMGetTargetFromTriple` selects by triple). Apple Silicon is now a
  first-class target.
- **PIC relocation on non-Windows** (B4): the target machine is created with
  `RELOC_PIC` on Linux/macOS (PIE is the default there) and `RELOC_DEFAULT` on
  Windows.
- **CI matrix** (T1.5/M1-M3): `windows-latest`, `ubuntu-latest`, `macos-13`
  (Intel), `macos-14` (arm64) — each runs build + 93 tests + smoke.

### Changed
- **Self-hosted codegen (`selfhost/codegen.ax`)**: the `_setmode(1, 0x8000)`
  entry-wrapper call is now emitted only when `target_os() == "windows"`
  (S2). POSIX stdout is already binary-safe.
- **Self-hosted driver (`selfhost/driver.ax`)**: the `-Wl,/STACK:8388608`
  link flag is Windows-only (S1); POSIX main-thread stacks are 8MB already.
  New `exe_suffix()` helper (`.exe` on Windows, empty elsewhere).
- **`selfhost/driver_self_demo.ax`**: the LLVM lib dir and artifact suffix are
  chosen by `target_os()` (S3) instead of hardcoded `C:/Program Files/...`.
  Other `driver_*_demo.ax` use `exe_suffix()` for portable output names.
- **`tests/pipeline.rs`**: the ~30 hardcoded `.exe` suffixes are replaced by a
  platform-aware `EXE` const (S3's Rust half).
- **`docs/spec.md`**: `target_os()` documented under "Platform query"; new
  "Platform support" section declaring Tier 1 = Windows x86_64, Tier 2 =
  Linux x86_64 / macOS x86_64 / macOS arm64, with per-platform toolchain notes.

### Notes / follow-ups
- Linux/macOS behavior is CI-verified; local verification on Windows used the
  same code paths (`platform::is_*` are `cfg!`-based, so the Windows build is
  byte-identical to v0.26.0 apart from the new builtin).
- The fixed-point test (`selfhost_driver_self_compiles`) compares ELF/Mach-O
  objects on non-Windows instead of COFF; the comparison is still byte-identical
  (same platform, same reloc model on both sides).
- `AOXN_PASSES=<pipeline>` (from v0.26.0) remains available for pass-pipeline
  experiments.

## [0.26.0] - 2026-09-26

Compile times collapse. The code generator no longer materializes whole
aggregates as SSA values and aggregates cross function boundaries by pointer.
On the self-hosting compiler (~7k lines of Aoxn) codegen drops from **32.0s to
~3.3s** (O3 passes 10.9s → 1.8s, instruction selection 21.0s → 1.5s); the
integration suite runs roughly twice as fast. Generated-code speed is
unchanged (struct-copy and array benchmarks stay within noise of their
documented values).

### Changed
- **Aggregate ABI: structs and arrays are passed by pointer.** The callee
  copies the pointee into its own slot (value semantics preserved) and
  aggregate returns use an sret out-pointer — the same convention the
  self-hosted codegen has used since v0.19. `extern def` keeps the plain C
  ABI, since that is the FFI boundary. By-value aggregates forced every call
  site to build and every callee to extract a whole SSA aggregate (the
  self-hosting compiler passes ~500-byte, 40-field state structs), which
  multiplied IR size and made the optimizer and the backend superlinear.
- **Aggregate values are represented by their address** throughout codegen:
  struct literals, array literals, replication temps and aggregate-returning
  calls no longer `load` the whole aggregate as an SSA value; copying stays
  an explicit `memcpy`. Those giant loads/stores were the second half of the
  pathology (instcombine alone: 15s; ISel 14-44s depending on how much the
  pipeline had simplified first).
- Aggregate sizes in `memcpy` are plain integer constants now
  (`LLVMStoreSizeOfType`, with the module data layout established *before* IR
  emission) instead of `LLVMSizeOf`'s `ptrtoint(gep)` constant expressions,
  which every pass had to re-fold.

### Fixed
- **Temp-cache aliasing across functions** (`module verification failed:
  Referring to an instruction in another function!`): struct construction
  cloned its field expressions before emitting them, but the literal/temp
  caches are keyed by AST node address — a temporary clone's address gets
  recycled, so sites in different functions could share one hoisted temp.
  Field expressions are now borrowed.

### Added
- `AOXN_TIME=1` additionally reports codegen sub-phases (`cg.build`,
  `cg.verify`, `cg.target`, `cg.passes`, `cg.isel`); this is what localized
  the pathology.
- `AOXN_PASSES=<pipeline>` overrides the LLVM pass pipeline (compile-time
  experiments and pathological inputs); the default remains `default<O3>`.

## [0.25.0] - 2026-09-26

The self-hosting fixed point is now verified down to the object file, and the
self-hosted codegen's nested-aggregate coverage is pinned by the shared
fixture.

### Added
- **Object-level fixed point**: `selfhost_driver_self_compiles` now also
  compares the COFF object files emitted by the Rust-built compiler and by
  the Aoxn-built (stage-2) compiler for the same program — byte-identical,
  alongside the v0.24 IR-byte comparison.
- **Nested-aggregate coverage in the self-hosting fixture**: the shared
  `STDLIB_USE_PROG` target now exercises 2D arrays (`[[int; 3]; 2]` literals,
  indexing, element assignment, `len`), structs with array-of-array fields,
  sub-array call arguments (`sum_row(g.cells[1])`), array literals passed
  straight to an array parameter (`sum_row([1, 2, 3])`), generic calls on
  literals (`sort([5, 3, 8, 1])[0]`), 2D `for` iteration and value-semantics
  copies of compound structs — verified through both the Rust-built and the
  Aoxn-built driver.

### Changed
- `STDLIB_USE_PROG` is written as a raw string literal (the
  escaped-continuation form had become unreadable at fixture size).

## [0.24.0] - 2026-09-26

Self-hosting reaches the artifact level: the Aoxn-written compiler gains an
IR dump (the self-hosted `aoxn ir`), and the fixed point is now verified on
IR bytes rather than only on program behavior. Also completes the
in-progress B1 `Diag::at` conversion from the v0.23 P2 work.

### Added
- **Self-hosted `aoxn ir`**: `selfhost/codegen.ax` gains `gen_ir_text`
  (`LLVMPrintModuleToString`) and `selfhost/driver.ax` gains
  `emit_ir(src, out)` — the Aoxn-written compiler can now dump a program's
  unoptimized module IR, matching the Rust CLI's `ir` subcommand.
- **Artifact-level self-hosting fixed point**: `selfhost_driver_self_compiles`
  now also compares the IR produced by the compiler built by the Rust
  compiler against the IR produced by the compiler built by the Aoxn compiler
  for the same stdlib program. The two dumps are byte-identical — the
  compiler reproduces itself at the artifact level, not only behaviorally
  (the v0.22 fixed point checked stdout + exit codes only).

### Changed
- `selfhost/driver_stdlib_demo.ax` additionally dumps `stdlib_use.ir` next to
  its product (fixed-point comparison material).

### Fixed
- Completed the in-progress B1 `Diag::at` conversion
  (docs/p2-compiler-performance.md): call sites carried a stray `Diag` prefix
  (`Err(Diag self.err(...))`, `Diag Diag::at(...)`), and 29 sites passed
  `"msg".into()` into `impl Into<String>` parameters, which is ambiguous.

## [0.23.0] - 2026-09-26

P2 "compiler performance & code quality" (docs/p2-compiler-performance.md),
part 1: measurement groundwork, the A-group hot paths, and the lexer
restructure. Batches 3-5 (Diag helpers, giant-function splits, table-driven
builtins, robustness) follow.

### Added
- **`AOXN_TIME=1` stage timing**: per-file `lex`/`parse` and per-phase
  `typecheck`/`codegen`/`link` wall-clock lines on stderr, in the style of
  `AOXN_TC_TRACE`/`AOXN_CG_TRACE` — the measurement base for all further
  compiler-performance work.
- `src/hashing.rs`: FxHash-style hasher (zero dependencies) for the
  compiler's internal lookup tables, whose iteration order is never
  observable; std's SipHash dominated small-map lookups.

### Changed (hot paths)
- **A1 monomorphization clones**: struct layouts are borrowed instead of
  cloned at construction sites (`structs` is a shared reference with the
  checker's lifetime, so the borrow outlives `&mut self` calls); call
  signatures are read piecewise from `sigs` (params/ret no longer deep-copied
  per concrete call); `field_type` returns `&Type`.
- **A2 parser `bump()` takes tokens by move** (`mem::replace`) instead of
  cloning every consumed token — payload strings and f-string token vecs are
  moved, and the f-string token vec is no longer cloned at all; the one error
  path that read a consumed token now peeks before bumping.
- **A3 O(1) lookups**: function-scoped binding tables switched from a
  reversed-linear-scan `Vec` to a `HashMap` (semantics preserved — Aoxn
  bindings are function-scoped and unique); codegen `struct_fields` became a
  per-struct field map, making field access O(1) instead of a linear scan
  per read/assignment; hot tables (scopes, sigs, generics, struct layouts,
  codegen locals/fns/fields) use the fast hasher.
- **A4 f-string interpolations lex in place** over the main char buffer: no
  padded string copy, no per-interpolation char re-collection. A virtual
  open paren disables indent tracking and the `brace_col + 1` column offset
  keeps every interpolation token at its exact source column.
- **A5**: `Gen` borrows `call_map` with a lifetime (the whole-map clone per
  compilation is gone); generic-call routing no longer clones the instance
  name (reading the `&'a` field copies the reference out of `self`); `elif`
  folding moves branches by value instead of cloning conditions/bodies.

### Changed (lexer restructure)
- `lex()` is now a `Lexer` struct with per-category scanners (`scan_word`/
  `scan_number`/`scan_string`/`scan_punct`/`scan_fstring`) and a single
  shared `adv!`/escape-table definition (previously duplicated between the
  main scan and f-string scanning). Interpolation sub-scans share the main
  buffer and scan bound. The hot advance/peek helpers stay macros so debug
  builds keep their speed.

### Fixed
- The `unclosed bracket` lex error now reports both `'('` and `'['` (it
  previously always said `'('`).
- Dead code removed: a constant-empty condition in the `load_u8`/`store_u8`
  arity diagnostic (whose suffix was malformed), and a redundant
  `cur_len_params` clear on the parse-error path.
- Mojibake (`鈥`/`鈫`/`路`) in comments replaced with proper `—`/`→`/`·`
  across `ast.rs`, `lib.rs`, `parser.rs`, `typecheck.rs`, `codegen.rs`.

## [0.22.0] - 2026-09-26

### Added
- **Self-hosting fixed point**: `selfhost/driver_self_demo.ax` — the
  Aoxn-written driver compiles the ENTIRE self-hosting compiler (driver +
  codegen + typecheck + parser + lexer + loader + stdlib, ~7k lines of Aoxn)
  into `target/selfhost_stage2.exe`. The stage-2 compiler then compiles a
  stdlib-importing program and its product's stdout + exit code match the
  Rust compiler's — the Aoxn compiler compiles itself and the product
  behaves identically. Regression test `selfhost_driver_self_compiles`.
- **The self-hosted driver compiles its own front end**:
  `selfhost/driver_frontend_demo.ax` builds `selfhost/lex_demo.ax` and
  `selfhost/parse_demo.ax` (pulling in the Aoxn-written lexer and parser)
  through the Aoxn pipeline; the products' output matches the Rust-compiled
  demos byte for byte. Regression test
  `selfhost_driver_compiles_selfhost_frontend`.
- The Aoxn-written driver compiles the **entire `examples/` suite** (11
  programs: hello, fib, primes, vectors, strings, benchmarks, stdlib_demo)
  with zero failures — arrays of structs, `[e] * N` struct replication,
  string arrays and generic instances all covered.
- `selfhost/driver.ax`: `compile_file_libs(...)` forwards `-l`/`-L` to clang
  (`compile_file` delegates with empty flags); needed to link programs that
  drive LLVM-C through `extern def`.

## [0.21.0] - 2026-09-26

### Added
- **Self-hosted codegen: arrays + raw memory (stdlib bootstrap).** The Aoxn
  code generator now covers the whole `stdlib/stdlib.ax` surface: array
  types/literals, `[e] * N` replication (runtime fill loop), indexing
  read/write, `len(array)`, `for x in arr` (hidden index + element copy),
  array params/returns using the same pointer + sret ABI as structs, arrays
  in struct fields, and the raw memory builtins (`load_i64`/`load_f64`/
  `load_u8`/`store_i64`/`store_f64`/`store_u8`/`as_ptr`/`as_string`).
  Short-circuit `and`/`or` lower to branch + phi like the Rust compiler.
- **The self-hosted driver compiles stdlib programs**: new
  `selfhost/driver_stdlib_demo.ax` + `selfhost_driver_compiles_stdlib` — the
  Aoxn-written pipeline (load -> check -> codegen -> clang) compiles a program
  importing the real `stdlib/stdlib.ax` (generic `sort`/`binary_search`/
  `sum_int`, `Vec`/raw memory, char classes) and the produced exe's stdout +
  exit code match the Rust compiler's.

### Fixed
- **Self-hosted `range(n)` loops emitted a null operand** (module
  verification failure): the single-argument form set `start` and then
  overwrote it with 0, leaving `end` null — only `range(a, b)` had ever been
  exercised by tests.
- **Self-hosted `if`/`elif`/`else` merge blocks** could be left unterminated
  (and a stray branch appended after a terminator) when a branch body ended
  in a nested compound statement; the merge now branches from the real end
  block of each path (`LLVMGetInsertBlock`). Regression covered by the
  stdlib-program test (`binary_search` has exactly this shape).

## [0.20.0] - 2026-09-25

### Added
- **`--cpu <name>` / `AOXN_CPU`**: select the LLVM target CPU (`native`
  enables host-specific SIMD, e.g. AVX2); the default stays generic so
  compiled output remains reproducible across machines.

### Changed
- **Codegen optimization pass.** `int` arithmetic now emits `nsw` and
  array/field GEPs emit `inbounds` — signed overflow and out-of-bounds
  indexing were already undefined by spec (matching C), and the spec text now
  says so explicitly. String lengths are cached: literals, `str()` and
  `as_string()` results record their byte length, and string bindings keep a
  tracked length in a side slot updated at every assignment. `s = s + piece`
  accumulator loops and f-string chains therefore run in O(total bytes)
  instead of rescanning the accumulated string on every `+` (the old O(n²)).
  Cached lengths are dropped when a raw store or an unknown C function could
  mutate string bytes (`invalidate_str_lens`).

### Fixed
- **Nested generic calls failed at codegen** (`internal error: unknown
  callable`): monomorphized instances are now checked as the same AST objects
  that codegen emits, so `call_map` routing by node address also works for
  generic calls *inside* generic instance bodies. Regression test
  `nested_generic_calls`. Side effect: generic bodies are no longer deep
  cloned three times per instance (faster typecheck on generic-heavy code).
- Codegen panic paths (empty array literal, callable lookup) surface as
  `internal` diagnostics instead of panicking.
- `--json` diagnostics escape newlines and control characters (multi-line
  messages previously produced invalid JSON).

## [0.19.0] - 2026-09-12

### Added
- **Self-hosted codegen: structs (value semantics).** Two-phase named LLVM
  struct declarations, field GEP read/write, struct literals filled into
  entry-hoisted temps, `memcpy` copies on binding/assignment, and field
  assignment. Struct parameters are passed as pointers that the callee copies
  into its own local slot; struct returns use an sret out-pointer — this
  avoids by-value aggregate function signatures (which hung LLVM) and scales
  to large structs. `LLVMVoidTypeInContext` is now used for void returns
  (previously a null type ref was passed to `LLVMFunctionType`).
- Codegen demo/test coverage: `dist2(Point, Point)`, a struct-returning
  `origin()`, field assignment, and copy-on-assignment (`s = p` leaves `p`
  untouched) — stdout parity with the Rust compiler.

## [0.18.0] - 2026-09-12

### Added
- **Self-hosted codegen: floats (f64).** The Aoxn code generator now handles
  float literals (`strtod` + `LLVMConstReal`), float locals/params/returns,
  `+ - * /` via `fadd/fsub/fmul/fdiv`, unary `-` via `fneg`, all six ordered
  comparisons (`fcmp` OEQ/UNE/OLT/OLE/OGT/OGE), `print(float)` (`"%f\n"`),
  and `str(float)` via `snprintf("%f")`. The codegen test now covers
  `4.000000`, `2.000000`, `-1.500000`, `f=1.500000`, `half f = 0.750000`
  with exact stdout parity against the Rust compiler.

## [0.17.0] - 2026-09-12

### Added
- **Self-hosting: the driver closes the loop in Aoxn.** `selfhost/driver.ax`
  orchestrates the Aoxn-written stages — import-aware `load_program` -> strict
  `check_all` -> LLVM-C codegen -> object emission -> `system("clang ...")`
  link — producing a native executable from a real `.ax` file. No Rust
  compiler involvement at runtime.
- `selfhost/driver_demo.ax` compiles `hello.ax` end to end; the
  `selfhost_driver_links_hello` test builds the demo, runs it, then executes
  the produced exe and compares its stdout with the Rust compiler's build
  (`hello, Aoxn`). 89 tests green.

## [0.16.0] - 2026-09-12

### Added
- **Self-hosted codegen: strings.** The Aoxn code generator now handles
  string literals/params/returns/locals (opaque pointers), `print(string)`,
  the `len` and `str` builtins (`str(int)` via `snprintf("%lld")`,
  `str(bool)` via a branch + phi over `true`/`false`), `+` concatenation
  (`malloc` + `memcpy` + NUL, never freed), and all six string comparisons
  via `strcmp`. f-strings work end-to-end because the parser already
  desugars them to `"lit" + str(expr) + ...`.
- C runtime declarations (`strlen`, `strcmp`, `malloc`, `memcpy`,
  `snprintf`) are emitted into the module on demand, so a checked program
  need not declare them.
- `selfhost_codegen_int_slice` now covers strings: `hello, aoxn!`, `12`,
  `n=5`, `n squared = 25` — stdout and exit code match the Rust compiler.

### Fixed
- Self-hosted codegen built pointer arithmetic with `LLVMBuildAdd` (invalid
  IR: "Invalid operator", verifier crash); mid/end pointers now use GEP
  byte offsets. Also fixed string bindings being tagged as ints.

## [0.15.0] - 2026-09-12

### Added
- **Self-hosted codegen: `bool` support and `print`.** The Aoxn code
  generator (`selfhost/codegen.ax`) now types locals/params/returns as
  `i64` or `i1` (arena tag -> LLVM type), so `bool` bindings, comparisons
  held in variables, and bool-returning functions work end-to-end.
  `print(int)` calls `printf("%lld\n")`; `print(bool)` branches over the
  static `true`/`false` strings like the Rust codegen. The C entry wrapper
  now sets stdout to binary mode (`_setmode`) for byte-identical newlines.
- `selfhost_codegen_int_slice` now compares **stdout** between the
  self-hosted object and the Rust compiler's build of the same program
  (`41\n24\ntrue\ntrue\n-41\n`, exit code 65) in addition to exit codes.

### Fixed
- Self-hosted codegen passed type-arena *tags* where `ty_ll` expects arena
  *indices*, allocating garbage element types for locals/params (crash).

## [0.14.0] - 2026-09-12

### Added
- **Self-hosting: multi-file import resolution in the Aoxn front end**
  (`selfhost/load.ax`). `load_program(path)` reads a real `.ax` file, resolves
  `import "..."` recursively (paths relative to the importing file), includes
  each file once, rejects cycles with an import stack, and parses all files
  into one shared arena so node indices stay valid across files. Top-level
  nodes are spliced into a single program root (careful `next`-chain
  detaching); `checker_state()` + `check_all` now accept an already-parsed
  state. Verified by `selfhost/load_demo.ax` +
  `selfhost_frontend_handles_imports` (diamond include-once, cycle rejection,
  missing-file rejection, and `examples/stdlib_demo.ax` typechecking as a
  real 2-file program with 10 monomorphized instances).

## [0.13.0] - 2026-09-12

### Added
- **Self-hosting stage 4, first slice: the Aoxn code generator written in
  Aoxn** (`selfhost/codegen.ax`). Drives the LLVM-C API through `extern def`
  and emits a native object file from the self-hosted checker's output:
  int/void functions, int locals, arithmetic and comparisons, `if`/`else`,
  `while`, `for`-range, `break`/`continue`, direct calls and **monomorphized
  generic instances** (routed through the checker's `call_node`/`call_fni`).
  Target setup, `default<O3>`, verification and object emission included.
- `selfhost/codegen_demo.ax`: parses, checks and code-generates a sample
  program (generic `twice`, `fact` recursion, loops) to `selfhost_out.obj`.
- `selfhost_codegen_int_slice` test: builds the demo with `-l LLVM-C`,
  links its object with clang, runs it and compares the exit code with the
  Rust compiler's output for the same source (both 65).

### Fixed
- **Rust codegen literal-temp collision across files** (latent since v0.8
  imports): per-file parse-time literal ids restart at 0, but `lit_temps`
  cached hoisted allocas globally by id — an aggregate literal in one file
  reused an alloca from another function ("Referring to an instruction in
  another function", LLVM abort). Temps/replication iterators are now keyed
  by AST node address (as generic call routing already was). Regression test
  `import_aggregate_literals_across_files`.
- Codegen type hints for unannotated `load_i64`/`load_u8` (int) and
  `load_f64` (float) bindings.

## [0.12.0] - 2026-09-12

### Added
- **Self-hosting stage 3: monomorphization in the Aoxn type checker**
  (`selfhost/typecheck.ax`). Generic declarations are collected with `TY_VAR`
  type parameters and length-`N` arrays; every generic call unifies argument
  types against the declared parameter types, builds a deterministic mangled
  instance name (`id.i`, `first.i.?.3` — same scheme as the Rust compiler),
  clones the declaration's AST with the type parameters and `N` substituted,
  registers the instance as a normal signature, and queues its body for
  checking. Instances are deduplicated (recursive generics terminate) and
  each cloned call site is routed to its instance via `call_node`/`call_fni`.
- Self-hosted front-end exit test: the Aoxn lexer + parser + checker now
  process the **entire `stdlib.ax`** in one program
  (`selfhost_frontend_handles_stdlib`), plus generic accept/reject cases in
  `tycheck_demo.ax`.

### Fixed
- Self-hosted lexer keyword parity: `and` / `or` / `not` now map to `&&` /
  `||` / `!` (as in the Rust lexer) instead of distinct tokens the parser
  did not understand — this blocked parsing any real program using them.
- Self-hosted checker argument indexing for multi-argument builtins
  (`load_u8`, `store_i64`, `store_f64`, `store_u8`): it followed the sibling
  chain of the first argument's *value* instead of the argument list,
  producing bogus "missing expression" errors.
- Self-hosted checker signature collection: extern declarations were
  reported as "malformed function" because the return type was assumed to
  sit before a body block; externs return the last child.
- Self-hosted parser records the length-parameter name on `[T; N]` nodes so
  the checker can substitute `N` inside cloned bodies.

## [0.11.0] - 2026-09-12

### Added
- **Self-hosting stage 2: the Aoxn parser written in Aoxn**
  (`selfhost/parser.ax`, ~1100 lines) — arena AST (tag / sval / ival / child /
  next in parallel Vecs, first-child + next-sibling), full grammar port:
  declarations (import / struct / def+extern, generic headers, array-length
  params), statements (let / annotated let / assignment / if-elif-else folding
  / while / for-range / for-array / break / continue / return / pass),
  precedence-climbing expressions, call arguments, indexing, field access,
  array literals + `[e] * N` replication, and f-string desugaring via
  sub-lexing. Verified by `selfhost/parse_demo.ax` plus an exact AST-dump
  regression test (`selfhost_parser_ast_dump`).
- **Self-hosting stage 3, first slice: the Aoxn type checker written in Aoxn**
  (`selfhost/typecheck.ax`, ~1000 lines) — strict rules ported: struct
  collection (duplicate + cycle detection), signature collection, scope and
  struct tables, all-paths-return / unreachable-code analysis, expression
  typing, builtins, struct literals. Generic functions are reported as
  unsupported for now. Verified by `selfhost/tycheck_demo.ax` (accepts a
  well-typed program, rejects an ill-typed one) plus
  `selfhost_typechecker_accepts_and_rejects`.
- Self-hosted lexer: `;` token for array types, and f-strings now emit the
  raw literal source in the FSTR token so the parser can re-lex
  interpolations (matching the Rust lexer's literal/expr split).

### Fixed
- **Self-hosted parser segfault (the v0.11 WIP known issue).** Helpers such as
  `new_node` mutated a pass-by-value `PState` copy and discarded the write-back
  (`p.n_tag = vec_push(...)`), so the caller's arena Vecs stayed at `data=0`
  and the first `vec_set` stored through NULL. Rewritten in write-back style:
  every mutating function returns the updated `PState`, and multi-value
  results travel through `p.res_node` / `p.res_vec`.
- Self-hosted type checker: same write-back bug in `alloc_ty`; additionally
  the type arena's `t_elem` / `t_len` / `t_sname` Vecs were index-misaligned
  (seeded with a dummy slot while `t_tag` was not), making `ty_sname` return
  the wrong struct names. Fixed; struct-field counting no longer includes the
  reservation rows.
- Codegen: an unannotated binding of `as_string(...)` / `as_ptr(...)`
  (`x = as_string(p)`) failed with "unknown call in type hint"; both builtins
  now carry type hints (`string` / `int`).

## [0.10.0] - 2026-09-06

### Changed
- **Project renamed: Axon → Aoxn** (crate, binary, docs, examples).

### Added
- **Self-hosting stage 1: the Aoxn lexer written in Aoxn**
  (`selfhost/lexer.ax`, ~770 lines) — a faithful port of the Rust lexer:
  Python-style layout (NEWLINE/INDENT/DEDENT with an indent stack), comments,
  paren continuation, all operators, string escapes, floats, f-string raw
  tokens. Verified by an exact token-stream test (`selfhost/lex_demo.ax`).
- stdlib: `vec_pop`.

### Fixed
- Struct cycle detection: a struct appearing in multiple sibling fields was
  falsely reported as recursive; replaced with proper gray/black DFS.
- Runtime crash in the demo pipeline: an empty indent stack (seed value
  missing) made `vec_get(stack, -1)` dereference NULL.

## [0.9.0] - 2026-09-06

### Added
- **Raw memory builtins** (self-hosting foundation): `load_i64`/`store_i64`,
  `load_f64`/`store_f64`, `load_u8`/`store_u8` (byte access on `int`
  addresses and on `string` bytes), and pointer reinterpretation
  `as_string`/`as_ptr`. Addresses are plain `int`; unsafe by design.
- stdlib: growable `Vec` (8-byte slots, write-back style: `v = vec_push(v, x)`,
  `vec_get`/`vec_set`/`vec_free`), byte buffers (`buf_new`, `fill_zero`),
  string byte access (`str_get`), char classification (`is_digit`, `is_alpha`,
  `is_space`), file IO (`read_file`, `write_file` via FFI), and `system(cmd)`
  for process spawning.

### Fixed
- Lexer: `>=` was lexed as `>` (a latent bug no earlier test caught 鈥?  `4 >= 5` is false under both). Added boundary-equality regression tests.

## [0.8.1] - 2026-09-06

### Added
- `-l NAME` / `-L DIR` link flags: user programs can link arbitrary C
  libraries (LLVM-C, crypto, ...).
- Self-hosting proof of concept: `examples/ffi_llvm.ax` drives the LLVM-C
  API from Aoxn (pointers pass as `int`, ABI-identical on x86-64) and emits
  real IR 鈥?`define i64 @answer() { ret i64 42 }`.
- Self-hosting feasibility assessment: `docs/selfhost.md` (capability
  matrix, gap analysis, staged bootstrap plan, verdict: feasible).

## [0.8.0] - 2026-09-06

### Added
- **Import / module system**: `import "../stdlib/stdlib.ax"` 鈥?paths resolve
  relative to the importing file; include-once per canonical path; circular
  imports rejected with the full cycle chain.
- **File-aware diagnostics**: every error now names its source file
  (`[type] stdlib/stdlib.ax:130:20: ...`, also in `--json` output).
- `Aoxn build / run / ir` are now import-aware 鈥?`Aoxn run examples\stdlib_demo.ax`
  alone pulls in the standard library.

## [0.7.0] - 2026-09-06

### Added
- **Generic functions**: `def sort[T, N](arr: [T; N]) -> [T; N]` 鈥?type
  parameters `T` and array-length parameters `N`, monomorphized at every call
  site (deterministic mangled instances like `sort.i.8`).
- The length parameter is usable as an `int` constant inside generic bodies
  (`range(N)`, `N - 1`).
- Standard library rewritten with generics: `sort`, `linear_search`,
  `binary_search`, `max_of`, `min_of`, `reverse`, `sum_int`, `sum_float` 鈥?  replacing the fixed-size `_8` functions.

### Fixed
- Generic declarations no longer reach codegen (a `[T; usize::MAX]` type
  crashed LLVM's array-type construction).

## [0.6.0] - 2026-09-06

### Added
- `extern def` 鈥?C FFI declarations (bodyless, resolved at link time).
- Multi-file compilation: `Aoxn build main.ax stdlib/stdlib.ax` merges all
  inputs into one namespace (build / run / ir).
- `stdlib/stdlib.ax` 鈥?the standard library written in Aoxn itself: math
  (`abs/min/max/clamp/pow_i/gcd/lcm/isqrt/is_prime/hypot` + `sqrt/floor/ceil`
  via FFI), search, sort.
- `examples/stdlib_demo.ax`.

## [0.5.0] - 2026-09-06

### Added
- `for` loops: `range(n)`, `range(a, b)`, `range(a, b, step)` (negative step
  supported), and array iteration (`for x in arr`). start/end/step evaluated
  once at loop entry (Python semantics).
- `break` / `continue`.
- f-strings: `f"hello {name}, {1 + 2}"` with `{{`/`}}` escapes and arbitrary
  expressions; desugars to `"lit" + str(expr) + ...`.
- `str()` builtin (int 鈫?decimal, float 鈫?`%f`, bool 鈫?`true`/`false`).

### Added (CI)
- GitHub Actions: windows-latest, winget LLVM, full test suite + smoke test.

## [0.4.0] - 2026-09-05

### Added
- String operations: `+` concatenation (runtime malloc + memcpy + NUL), all
  six comparison operators (byte-wise `strcmp`), `len(str)` (bytes).
- Strings in struct fields, array elements, parameters, and returns.
- C runtime functions (malloc/strlen/strcmp/snprintf) declared lazily.

### Design
- Strings are immutable; concatenation results are heap-allocated and never
  freed (no GC yet 鈥?documented behavior).

## [0.3.0] - 2026-09-05

### Added
- Fixed-size arrays `[T; N]`: literals, unchecked indexing, `[e] * N`
  replication (element evaluated once, runtime fill loop).
- Structs: Python-style indented fields, named-field construction
  (`Point(x=1, y=2)`), field access/assignment, nesting, forward references,
  recursion rejected.
- Value semantics: assignment/params/returns copy whole aggregates (memcpy).

### Fixed
- Struct GEP field indices must be i32 constants (LangRef rule).
- Large aggregates are never materialized as SSA values 鈥?`load [50000 x i64]`
  made SROA/O3 hang; assignment now goes through memcpy.

## [0.2.0] - 2026-09-05

### Changed
- Python-style surface syntax: indentation-delimited blocks, `def` / `elif` /
  `pass`, `#` comments, no braces or semicolons, `//` is integer division.
- Lexer emits NEWLINE / INDENT / DEDENT; blank and comment-only lines produce
  no tokens; newlines inside parentheses ignored (implicit line joining).
- `and` / `or` / `not` and `True` / `False` as aliases of the symbolic forms.
- Bindings: `x = 5` infers, `x: int = 5` checks, re-assignment keeps the type.

### Removed
- `fn` / `let` keywords, braces, semicolons, block comments.

## [0.1.0] - 2026-09-05

### Added
- Initial compiler in Rust with zero external crates: hand-written LLVM-C FFI
  (no inkwell/llvm-sys), pipeline lexer 鈫?parser 鈫?strict typecheck 鈫?LLVM O3
  鈫?object file 鈫?clang link.
- Primitives (int/float/bool/string), functions (mutual recursion), control
  flow, `print` builtin.
- `Aoxn build / run / ir` CLI with `--json` diagnostics for AI agents.
- Benchmarks: parity with `clang -O3` on identical algorithms.

