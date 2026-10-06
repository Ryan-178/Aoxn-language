# Platform support

**Aoxn's first-class platform is Windows x86_64. Linux x86_64 is supported
for the compiler, the standard library and the X11 UI backend as of
v0.46.0.**

v0.30.0 cut the platform list down to Windows alone (one installer, one CI
job, one windowing backend). v0.46.0 brings Linux back as a supported
target — the compiler, the whole test suite and an X11 UI backend
rewritten against the v3 `plat_*` contract all run on it — while Windows
keeps everything that is Windows-specific: the installer, the release
packaging and the Win32/GDI UI backend. macOS remains unsupported (the
Cocoa surface needs struct-by-value externs the language cannot express;
see `docs/ui.md`).

## What ships where

| | Windows (first-class) | Linux (v0.46.0) |
|---|---|---|
| Compiler | `aoxn.exe` | `aoxn` (ELF objects, `-lm` linked automatically) |
| Installer | `Aoxn-<version>-Setup.exe` | none — build from source |
| Standard library | full, including `stdlib/net` (WinHTTP) | full **except** the `net/http` + `openai` transports (honest runtime error) |
| UI backend | Win32/GDI (`stdlib/ui_win.ax`) | Xlib + Xft (`stdlib/ui_x11.ax`) |
| UI link flags | `-l user32 -l gdi32` | `-l X11 -l Xft` |
| CI | `windows-latest`, whole suite | `ubuntu-latest`, whole suite + Xvfb probe |
| External tools | clang + MSVC Build Tools | clang (distro packages) |

## The invariants that make both platforms one codebase

- **`target_os()` folds the BUILD host's OS.** `"windows"` on a Windows
  build, `"linux"` on a Linux build (v0.46.0), `"other"` elsewhere. The
  self-hosted compiler has no platform check of its own:
  `selfhost/codegen.ax` folds `target_os()` by calling the builtin, whose
  value was burned in by the compiler that built it — so every stage of
  the fixed-point chain agrees, and the fixed point (both stages built
  and run on the same machine) stays byte-identical by construction.
- **The stdlib branches, it does not fork.** `os.ax`, `stdlib.ax`
  (`exe_path`/`asset_path`) and the UI backends select their platform
  code with `if target_os() == "windows"` at runtime. Every extern for
  both platforms is declared in both files; an unreferenced `extern def`
  produces no symbol reference in the emitted C, so the Windows build
  never links Xlib and the Linux build never links kernel32.
- **`stack_link_flag()`/`default_link_libs()`** (`src/platform.rs`):
  Windows links `-Wl,/STACK:8388608` and folds C math into the CRT;
  Linux passes no stack flag (RLIMIT_STACK governs) and adds `-lm`.
- **The installer stays Windows-only.** `src/setup/` is cfg-gated: on
  Linux the `aoxn-setup` binary builds as a stub that refuses to run.
  Linux users install from source (`cargo build`).

## What v0.46.0 brought back

| Restored | Notes |
|---|---|
| `stdlib/ui_x11.ax` | rewritten against the v3 `plat_*` contract: Xft text, selection-based clipboard, event-driven mouse, keymap-swept keyboard state, slot-824 close request |
| `examples/ui_probe_x11.ax` | 30-frame smoke probe, run under Xvfb in CI |
| The Linux CI job | whole suite + Xvfb probe (`apt install clang libx11-dev libxft-dev libfontconfig1-dev xvfb`) |
| POSIX link behavior | `-lm` at link time; no `/STACK` flag; `.o`/no-extension artifacts |

## Still Windows-only (deliberately)

- **`net/http` + `openai` transports** — the HTTP layer rides WinHTTP
  (TLS, proxies, redirects for free). There is no TLS stack in the
  language to replace it on Linux; the entry points return a clean
  "no transport on this platform" error instead of failing to link.
- **The installer / `aoxn-setup`** — one artifact, Windows packaging.
- **The web benchmark suite's server** (`web/`) — its sockets speak
  ws2_32; a POSIX socket module is future work, not a port.
- **macOS** — no Cocoa backend is expressible (struct-by-value externs);
  nothing changed since the v0.30.0 removal.

## History

- **v0.30.0 (2026-10-02)**: platform list cut to Windows alone; the
  v0.29-era X11 backend, POSIX sockets, POSIX link flags and the
  Linux/macOS CI jobs were removed rather than left to rot.
- **v0.46.0 (2026-10-06)**: Linux returns as a supported target —
  compiler + stdlib + suite green on ubuntu-latest, and the UI toolkit
  gains a second backend through the same `plat_*` contract that kept
  `ui_draw.ax` portable all along.
