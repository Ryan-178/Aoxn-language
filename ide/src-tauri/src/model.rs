//! Types shared between the Rust backend and the workbench UI.
//!
//! Everything the frontend receives is one of these; there is no free-form
//! JSON shape anywhere in the command surface, so the TypeScript side can
//! mirror it exactly (`lib/types.ts`).

/// One entry in the project explorer.
///
/// The tree is sent FLAT with a `depth` column rather than nested: a 4000
/// node JSON tree is expensive to build and re-serialise on every refresh,
/// while a flat array renders with a trivial map in React and matches how
/// the model is actually consumed (expand/collapse over a sorted list).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeNode {
    pub name: String,
    /// Full path, so the frontend never has to reassemble it (and never
    /// has to agree with us about separators).
    pub path: String,
    pub is_dir: bool,
    pub depth: u32,
}

/// A file's contents plus enough metadata for the status bar.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileContents {
    pub path: String,
    pub text: String,
}

/// Result of running an external command.
#[derive(Debug, Clone, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExecResult {
    pub code: i32,
    /// stdout and stderr interleaved, as one stream. The Aoxn compiler writes
    /// diagnostics to stderr, and a user reading a build log wants them in
    /// the order they happened, so they are merged rather than separated.
    pub output: String,
    /// How long the command took, in milliseconds — the status bar shows it
    /// and it is the only honest feedback that anything happened at all.
    pub duration_ms: u64,
    /// Diagnostics parsed out of the compiler's `--json` report.
    ///
    /// BESIDE `output`, never instead of it. The output panel shows the
    /// compiler's own words verbatim — including clang's, and a program's own
    /// prints — while the editor's markers, the problems list and the
    /// status-bar count come from here, where every field is exact instead
    /// of recovered from text by a scanner that has to cope with a Windows
    /// drive letter. Empty when the command produced no JSON (a crash, a
    /// missing compiler) or was not run with `--json`; the frontend falls
    /// back to parsing `output` then, which is why the fallback still exists.
    #[serde(default)]
    pub diags: Vec<DiagInfo>,
}

/// One compiler diagnostic, as `aoxn --json` reports it.
///
/// Mirrors `Diag`'s JSON in the compiler's `lib.rs`. Its own type rather than
/// a free-form blob so the TypeScript side has a shape to mirror exactly.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiagInfo {
    /// `lex` | `parse` | `type` | `internal` | `link` | `io`.
    #[serde(default)]
    pub stage: String,
    /// The file the diagnostic is in, spelled the way the compiler spells it:
    /// absolute, with no `\\?\` verbatim prefix (see `paths::strip_verbatim`).
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub line: usize,
    #[serde(default)]
    pub col: usize,
    #[serde(default)]
    pub message: String,
}

/// The top-level declarations of a program and of everything it imports.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SymbolTable {
    #[serde(default)]
    pub symbols: Vec<SymbolInfo>,
}

/// One top-level declaration, as `aoxn symbols --json` reports it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SymbolInfo {
    /// `"function"` or `"struct"`.
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub name: String,
    /// The declaration as source text, e.g. `def f(a: int) -> int`. Shown
    /// verbatim in the outline and in a hover, so it reads like the file
    /// rather than like a rendering of it.
    #[serde(default)]
    pub signature: String,
    pub file: String,
    #[serde(default)]
    pub line: usize,
    #[serde(default)]
    pub col: usize,
    /// Last line of the declaration, so the outline can fold the range and
    /// "go to definition" can select all of it. Equals `line` for a
    /// one-liner and for a `struct` header.
    #[serde(default)]
    pub end_line: usize,
    /// Declared return type; `"void"` when the function returns nothing.
    #[serde(default)]
    pub ret: String,
    /// True for `extern def` — declared here, implemented in C, so there is
    /// no body to jump to.
    #[serde(default)]
    pub is_extern: bool,
    #[serde(default)]
    pub params: Vec<NameType>,
    /// Struct fields; empty for functions.
    #[serde(default)]
    pub fields: Vec<NameType>,
}

/// A `name: type` pair — a function parameter or a struct field.
///
/// `ty` is spelled `ty` in Rust because `type` is a keyword; the JSON key
/// stays `"type"`, which is what the compiler emits.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NameType {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
}

/// What the UI needs to know about the Aoxn toolchain before it offers to
/// build anything.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolchainInfo {
    /// The compiler command as configured (never empty — falls back to
    /// "aoxn", which is what a normal install puts on PATH).
    pub compiler: String,
    /// Whether that command actually resolves on this machine. A false here
    /// is the single most useful thing the IDE can tell a new user.
    pub compiler_found: bool,
    /// Whether clang is visible, because the compiler shells out to it and
    /// every build fails without it. `AOXN_CLANG` wins over PATH.
    pub clang_found: bool,
    pub version: String,
}

/// One dependency row in the packages panel.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PkgDep {
    pub name: String,
    /// The requirement as written in the manifest (`^2`, `*`, a path…).
    pub req: String,
    /// True when it came from `devDependencies`.
    pub dev: bool,
}

/// What the lockfile says is actually installed, plus what could be newer.
///
/// This is the difference between what the manifest ASKS for (`^2`) and what
/// the resolver DELIVERED (`2.1.0`), which is the question a package panel
/// exists to answer. Both come from the package manager's own `--json`
/// reports — `aoxn list --json` and `aoxn outdated --json` — so the panel
/// cannot disagree with the tool it drives.
#[derive(Debug, Clone, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PkgReport {
    /// Rows from `aoxn list --json`, sorted by name by the package manager.
    pub installed: Vec<InstalledPkg>,
    /// Rows from `aoxn outdated --json`: a package and the newest version
    /// known. Empty when nothing is behind — or when the registries could
    /// not be reached, which the panel reports separately rather than
    /// showing as "everything is current".
    pub outdated: Vec<OutdatedPkg>,
    /// Set when a report could not be read (no lockfile yet, a registry that
    /// did not answer, a compiler too old for `--json`). The panel shows it
    /// and keeps whatever DID parse.
    pub error: Option<String>,
}

/// One installed package, as `aoxn list --json` reports it.
#[derive(Debug, Clone, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPkg {
    pub name: String,
    /// The resolved version — `2.1.0`, not the manifest's `^2`.
    #[serde(default)]
    pub version: String,
    /// `"prod"` or `"dev"`.
    #[serde(default)]
    pub scope: String,
    /// A short registry name (or `local` for a path dependency).
    #[serde(default)]
    pub source: String,
}

/// One package with a newer version available.
#[derive(Debug, Clone, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OutdatedPkg {
    pub name: String,
    /// The version currently locked.
    #[serde(default)]
    pub locked: String,
    /// The newest version the registry knows about.
    #[serde(default)]
    pub newest: String,
    /// Free-text note from the report (a pre-release, a yanked version…).
    #[serde(default)]
    pub note: String,
}

/// The workspace's package manifest, read TOLERANTLY.
///
/// The packages panel must open — empty, with a hint — in a folder that has
/// no `aoxn.json` at all, and a malformed manifest must show up as a line in
/// the panel rather than a failed command. Missing is not an error here.
#[derive(Debug, Clone, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PkgManifestInfo {
    pub has_manifest: bool,
    pub name: String,
    pub version: String,
    pub has_lockfile: bool,
    /// Set when `aoxn.json` exists but is not valid JSON; the panel shows it
    /// instead of pretending the manifest is fine.
    pub parse_error: Option<String>,
    pub dependencies: Vec<PkgDep>,
    /// Names unpacked into `aox_modules/`.
    pub installed: Vec<String>,
}
/// One changed file in `git status --porcelain` order.
#[derive(Debug, Clone, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GitChange {
    /// Path relative to the workspace root, with forward slashes.
    pub path: String,
    /// `modified` | `added` | `deleted` | `renamed` | `untracked` | `ignored`
    /// | `conflicted`.
    pub status: String,
    /// True when the change is in the index (first porcelain column).
    pub staged: bool,
}

/// `git status` for the open workspace.
#[derive(Debug, Clone, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GitStatus {
    pub is_repo: bool,
    #[serde(default)]
    pub branch: String,
    #[serde(default)]
    pub changes: Vec<GitChange>,
    /// Set when git exists but refused to answer (no repo, unsafe dir, …).
    #[serde(default)]
    pub error: Option<String>,
}

/// The two sides of a file diff. `binary` is true when either side is not
/// valid UTF-8; the UI then shows a summary instead of a text diff.
#[derive(Debug, Clone, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GitFileDiff {
    pub path: String,
    #[serde(default)]
    pub original: String,
    #[serde(default)]
    pub modified: String,
    #[serde(default)]
    pub binary: bool,
}

/// A file's bytes for the preview pane. Base64 keeps the JSON transport
/// simple and works for every image type the webview can render.
#[derive(Debug, Clone, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileBytes {
    pub path: String,
    #[serde(default)]
    pub base64: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub mime: String,
}

/// One message in an OpenAI-compatible chat request.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// The assistant's answer, or the transport/API error that replaced it.
#[derive(Debug, Clone, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChatReply {
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub model: String,
}
