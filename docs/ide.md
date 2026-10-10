# The Aoxn IDE

The official editor for Aoxn, in `ide/`. A native desktop app: a Tauri 2
shell around a Next.js + Monaco workbench, with the filesystem and process
work done by a small Rust command layer.

```
ide/
├─ app/            Next.js App Router entry (layout, page, global styles)
├─ components/     Editor.tsx (Monaco), icons.tsx
├─ lib/            bridge.ts (backend calls), diagnostics.ts (structured
│                  first, text fallback), symbols.ts (outline, jump, search
│                  ranking), monaco.ts, paths.ts (prompt validation),
│                  pkg.ts (manifest + report), tree.ts (fixture tree)
├─ test/           node:test suites: diagnostics, symbols, package report,
│                  path validation, tree
├─ out/            the static export Tauri serves (build output, gitignored)
└─ src-tauri/      Rust: fsops.rs, toolchain.rs, pkg.rs, model.rs, lib.rs
```

## Building and running

```bash
cd ide
pnpm install
pnpm ide:dev        # development, with hot reload
pnpm ide:build      # a release binary + installers
```

`pnpm build` alone produces `out/`, the static bundle. `pnpm ide:build`
compiles the Rust half on top of it (about 12 minutes cold, with LTO).

### Working on the UI without a native rebuild

```bash
pnpm dev            # http://localhost:3000
```

The workbench runs in a plain browser against an in-memory fixture folder
(`lib/bridge.ts`). Everything except the compiler is real — the explorer,
tabs, editor, output panel and quick-open all work — which makes the
visual design loop a browser refresh instead of a 12-minute Rust compile.
The Build/Run buttons answer honestly that there is no toolchain behind
them.

### Tests

```bash
pnpm test                  # the compiler-output parser (node:test)
pnpm test:rust             # the Rust command layer
pnpm typecheck             # tsc --noEmit
```

### Pinned dependency, and one accepted advisory

- **`dompurify` is overridden to 3.4.16** (`package.json`'s
  `pnpm.overrides`). Monaco depends on it but pins an EXACT version
  (`3.4.15`), and 0.57.0 is monaco's latest release, so the two patched
  versions of DOMPurify's `IN_PLACE` handling (a node-removing
  `afterSanitize` hook leaving a detached subtree's handlers armed, and a
  force-removed rawtext root whose text carries attacker markup) could not
  be cleared any other way. The override is a patch bump within the same
  minor, and the editor's 62 frontend tests plus `pnpm build` pass on it.
- **`RUSTSEC-2024-0429` (glib) is ACCEPTED, not fixed** —
  `ide/src-tauri/audit.toml` records why in the repo: the advisory needs
  glib ≥ 0.20, and glib reaches this lock through wry 0.57.0 → `gtk ^0.18`
  → `glib ^0.18`, with both wry and tao already at their latest releases,
  so `cargo update` has nothing to move (the lock stays at glib 0.18.5).
  The unsound `VariantStrIter` path is a transitive webview-backend
  dependency, is Linux-only, and this app never calls it. The file's
  exit condition says when to delete the entry.

## What it does

- **Explorer** — a project tree, directories first, with `target`,
  `node_modules`, `.git` and friends skipped. Click a file to open it.
  **New file / New folder** (the toolbar buttons, or the command palette)
  create entries from a prompt: the name is relative to the selected
  folder, nesting works in one step (`src/util.ax`), and a name that would
  escape the workspace or clobber an existing entry is refused in the
  dialog, not by the filesystem. Creating a file opens it.
- **Editor** — Monaco with an Aoxn grammar (`lib/monaco.ts`: keywords,
  types, builtins, `f""` interpolation, `#` comments). One model per open
  file, so undo is per-file and tab switching keeps position. The Aoxn
  language and the theme are registered in the Editor's `beforeMount`,
  strictly between Monaco's load and the first model's creation — a
  page-level registration raced the mount in v0.31.0 and let Monaco's
  light default flash through.
- **Diagnostics** — the compiler's output becomes editor markers. Clicking
  a diagnostic line in the output panel opens the file it names and jumps
  to the line. **Saving an `.ax` file auto-checks it** (one quiet meta line
  in the output panel; skipped while a manual command is running or no
  compiler was found), so the squiggles follow the edits without a key
  press. Since v0.35.0 the markers come from the compiler's **`--json`
  report** rather than from scanning its text: the backend parses
  `{"ok":…,"errors":[…]}` into `ExecResult.diags` and the panel still
  shows the raw words verbatim beside it. The text scanner
  (`lib/diagnostics.ts`) remains as the fallback for a compiler older than
  v0.35.0 or a command that died before printing.
- **Check / Build / Run** — F7, F6, F5. They shell out to `aoxn` and show
  stdout and stderr in the output panel. Check runs `aoxn check` (v0.31.1),
  which prints only diagnostics — `aoxn c` would dump the generated C into
  the panel. A build or run refreshes the explorer afterwards, because the
  executable lands beside the source.
- **Outline** (v0.35.0) — a third sidebar view listing the declarations of
  the file on screen, in source order, one click to any of them. The rows
  come from `aoxn symbols <file> --json`: the compiler's own AST, walked
  after import resolution, so every `def` and `struct` in the file AND in
  everything it imports is known with its signature and position. The
  outline filters to the current file — the table spans the whole program,
  and drawing all of it would put the stdlib's several hundred
  declarations under every file. A file that does not parse shows the
  compiler's own error instead of an empty list, because "does not parse"
  and "declares nothing" are different facts.
- **Go to definition** (v0.35.0) — Ctrl+click inside the editor resolves
  the identifier under the caret against the same symbol table and opens
  its declaration, **across an `import`**: Ctrl+clicking `clamp_i` in a
  file that imports it opens `util.ax` at the declaration. An `extern def`
  is a valid target (it is where the symbol is declared, though it has no
  body). A local variable is not — Aoxn exports no position for one — and
  the status bar says so rather than doing nothing. The handler was an
  empty function from v0.31.0 until now.
- **Symbol search** (v0.35.0) — Ctrl+Shift+O, ranked exact → prefix →
  word-initials → substring, case-insensitive, with the origin
  `file:line` beside each row. Ties break deterministically (name, then
  path, then line) so the list does not reshuffle between two identical
  keystrokes. Searching from a file finds declarations in its imports,
  which is the case a per-file search would miss.
- **Packages** (v0.33.0) — a second sidebar view (the crate icon in the
  activity bar) over the package manager. It reads the workspace's
  `aoxn.json` tolerantly (missing is a normal state offering Init; broken
  JSON is reported in the panel), lists the dependencies and what is
  unpacked in `aox_modules/`, and exposes the whitelisted verbs as buttons:
  Init, Add, Install, Update, Outdated, Tree, Audit, Why, Remove. Every
  command runs the REAL `aoxn pkg` from the opened folder — the IDE
  reimplements nothing — and the output panel shows it verbatim. The
  whitelist lives in `src-tauri/src/pkg.rs::ALLOWED_SUBCOMMANDS` and is
  enforced in Rust: publish, yank, cache, trust bootstrap and npm-import
  are refused at the gate, not hidden in the UI. Package names typed into
  the Add/Why/Remove prompts are validated on both sides of the bridge
  (`lib/pkg.ts` mirrors `pkg::validate_pkg_name`).
- **Installed versions** (v0.35.0) — the panel reads
  `aoxn pkg list --json` and `aoxn pkg outdated --json` (both already
  whitelisted, both read-only) and shows the **resolved** version — what
  the lockfile says was installed — next to its scope and source
  registry, plus an `old → new` marker for anything behind. This is the
  difference from reading `aoxn.json` alone, which only says what the
  project *asks* for (`^2`). Neither report is fatal: no lockfile yet or
  a registry that did not answer costs one section and is reported in
  the panel, so a failed `outdated` never reads as "everything is
  current".
- **Doctor** — the status bar's "compiler not found" / "clang not found"
  buttons run `aoxn doctor` into the output panel; so does the command
  palette. When a build fails for environment reasons, this is the first
  thing to try, and it re-probes the toolchain when it finishes.
- **Quick open** — Ctrl+P for files, Ctrl+Shift+P for commands,
  Ctrl+Shift+O for symbols.

## Keyboard

| Key | Action |
|---|---|
| `Ctrl+P` | go to file |
| `Ctrl+Shift+P` | command palette |
| `Ctrl+Shift+O` | go to symbol |
| `Ctrl+O` | open folder |
| `Ctrl+S` | save |
| `Ctrl+B` | toggle the sidebar |
| `Ctrl+J` | toggle the output panel |
| `F5` / `F6` / `F7` | run / build / check |
| `Ctrl`+click | go to definition |

## Configuration

| Variable | Meaning |
|---|---|
| `AOXN_IDE_ROOT` | folder to open at launch |
| `AOXN_IDE_CC` | the `aoxn` to drive (default: `aoxn` on PATH) |
| `AOXN_CLANG` | forwarded to the compiler; the compiler needs clang |

A folder may also be passed as a command-line argument. The status bar
reports when no compiler is found, which is the single most useful thing
the IDE can say to a new user.

## Why it is built this way

**It drives the real compiler.** No build logic is reimplemented. A build
inside the IDE runs the same binary with the same arguments a user would
type, so the two cannot disagree — and anything the compiler prints,
including clang's own output, reaches the panel unchanged. The same rule
governs the language service and the packages panel: the outline, "go to
definition" and the symbol search all read `aoxn symbols --json`, the
compiler's own AST, and the installed-versions list reads `aoxn list
--json`. The IDE derives nothing the tool it drives could have answered.

**It asks for machine-readable answers where they exist.** `check`,
`build` and `run` are driven with `--json`, and the backend parses the
report into `ExecResult.diags` — the editor's markers, the problem count
and the status bar read that, where every field is exact. The raw `output`
is kept BESIDE it and is what the panel shows, because a build log is
mostly things a JSON schema does not model: clang's warnings, a program's
prints, a crash. The text scanner in `lib/diagnostics.ts` survives as the
fallback for an older compiler or a command that died before printing;
it is no longer the primary path.

**The log is never re-rendered from structured data.** A reader debugging
a build wants the tool's own words, in the tool's own order, including
the parts nobody modelled. Formatting a diagnostic back into a sentence
loses that, so the panel gets the text and the editor gets the structure,
and neither pretends to be the other.

**Monaco is bundled, not fetched.** `@monaco-editor/react` defaults to a
CDN. That is correct for a website and wrong for a desktop app: the IDE has
to work offline, which is the normal condition on a locked-down machine
that compiles code. `app/page.tsx` points the loader at the bundled copy.

**The workspace is a security boundary.** Every path arriving from the
webview goes through `Workspace::resolve`, which canonicalises it and
refuses anything resolving outside the opened folder — comparing path
*components*, because a string-prefix test lets `C:\proj-evil` through a
`C:\proj` check. Creation commands go through the same gate and refuse to
clobber: a new file or folder that silently replaced an existing one would
be data loss from a single misclick. There is no general read-any-path
command and no shell plugin; `capabilities/default.json` grants only the
window and the folder picker. The rules are in `fsops.rs` and covered by
tests.

**One spelling of every path.** `fs::canonicalize` returns `\\?\D:\...`
verbatim paths on Windows; `fsops::pretty` strips that prefix before any
path leaves the backend, so the explorer's tooltips, the compiler's echoed
diagnostics and the output panel all say `D:\proj\main.ax` — and the
frontend's string comparisons against tree paths actually hold.

**One Cargo workspace.** `ide/src-tauri` is excluded from the root
workspace so `cargo test` at the repo root does not compile a Tauri app.