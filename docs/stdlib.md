# The Aoxn standard library (v0.44.0)

The v0.44.0 batch landed the first thirteen modules of the stdlib roadmap
(`docs/stdlib-todo.md` §6's items 1–6 plus the bisect/heapq pair): every
module is its own file under `stdlib/`, imported BY NAME —
`import * from "stdlib/time"` — so none of them touched
`stdlib/stdlib.ax` (the self-host critical path) or broke the fixed point.
This file is the reference for the batch; the roadmap that produced it lives
in `docs/stdlib-todo.md`.

All modules are **plain Aoxn** except `hmac.ax` (function pointers, marked
`selfhost 尚不可编译` in its header). No module needs a `-l` flag: the Win32
APIs used (kernel32 time/filesystem) and the CRT math functions all link
through the default set, and Linux links libc (plus the compiler's `-lm`).

## The one-platform rule, and how a module obeys it

An `extern def` a program CALLS is a symbol reference in the object file,
whatever the rest of the source says. So a stdlib module that names one
platform's API and nothing else simply does not link on the other one — it
fails at `lld-link`/`ld` with `undefined reference to 'GetLocalTime'` and no
diagnostic pointing at the cause.

The idiom that fixes it (`os.ax` first, v0.46.0; `time.ax` / `datetime.ax`
joined in v0.50.1): **declare every platform's externs, and branch on
`target_os()` in every function that calls one.** The backend emits

```c
if (((__builtin_strcmp("windows", "linux") == 0))) { ... }
```

which clang constant-folds *before* code generation, so the branch that is
not taken never becomes a call and its symbols never reach the object file.
That holds at `-O0` as well as `-O3` — `stdlib/os.ax` relies on it and the
v0.50.1 test reads the object file's undefined-symbol table to prove it.

**The guard goes in the function that makes the call, not only in its
caller.** The compiler emits *every* function body, so a shared POSIX
helper reached from a dead branch still drags `clock_gettime` into a Windows
link. That is the shape of the bug v0.50.1 fixed.

| module | file | one-liner |
|---|---|---|
| `math` | `stdlib/math.ax` | CRT trig/exp externs + the NaN/Inf toolkit |
| `time` | `stdlib/time.ax` | QPC/`clock_gettime` monotonic clock, UTC wall clock normalised to the 1970 origin |
| `datetime` | `stdlib/datetime.ax` | civil calendar math + strftime subset |
| `calendar` | `stdlib/calendar.ax` | month ranges, grids, weekday queries |
| `pathlib` | `stdlib/pathlib.ax` | pure path string operations |
| `base64` | `stdlib/base64.ax` | RFC 4648 encode **and** decode, std + URL-safe |
| `hashlib` | `stdlib/hashlib.ax` | SHA-256 / SHA-1 / MD5, one-shot + incremental |
| `hmac` | `stdlib/hmac.ax` | RFC 2104 over the hashlib one-shots (fn-ptr) |
| `os` | `stdlib/os.ax` | filesystem core, env, cwd, listdir (Win32 + POSIX) |
| `glob` | `stdlib/glob.ax` | shell wildcards over `os_listdir` |
| `json` | `stdlib/json.ax` | the JSON DOM (promoted from `net/`) + file I/O |
| `bisect` | `stdlib/bisect.ax` | binary search + sorted insertion on `Vec` |
| `heapq` | `stdlib/heapq.ax` | min-heap over `Vec` |

Demos: `examples/{math,time,pathlib,hashlib,os,json,bisect}_demo.ax`.
Tests: `tests/{math,pathlib,datetime,hashlib,osglob,containers,json_fileio}.rs`
(each embeds a driver that prints `PASS` markers; the Rust side asserts the
marker list and `DONE fails=0`).

## math

Python's `math` spelling for the CRT functions: `sin cos tan asin acos atan
atan2 exp log log2 log10 pow fmod trunc`, plus `degrees`/`radians`,
`lerp`, `remap` and `copysign_f`. `sqrt`/`floor`/`ceil`/`hypot`/`abs_f`
stay in `stdlib.ax` (imported transitively; nothing is redefined).

**The NaN/Inf toolkit (stdlib-todo §0.6)** — the language has no
`inf`/`nan` literals, no float bitcasting, and emits float division raw, so:

- `fdiv(a, b)` is THE division for numeric code: IEEE-correct for zero
  denominators (NaN for 0/0, ±Inf otherwise) where `/` is C UB.
- `NaN()`, `Inf()`, `NInf()` are the only sanctioned non-finite sources
  (`log(-1.0)` and `log(0.0)` under the hood).
- `is_nan` / `is_inf` / `is_fin` classify without touching UB.

The capitalized spellings are load-bearing: a lowercase `nan`/`inf`/
`copysign` **definition** collides with the UCRT's own symbols at link time
(the math externs share object files with them — `lld-link: duplicate
symbol`). Never rename them to Python's casing.

Zero-argument "constants" follow the no-module-bindings rule: `pi()`,
`e()`, `tau()` are functions.

## time

Three clocks, three purposes:

- `time_now_ns()` / `time_now_us()` — monotonic. Windows reads
  QueryPerformanceCounter (the seconds and the remainder are converted
  separately, because the naive `c * 1e9 / f` overflows i64 after ~292 days
  at 10 MHz); Linux reads `clock_gettime(CLOCK_MONOTONIC)`.
- `time_unix()` / `time_unix_ms()` / `time_unix_ft()` — wall clock UTC.
  Windows reads `GetSystemTimeAsFileTime`, whose raw value counts 100 ns
  units from **1601**; `time_unix_ft` **subtracts** the epoch gap, so what
  it returns is 100 ns ticks from **1970**. Linux reads
  `clock_gettime(CLOCK_REALTIME)`, which counts from 1970 already — so the
  POSIX branch subtracts nothing. **Both sides answer on the 1970 origin,
  which is the whole contract of `time_unix_ft`** (everything above divides
  it). Reading the name the other way round and "converting" the POSIX clock
  *into* FILETIME shape costs exactly the gap: v0.50.1 did that and the
  module answered 369 years in the future (`time_unix` = 13436106024
  instead of 1791632424). Note that `time-unix-ms-agrees` could not see it —
  it compares two readings of the same clock and is blind to a constant
  offset; `time-unix-sane` is the absolute check that did.
- `time_mono_ms()` — cheap monotonic milliseconds since boot
  (`GetTickCount64` / the monotonic clock divided down).
- `time_sleep(sec: float)` / `time_sleep_ms(ms: int)` — `Sleep` on Windows,
  `nanosleep` on Linux.

`timespec_get` does NOT link on the Windows toolchain (AGENTS.md); that is why
everything there goes through Win32 externs. kernel32 needs no `-l`, and
glibc ≥ 2.17 carries `clock_gettime`/`nanosleep` in libc (not the old librt),
so Linux needs none either.

## datetime

`struct DateTime{year, month, day, hour, minute, second}` plus pure
calendar arithmetic — Howard Hinnant's `days_from_civil` /
`civil_from_days`, with every possibly-negative division routed through
`floor_div` (C `/` truncates toward zero and would corrupt pre-1970 dates;
`datetime_from_unix(-1)` is 1969-12-31 23:59:59 and tested).

- `datetime_now()` — LOCAL time: `GetLocalTime` on Windows, `localtime_r`
  over `time_unix()` on Linux (the glibc `struct tm` offsets are documented
  in the source; a refused conversion falls back to UTC rather than
  crashing). `datetime_utcnow()` — UTC on both.
- `datetime_from_unix` / `datetime_to_unix` — exact inverses.
- `datetime_weekday` — Monday == 0 .. Sunday == 6 (Python), plus
  `datetime_isoweekday`; `datetime_is_leap`, `datetime_days_in_month`,
  `datetime_yday`.
- `datetime_format(dt, fmt)` — the strftime subset `%Y %y %m %d %H %M %S
  %j %A %a %B %b %%`, parsed by hand (the CRT's `wcsftime` is
  locale-dependent; the todo forbids it). `datetime_iso` /
  `datetime_iso_date` are the two fixed spellings.

## calendar

Built on datetime: `cal_monthrange(y, m) -> CalRange{wday, days}`,
`cal_weekday(y, m, d)`, `cal_is_leap`, `cal_monthcalendar(y, m) -> VecVec`
(6×7 day numbers, 0 = out of month) and `cal_month(y, m)` — the text grid,
Monday-first like Python's default.

## pathlib

Pure string operations, no filesystem access (that is `os.ax`): `path_name`
`path_parent` `path_stem` `path_suffix` `path_join` `path_norm`
`path_is_abs` `path_with_suffix` `path_with_name` `path_split`, plus
**`path_sep()` / `path_sep_byte()`** — the platform's own separator. Both
separators are recognized on **input** everywhere; what a function **builds**
is platform-native, and that asymmetry is deliberate: a joined path has to
name something, and `os_exists("sandbox\\sub")` is false on Linux.
`path_join` inserts `path_sep()` and defers to an absolute right side;
`path_norm` collapses separators and resolves `.`/`..` lexically (no symlink
awareness — same as Python's PurePath) and keeps the root, spelled the
platform's way (`/` stays `/` on Linux; the `\\server\share` UNC form exists
only on Windows); a leading dot does not make a suffix (`.gitignore` has
none).

## base64

`b64_encode` / `b64_encode_str` (standard alphabet, `=` padding),
`b64_encode_url` / `b64_encode_url_str` (`-_`), and the decode direction the
SDK codec never had: `b64_decode(s) -> B64{p, n}` — a pointer+length because
decoded bytes may contain NUL and a string cannot (`len()` is strlen). `n
== -1` marks invalid input. Decode is liberal in exactly one way: both
alphabets are accepted everywhere; padding is optional; a single orphan
character (length % 4 == 1) and trailing garbage after `=` are rejected.
`b64_decode_str` is the text convenience (NUL truncation documented).

**Invalid input returns `n == -1` AND `p == 0`** (v0.48.0). The rejection
paths used to hand back a buffer the caller had no way to release, and the
length of a base64 body is chosen by whoever sent it — an `Authorization`
header, a cookie, an SSE line — so that leaked on a security boundary.

Both directions are O(n). `len()` on a string is `strlen` emitted inline at
every use site, so a decode loop bounded by `len(s)` re-scans the whole input
every iteration; the loop takes a hoisted `int` instead. The old spelling was
quadratic: 64 KB took 608 ms and 1.4 MB did not finish inside 600 s.

`stdlib/net/codec.ax` keeps `net_b64_encode_*` for the SDKs; it predates
this module and does not move.

## hashlib

SHA-256, SHA-1 and MD5 as pure Aoxn over the v0.38.0 bitwise operators.
Each digest ships a one-shot (`sha256(data, len) -> hex`, `sha256_str(s)`,
plus `sha256_raw(data, len, out)` writing raw bytes for HMAC) and an
incremental context (`sha256_new` / `sha256_update` (write-back) /
`sha256_final` / `sha256_digest`). Pinned by the FIPS 180 / RFC 1321
vectors including the >55-byte multi-block case, and incremental == one-shot
across chunk boundaries.

uint32-on-i64 rules (see the module header): mask every add/rotate back
under 2^32 so `>>` stays logical, and rotate via `hash_rotr`/`hash_rol`
which mask BEFORE the left shift (a 32-bit value shifted left 32 would
overflow i64). The context block is 784 bytes — the schedule scratch holds
**80** words because SHA-1 needs them; 64 was the heap-corruption bug.

## hmac

`hmac_sha256` / `hmac_sha1` / `hmac_md5(key, msg) -> hex`, one
implementation (`hmac_bytes`) driven by a **function pointer** to the
`*_raw` one-shots — the only module in the batch that uses a v0.40.0
feature, hence `selfhost 尚不可编译` in its header. Keys longer than the 64-
byte block are hashed first (RFC 2104); binary keys/messages use the
`(ptr, len)` core directly because strings cannot carry NUL. Verified
against the RFC 4231 vectors and a 131-byte 0xaa key cross-checked against
node:crypto.

## os

The Win32 core, everything through the `W` APIs with UTF-8 ↔ UTF-16
conversion (the `os_to_wide`/`os_from_wide` helpers are deliberately
DUPLICATED from `net/codec.ax` — a filesystem module should not pull the
network layer; keep the copies in sync):

- dirs/files: `os_mkdir` `os_rmdir` `os_remove` `os_copy(src, dst,
  fail_if_exists)` `os_rename`
- attributes: `os_exists` `os_isdir` `os_isfile` `os_islink` (reparse
  points — recursive walkers MUST check this before descending),
  `os_attributes` returns the raw DWORD or `OS_ATTR_INVALID()` =
  **4294967295** (the zero-extension trap: a 32-bit `int` return arrives
  zero-extended, so the sentinel is compared unsigned, not against -1)
- env: `os_getenv` (two-call Win32 sizing: NULL buffer for the required
  size, then allocate and read — a fixed buffer silently truncated longer
  values, and the CI runners' PATH is longer than 2048 chars, which is how
  v0.44.1 caught it) `os_has_env` (distinguishes empty from unset via
  `GetLastError == 203`, and **primes it with `SetLastError(0)` first** —
  Win32's last error is thread-sticky and a successful call does NOT clear
  it, so any earlier miss, including `os_getenv`'s own probe of an absent
  name, used to leave 203 latched and make the next probe of a variable that
  really exists answer `False`) `os_setenv` `os_unsetenv` (deletes via a NULL
  value — the empty string would create an empty variable)
- cwd: `os_cwd` `os_chdir`
- `os_listdir(path) -> Vec` — names without `.`/`..`, discovery order;
  the traversal primitive `glob.ax` builds on.

## glob

`glob_match(pattern, name)` — the single-segment backtracking matcher
(`*`, `?`, `[a-z]`/`[!a-z]`/`[^a-z]`, unterminated classes literal) — and
`glob(pattern) -> Vec` — the sorted, deterministic directory walk over
`os_listdir`. Python parity kept: `*`/`?` do not match a leading `.` unless
the pattern segment starts with one; no-wildcard segments are only
descended when they exist; `C:`/UNC roots are preserved. NOT supported:
`**` cross-segment recursion (documented gap). All walk recursion returns
the grown Vec (the write-back discipline).

**Results are spelled the platform's way.** A pattern accepts either
separator, but every path the walk BUILDS goes through `path_sep()`, roots
included — so on Linux `glob("sandbox/sub/*.txt")` returns `sandbox/sub/c.txt`
and that string can be handed straight to `os_*`. v0.50.3 had a literal
`"\\"` in `glob_join` and in the two root branches, which made a nested
pattern walk `os_exists("sandbox\\sub")`, find nothing, and return an empty
result on Linux while passing on Windows.

## json

The JSON DOM (40-byte tagged slab, parse/dumps/builders — see the module
header) promoted from `stdlib/net/json.ax` to `stdlib/json.ax` in v0.44.0;
the OpenAI and Anthropic SDKs import this exact file at its new home. The
promotion's addition is the file pair:

- `j_read_file(path) -> JParse` — `err`: 0 ok, 1 parse error, **2 I/O
  failure** (a missing file upgrades the empty-document parse error via an
  fopen probe; a readable-but-EMPTY file is still err 1).
- `j_write_file(path, dom, i) -> bool` — `j_dumps` to disk.

### A parsed node can be extended — and how not to break it (v0.48.0)

The parser ADOPTS a `Vec`'s child buffer into the node's `ptr_a`/`ptr_b`, so
the block is really `Vec.cap` slots wide, but the node's capacity slot is
left at 0. `jb_set`/`jb_push` on a parsed object or array therefore grow it,
and the grow must **floor the new capacity at the live child count** — never
at a "default" read from a 0. The old code doubled 0→8→16 and reallocated a
20-member object's 256-byte buffer DOWN to 128 before writing slot 20, so any
parsed container with **16+ members** died with `0xC0000005` on its first
`jb_set`. That is exactly the `jb_set_raw` splice the SDKs use for tool
schemas, which is why it survived: the SDK tests only ever built fresh nodes.
`tests/stdlib_defect_pins.rs` is the pin.

### Both directions are linear now, and that was not free

Two loops in this module were quadratic, and one of them was a leak:

- **`j_at`** tested end-of-input with `len(p.src)`. `len()` on a string is
  `strlen` emitted inline at every use site, so every character of every
  token re-scanned the remaining document. `JParse` now carries the length in
  an `sn` field, computed once in `j_parse`/`j_parse_into`. Measured at
  `-O3`: 266 KB went from 7064 ms to 7 ms.
- **`jp_str`** allocated the whole document for EVERY string. Safe (escapes
  only shrink, so the decoded form always fits) but never freed by design, so
  a 330 KB document with 20 000 strings pushed 6.29 GB of allocator traffic
  through a 325.8 MB peak. It is now sized from the remaining input and grown
  geometrically — same document, 4.8 MB.
- **`j_dumps`** built its result by repeated `out = out + ...`. Concatenation
  allocates a fresh buffer per step and abandons the old one, so an n-element
  container cost n(n+1)/2 allocations with every intermediate leaked: a
  20 000-key response touched 8627.8 MB and 28.4 s. It is now one pass into
  `JOut`, a byte sink that grows geometrically, threaded by value (there is
  no address-of, so the cursor is handed back rather than pointed at).
  `j_quote` is now a pre-reserved wrapper over `jw_quote`, so the builder
  still routes through ONE escaping implementation. Round trips stay
  byte-identical.

`j_num_float` also clamps its exponent at `J_MAX_EXP() = 400`. The digit run
used to be unbounded, so `1e999999999` spun the scaling loop for minutes — a
reachable DoS on any untrusted JSON — and the accumulator could wrap past i64
into a *small* exponent. 400 is past the double range, so the clamped answer
is still the IEEE one (inf, or 0.0 for a large negative).

## bisect

Binary search and sorted insertion over an int `Vec`:
`bisect_left`/`bisect_right` return the insertion position (Python
semantics), `insort_left`/`insort_right` return the NEW Vec handle
(`vec_reserve` may have reallocated — the write-back rule), `bisect_find`
returns -1 when absent. The `_f` family holds f64 elements in the same
8-byte slots via `store_f64`/`load_f64`. No string variants: a string slot
would compare POINTERS, not bytes.

## heapq

A binary min-heap over an int `Vec`, Python's shape:
`heap_push(h, x) -> Heap` (handle may have realloc'd),
`heap_pop(h) -> Heap` (the value lands in `h.res` — the struct carries a
`res` slot because Aoxn returns one value), `heap_peek`, `heap_len`,
`heap_from_vec` (heapify in place), `heap_sorted` (full drain into an
ascending Vec). Ints only; `bisect.ax` shows the float pattern.

## The two string rules (v0.50.0)

Two audit families, both now documented as rules. Every defect was
REPRODUCED (compiled probes at `-O3`, before/after wall clock) before the fix;
`tests/stdlib_perf_pins.rs` pins the rewrites byte-for-byte.

**1. `len()` on a string is `strlen`, emitted inline at every use site.**
A loop that only READS its string gets that strlen hoisted out by clang as
loop-invariant — `css_has_quote` was always fast, and looking at it proves
nothing. A loop that WRITES through another pointer, or calls anything
opaque, does NOT: clang cannot prove the store does not alias the string,
so the string is re-scanned once per character. The write-pointer traps
found and hoisted: `os_utf16_write` / `net_utf16_write` (the twins of
`ui.ax`'s `utf16_write`, fixed in v0.49.0 in one copy only — every `os_*`
path call and every WinHTTP wide string went through the other two),
`net_url_encode`, `net_url_split`, `net_ieq`, `glob_match_at`,
`glob_class_end`, `glob_has_wild`, `glob_split`, `path_norm`,
`j_num_float`, `jp_num`, `str_sub` (its clamp asked twice), and
`css_has_quote` (hoisted in the SOURCE so it does not depend on the
optimizer's aliasing proof). Measured on this machine: `os_utf16_write`
1950 → 10 µs per 4096-char call, `net_to_wide` 1959 → 9 µs,
`net_url_encode` 141 → 9 µs per 1024.

`jp_num` additionally extracted each number's text with
`str_sub(p.src, start, p.pos)`, paying a whole-document strlen per number
(20 000 numbers: 1 s). The span is already known, so it is now
`buf_str(as_ptr(p.src) + start, p.pos - start)` — **`buf_str(p, n)` is the
pointer-plus-length escape**: copy exactly n bytes, scan nothing.
`net_sse_next` uses it per SSE line, where `str_sub(as_string(s.buf), …)`
used to `strlen` the whole network buffer per line.

**2. Never grow a string in a loop with `out = out + …`.** Concatenation
allocates a fresh buffer per step and abandons the old one: O(n²)
allocations and O(n²) leaked bytes (strings leak by design — that is the
contract, the waste is not). Rewritten into byte sinks: `an_msgs_json`,
`an_blocks_json`, `an_msg_text`, `an_msg_thinking`, `oa_body_chat`,
`oa_body_embeddings`, `oa_responses_text`, `datetime_format`,
`net_url_encode_pairs`, the `path_norm` join — 200 messages of 8 KB went
from 2105 ms to ~3 ms. The SDKs route through `json.ax`'s existing `JOut`
(one escaping implementation); `datetime` and `pathlib` use `StrBuf`
(`sb_new`/`sb_byte`/`sb_str`/`sb_text`, threaded by value like `Vec`)
rather than pulling the JSON DOM — and through it `net/codec.ax` — into
every program that formats a date. **Output is unchanged; every rewrite is
pinned byte-identical.**

**The trap the rewrites exposed:** a sink that may stay EMPTY must not end
with `store_u8(w.buf, w.n, 0)`. `JOut(buf=0, n=0, cap=0)` has a NULL
buffer until the first append, so a response with no text block — a pure
tool call, an empty accumulator — wrote its final NUL through NULL and
died later with `0xC0000005`. `an_msg_text`, `an_msg_thinking` and
`oa_responses_text` now return `""`; `msg-text-empty` is the pin. The
other sinks always write a literal first byte (`[`, `{"model":`), and
`sb_new`/`path_norm` allocate up front — those are safe by construction,
and the comment in each rewrite says which case it is.

## Vec[T] status (stdlib-todo §0.3)

The "generic heap container" that `collections`/`itertools` were waiting
for is resolved in v0.44.0 as a SLOT-VIEW discipline rather than a new
type: a `Vec` slot is 8 bytes — int/bool store natively, f64 through the
`store_f64`/`load_f64` views (bisect/heapq/bisect's `_f` family are the
worked examples), strings through `as_ptr` (`vec_push_str`/`vec_get_str`
already existed). Generic STRUCT containers remain impossible (no
address-of-struct, no `sizeof`), so heterogeneous records still go through
the tagged-slab pattern (`stdlib/json.ax`); `collections`/`itertools` (P2)
can now build on the slot views.
