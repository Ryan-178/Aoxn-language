# The Aoxn IDE

The official editor for Aoxn, in `ide/`. A native desktop app: a Tauri 2
shell around a Next.js + Monaco workbench, with the filesystem and process
work done by a small Rust command layer. Since the ZCode rework the UI is
built on the **ZCode v3.17.2 design system** (Apache-2.0): the Zai Light /
Zai Dark tokens, the Tailwind 4 surface, the shadcn/Radix primitives, the
file tree and the assistant conversation shell are adapted from ZCode's
`packages/ui` source. The Aoxn-specific parts — the compiler, package
manager, language service and workspace bridge — are unchanged in
behaviour and remain the IDE's source of truth.

```
ide/
├─ app/            Next.js App Router entry, Workbench.tsx, globals.css,
│                  zcode-theme.css (the Zai token blocks)
├─ components/
│   ├─ ui/         ZCode/shadcn primitives (button, input, dialog, tabs,
│   │              resizable, select, tooltip, toast, …)
│   ├─ ai-elements/ ZCode's conversation, file-tree, terminal and message
│   │              layout; PromptInput; the Aoxn message renderer
│   ├─ ChatPanel.tsx, TerminalPanel.tsx, GitPanel.tsx, PreviewPanel.tsx
│   ├─ FileExplorer.tsx, DiffView.tsx, Editor.tsx (Monaco), icons.tsx
│   └─ lib/utils.ts  the `cn` class merger (text-ui-* aware)
├─ lib/            bridge.ts (backend calls), diagnostics.ts (structured
│                  first, text fallback), symbols.ts (outline, jump, search
│                  ranking), monaco.ts (Zai Monaco themes), paths.ts,
│                  pkg.ts, tree.ts, theme.ts
├─ test/           node:test suites: diagnostics, symbols, package report,
│                  path validation, tree
├─ out/            the static export Tauri serves (build output, gitignored)
└─ src-tauri/      Rust: fsops.rs, toolchain.rs, pkg.rs, model.rs, lib.rs
```

The ZCode source itself is NOT vendored as a buildable workspace: the
adapted files live under `ide/components/ui` and `ide/components/ai-elements`
and import only React, `radix-ui`, `class-variance-authority`,
`tailwind-merge`, `lucide-react` and the small set of layout/runtime
packages in `ide/package.json`. Re-copying a component from
`.zcode-src/ZCode-main/packages/ui/src` requires stripping its `.js`
import suffixes and replacing ZCode's `@/i18n` / `@/store` / `@/services`
hooks with the Aoxn IDE's own state (the file tree and conversation show
the pattern).

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

### Dependency pins, and one accepted advisory

The ZCode rework added the design-system dependency set: `tailwindcss` and
`@tailwindcss/postcss` (4.3.x), `radix-ui`, `class-variance-authority`,
`tailwind-merge`, `lucide-react`, `react-resizable-panels`,
`use-stick-to-bottom`, `ansi-to-react`, `react-markdown`, `remark-gfm`,
`tailwind-scrollbar-hide` and `tw-animate-css`. The copied ZCode source is
Apache-2.0; attribution is in `ide/THIRD-PARTY-NOTICES.md`. Re-run
`pnpm --dir ide audit` after changing these.

`ansi-to-react` 6.2.6 pins `linkify-it ^3.0.3`, and the 3.x line plus
linkify-it ≤5.0.1 carry quadratic-complexity DoS advisories. The
`ide/pnpm-workspace.yaml` override pins **5.0.2**; the `linkify().match` API
ansi-to-react uses is unchanged, and `pnpm audit` is clean after the bump.
Like the dompurify pin, this override lives in the workspace file so a bot
lockfile regeneration cannot silently drop it.



- **`next` is on 16.4.3.** The 16.3 line carried three advisories — two
  critical (the image-optimisation RCE via `sharp`, and `use cache` returning
  typed arrays) and the App Router Server-Action DoS — all patched at 16.4.1.
  This app is a static export (`output: 'export'`), so no Next server runs in
  the shipped product and none of the three is reachable here; it is upgraded
  anyway, because the lockfile is what a scanner reads and what a future
  `next start` would run.
- **`dompurify` is overridden to 3.4.17** in `ide/pnpm-workspace.yaml`.
  Monaco depends on it but pins an EXACT version (`3.4.15`), and 0.57.0 is
  monaco's latest release, so no bump reaches it — an override is the only
  route. 3.4.16 fixed two `IN_PLACE` advisories but left three more open
  (a critical `SAFE_FOR_TEMPLATES` bypass through attached shadow-root content,
  the incomplete `IN_PLACE` fix, and a skipped SMIL `attributeName=href`
  guard); 3.4.17 closes all three. They are DOM XSS in the sanitiser Monaco
  applies to pasted and loaded HTML. The override is a patch bump within the
  same minor, and the editor's 64 frontend tests, `pnpm typecheck` and
  `pnpm build` pass on it.
- **The override lives in `ide/pnpm-workspace.yaml`, not in the
  `pnpm.overrides` field of `ide/package.json`, and that is not a style
  choice.** The dependabot PR that
  bumped `next` (2026-11-01) regenerated `ide/pnpm-lock.yaml` without the
  package.json override: dompurify fell back to monaco's exact 3.4.15 and two
  already-closed XSS advisories came back — with `ide/package.json` still
  claiming the pin in prose, so nothing in the diff said anything had been
  reverted. `web/next-app/pnpm-workspace.yaml` pins `postcss` for the same
  reason. After any bot-driven lockfile change, check that the lock's
  `overrides:` block still names dompurify.
- `pnpm audit` reports **no known vulnerabilities** for `ide/pnpm-lock.yaml`.
  Re-check both JS manifests with `pnpm --dir ide audit` and
  `pnpm --dir web/next-app audit`; the Rust one with
  `cargo audit` in `ide/src-tauri` (it reads the `audit.toml` below).
- **`RUSTSEC-2024-0429` (glib) is ACCEPTED, not fixed** —
  `ide/src-tauri/audit.toml` records why in the repo and carries the
  2026-11-01 re-check: the advisory needs glib ≥ 0.20, and glib reaches this
  lock through wry → `gtk ^0.18` → `glib ^0.18`. The fix is released on the
  glib 0.20+ line but nothing on the 0.18 line backports it, and the newest
  wry (0.57.1) still asks for `gtk ^0.18`, so `cargo update` has nothing to
  move (the lock stays at glib 0.18.5). The unsound `VariantStrIter` path is a
  transitive webview-backend dependency, is Linux-only, and this app never
  calls it. The file's exit condition says when to delete the entry.

## What it does

- **Explorer** — a project tree, directories first, with `target`,
  `node_modules`, `.git` and friends skipped. Click a file to open it.
  **New file / New folder** (the toolbar buttons, or the command palette)
  create entries from a prompt: the name is relative to the selected
  folder, nesting works in one step (`src/util.ax`), and a name that would
  escape the workspace or clobber an existing entry is refused in the
  dialog, not by the filesystem. Creating a file opens it. The tree is
  ZCode's `ai-elements/file-tree`: controlled expansion, directory chevrons,
  file-type icons and the Zai sidebar surface.
- **Assistant** (new) — a right-hand conversation pane built from ZCode's
  `Conversation` / `Message` / `PromptInput` shell. The active file is
  attached as context on every send. The provider is configurable
  (OpenAI-compatible or Anthropic) and every request is proxied by the Rust
  `ide_ai_chat` command, so the API key lives in the app config file and
  never in the webview. The browser preview reports honestly that it has no
  proxy; the desktop app is where the assistant works.
- **Terminal** (new) — a command panel with ANSI output (`ansi-to-react`),
  command history and a `cd`-aware cwd. The Rust `ide_terminal_exec`
  command runs one shell command at a time in the workspace (or a folder
  inside it); it is a command runner, not a persistent PTY.
- **Source control** (new) — `git status --porcelain=v1 -z` for the open
  workspace, grouped by file with staged/untracked/modified/renamed states.
  Clicking a row opens a Monaco side-by-side diff against `HEAD`
  (`ide_git_diff`). A folder that is not a repository is a normal empty
  state, not an error.
- **Preview** (new) — the bottom dock's Preview tab renders images, PDFs,
  markdown and plain text from the selected file. Bytes come through
  `ide_read_bytes` with a 16 MiB cap; text files reuse the editor's loaded
  content.
- **Themes** — Zai Dark and Zai Light, switched from the title bar and
  remembered in `localStorage`. Monaco has a matching `aoxn-zai-dark` /
  `aoxn-zai-light` theme; the syntax palette is the same family as the DOM
  tokens, kept as concrete hex values because Monaco's parser does not
  accept the `lab()`/`color-mix()` serializations the browser computes.
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
| `Ctrl+J` | toggle the bottom panel |
| `Ctrl` + `` ` `` | show the terminal |
| `Ctrl+Shift+G` | show source control |
| `Ctrl+Shift+A` | toggle the assistant |
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

The assistant's provider, base URL, model and API key live in the Tauri
app config file (`ide-config.json` under the platform app-config
directory), written by `ide_config_write`. They are not environment
variables and they never enter `localStorage` in the desktop app.

## Design system (ZCode v3.17.2)

- `app/zcode-theme.css` is the ZCode token source: `@theme`, `.dark`,
  `.theme-zai-light` and `.theme-zai-dark`, copied verbatim from
  `packages/ui/src/styles.css`. `app/globals.css` imports Tailwind 4, the
  theme, `tw-animate-css` and `tailwind-scrollbar-hide`, adds the
  `data-open` / `data-vertical` / `data-active` custom variants ZCode's
  shadcn primitives expect, and defines the workbench shell classes.
- **Application UI uses `text-ui-*`, not Tailwind's `text-sm`/`text-xs`.**
  That is ZCode's highest-priority design constraint; `components/lib/utils.ts`
  registers the same scale in `tailwind-merge` so a size class and a colour
  class never overwrite each other.
- The copied primitives under `components/ui` come from ZCode's
  `packages/ui/src/components/ui`. Their only edits are import-path
  normalization (`.js` suffixes removed) and replacing ZCode i18n hooks in
  `dialog`/`spinner`/`kbd` with local strings. The conversation and
  file-tree shells under `components/ai-elements` follow the same rule.
- `components/ai-elements/message.tsx` preserves ZCode's layout classes but
  renders Markdown with `react-markdown` + `remark-gfm`; ZCode's full
  Streamdown pipeline is not needed for the IDE's assistant.
- The theme is toggled by `lib/theme.ts` (ported from ZCode's `useTheme`):
  it applies `dark` plus `theme-zai-dark` / `theme-zai-light` to `<html>`.
  Monaco cannot read the computed `lab()`/`color-mix()` values, so
  `lib/monaco.ts` carries the two matching hex palettes.

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

**The terminal and the assistant are explicit widenings, not side doors.**
`ide_terminal_exec` runs one command in a cwd that is itself passed through
`Workspace::resolve`, so the shell cannot be pointed at a directory the
user did not open; the command string is user-typed by definition. The
assistant request is made in Rust, which keeps the API key out of the
webview's network layer and sidesteps provider CORS. `ide_read_bytes`
caps preview payloads at 16 MiB. Git commands are read-only queries
against the workspace root. Keep those properties when adding a command:
the frontend may name a resource, but the backend decides whether it is
inside the boundary.

**One spelling of every path.** `fs::canonicalize` returns `\\?\D:\...`
verbatim paths on Windows; `fsops::pretty` strips that prefix before any
path leaves the backend, so the explorer's tooltips, the compiler's echoed
diagnostics and the output panel all say `D:\proj\main.ax` — and the
frontend's string comparisons against tree paths actually hold.

**One Cargo workspace.** `ide/src-tauri` is excluded from the root
workspace so `cargo test` at the repo root does not compile a Tauri app.