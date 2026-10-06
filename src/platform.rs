//! Platform abstraction: the OS-specific decisions that used to be scattered
//! as `cfg!(windows)` branches across `lib.rs` / `main.rs` / `codegen_c.rs`.
//!
//! v0.46.0: Linux support is back. Windows remains the FIRST-CLASS platform
//! (the installer, the Win32/GDI UI backend, the release packaging), but the
//! compiler itself now also builds and runs on Linux: ELF objects, no
//! `/STACK` link flag, `-lm` for the C math library, and the `target_os()`
//! builtin folding to `"linux"`. `selfhost/codegen.ax` needs NO mirror change
//! for that fold: its `emit_builtin("target_os")` calls `target_os()` (the
//! builtin), whose value was burned in by whatever compiler built it — so
//! every stage folds the OS it was BUILT on, and the fixed point (stage 1 and
//! stage 2 on the same machine) stays byte-identical by construction.
//!
//! Since v0.29.0 this module carries no LLVM surface: the compiler has no
//! LLVM dependency (the C-emitting backend + clang replaced it) and the
//! `llvm_dir_candidates`/`llvm_link_name*` probes were removed with it.

/// Executable file extension: `.exe` on Windows, empty elsewhere.
pub fn exe_ext() -> &'static str {
    if cfg!(windows) {
        "exe"
    } else {
        ""
    }
}

/// Object file extension: `.obj` on Windows (MSVC convention), `.o` elsewhere
/// (ELF convention).
pub fn obj_ext() -> &'static str {
    if cfg!(windows) {
        "obj"
    } else {
        "o"
    }
}

/// Linker flag to give the main thread 8MB of stack: the default is 1MB and
/// large stack-allocated arrays (the compiler's own allocas) overflow it.
/// MSVC's `link.exe` spells this `/STACK:<bytes>`. Linux needs no flag —
/// the main thread gets RLIMIT_STACK (8MB by default) — so `None` there.
pub fn stack_link_flag() -> Option<&'static str> {
    if cfg!(windows) {
        Some("-Wl,/STACK:8388608")
    } else {
        None
    }
}

/// Extra link flags a user program needs beyond the object itself. Linux
/// keeps C math (`fmod`/`sqrt`/...) in a separate `libm`; the Windows CRT
/// link covers it, so the list is empty there.
pub fn default_link_libs() -> &'static [&'static str] {
    if cfg!(windows) {
        &[]
    } else {
        &["m"]
    }
}

/// `true` when targeting Windows.
pub fn is_windows() -> bool {
    cfg!(windows)
}

/// Platform name for the `target_os()` builtin: the OS the compiler was
/// BUILT on (this is also what the self-hosted compiler folds — see the
/// module comment for why that keeps the fixed point exact). The builtin
/// stays a compile-time fold because it is part of the language.
pub fn target_os_name() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "other"
    }
}
