'use client'

/**
 * The bridge to the Rust backend.
 *
 * Every call goes through `call()`, which asks Tauri for the result and
 * falls back to a browser-mode stub when there is no Tauri runtime. That
 * fallback is not decoration: it is what makes `pnpm dev` a usable way to
 * work on the workbench's layout, theme and interaction design without
 * paying for a native rebuild on every change, and it is what the CI
 * screenshot of the UI is taken against.
 *
 * In browser mode the stub answers from an in-memory fixture folder, so the
 * explorer, tabs, editor and output panel are all exercised; only the
 * compiler is inert, and it says so rather than pretending.
 */

import { invoke } from '@tauri-apps/api/core'
import { treeFromPaths } from './tree'
import { EMPTY_MANIFEST, formatManifest, type PkgManifest } from './pkg'

/** Mirror of `model::TreeNode` in src-tauri/src/model.rs. */
export interface TreeNode {
  name: string
  path: string
  isDir: boolean
  depth: number
}

/** Mirror of `model::FileContents`. */
export interface FileContents {
  path: string
  text: string
}

/** Mirror of `model::ExecResult`. */
export interface ExecResult {
  code: number
  output: string
  durationMs: number
  /**
   * Diagnostics parsed out of the compiler's `--json` report.
   *
   * Present but EMPTY when the command produced no JSON — a missing
   * compiler, a crash, a compiler older than v0.34.0. The frontend falls
   * back to scanning `output` in that case, which is why the text scanner
   * in `lib/diagnostics.ts` still exists; it is just no longer the primary
   * path. `output` itself is never replaced by the structured form: the
   * panel shows the compiler's own words, clang's, and the program's.
   */
  diags: DiagInfo[]
}

/** Mirror of `model::DiagInfo`. */
export interface DiagInfo {
  /** `lex` | `parse` | `type` | `internal` | `link` | `io`. */
  stage: string
  /** Absolute, spelled the way the compiler spells it (no `\\?\`). */
  file: string
  line: number
  col: number
  message: string
}

/** Mirror of `model::SymbolTable` — what `aoxn symbols --json` reports. */
export interface SymbolTable {
  symbols: SymbolInfo[]
}

/** Mirror of `model::SymbolInfo`. */
export interface SymbolInfo {
  kind: 'function' | 'struct'
  name: string
  /** The declaration as source text, e.g. `def f(a: int) -> int`. */
  signature: string
  /** Where it is declared — often NOT the file the editor has open. */
  file: string
  line: number
  col: number
  endLine: number
  ret: string
  /** `extern def`: declared here, implemented in C, so no body to jump to. */
  isExtern: boolean
  params: NameType[]
  fields: NameType[]
}

/** Mirror of `model::NameType`. */
export interface NameType {
  name: string
  type: string
}

/** Mirror of `model::PkgReport` — what is installed and what is behind. */
export interface PkgReport {
  installed: InstalledPkg[]
  outdated: OutdatedPkg[]
  /** Set when a half of the report could not be read; the panel shows it. */
  error: string | null
}

/** Mirror of `model::InstalledPkg` — one row of `aoxn list --json`. */
export interface InstalledPkg {
  name: string
  /** The RESOLVED version, not the manifest's requirement. */
  version: string
  scope: string
  source: string
}

/** Mirror of `model::OutdatedPkg` — one row of `aoxn outdated --json`. */
export interface OutdatedPkg {
  name: string
  locked: string
  newest: string
  note: string
}

/** Mirror of `model::ToolchainInfo`. */
export interface ToolchainInfo {
  compiler: string
  compilerFound: boolean
  clangFound: boolean
  version: string
}

/** True when running inside the Tauri webview. */
export const isNative = (): boolean =>
  typeof window !== 'undefined' &&
  ('__TAURI_INTERNALS__' in window || '__TAURI__' in window)

// ---- browser-mode fixture ----
// A tiny synthetic project: enough shape to prove the explorer renders,
// expand/collapse works and a file opens into the editor. Keys are the
// SAME absolute paths the tree hands out — the editor reads and saves
// through them, so half-paths here would open empty editors.

const FIXTURE_ROOT = '/preview'

const FIXTURE: Record<string, string> = {
  '/preview/hello.ax': `# Aoxn IDE — browser preview
# This tree is a fixture; run the packaged app for a real folder.

import * from "stdlib"

def main() -> int:
    n = fib(20)
    print("fib(20) = " + str(n))
    return 0

def fib(n: int) -> int:
    if n < 2:
        return n
    return fib(n - 1) + fib(n - 2)
`,
  '/preview/notes.md': `# Notes

The IDE shells out to the real \`aoxn\` binary; nothing about the build
is reimplemented here.
`,
  '/preview/src/util.ax': `def clamp_i(v: int, lo: int, hi: int) -> int:
    if v < lo:
        return lo
    if v > hi:
        return hi
    return v
`,
  '/preview/src/main.ax': `import * from "./util.ax"

def main() -> int:
    print(clamp_i(300, 0, 255))
    return 0
`,
  '/preview/aoxn.json': `{
  "name": "preview-fixture",
  "version": "0.1.0",
  "dependencies": {
    "stdlib-demo": "^1.2"
  },
  "devDependencies": {
    "axtest": "*"
  }
}
`,
  '/preview/aoxn.lock': '{ "lockfile_version": 1 }\n',
  '/preview/README.txt': 'Aoxn IDE — browser preview fixture.',
}

/** Directories kept in the fixture even when they hold no file (New folder). */
const FIXTURE_DIRS: string[] = []

/** The fixture tree is always derived, never hand-maintained: a created
 *  file or folder must show up without the fixture learning about it. */
const fixtureTree = (): TreeNode[] => treeFromPaths(FIXTURE, FIXTURE_ROOT, FIXTURE_DIRS)

async function call<T>(cmd: string, args?: Record<string, unknown>, fallback?: () => T | Promise<T>): Promise<T> {
  if (isNative()) {
    return invoke<T>(cmd, args)
  }
  if (!fallback) {
    throw new Error(`${cmd} is only available in the desktop app`)
  }
  return fallback()
}

// ---- commands ----

/**
 * Ask the backend to open a project folder. There is deliberately NO path
 * argument: the backend opens a native folder dialog itself, so a compromised
 * webview cannot name a directory the user never picked. Cancelling the dialog
 * leaves the current workspace untouched.
 */
export const openFolder = (): Promise<TreeNode[]> =>
  call('ide_open_folder', undefined, () => fixtureTree())

export const scan = (): Promise<TreeNode[]> =>
  call('ide_scan', undefined, () => fixtureTree())

/** The folder the backend has open (the fixture root in browser mode). */
export const workspaceRoot = (): Promise<string | null> =>
  call('ide_root', undefined, () => FIXTURE_ROOT)

export const readFile = (path: string): Promise<FileContents> =>
  call('ide_read', { path }, () => ({ path, text: FIXTURE[path] ?? '' }))

export const saveFile = (path: string, text: string): Promise<void> =>
  call('ide_save', { path, text }, () => {
    FIXTURE[path] = text
  })

/**
 * Create a file and get the refreshed tree back. The backend refuses to
 * clobber; the fixture mirrors that so the preview behaves like the app.
 */
export const newFile = (path: string): Promise<TreeNode[]> =>
  call('ide_new_file', { path }, () => {
    if (FIXTURE[path] !== undefined) throw new Error(`${path} already exists`)
    FIXTURE[path] = ''
    return fixtureTree()
  })

/** Create a folder and get the refreshed tree back. */
export const newDir = (path: string): Promise<TreeNode[]> =>
  call('ide_new_dir', { path }, () => {
    if (FIXTURE[path] !== undefined || FIXTURE_DIRS.includes(path)) {
      throw new Error(`${path} already exists`)
    }
    FIXTURE_DIRS.push(path)
    return fixtureTree()
  })

export const toolchain = (): Promise<ToolchainInfo> =>
  call(
    'ide_toolchain',
    undefined,
    async () => ({
      compiler: 'aoxn',
      compilerFound: false,
      clangFound: false,
      version: 'browser preview — no compiler available',
    }),
  )

/** `aoxn doctor` — the toolchain's own self-check, into the output panel. */
export const doctor = (): Promise<ExecResult> => call('ide_doctor', {}, notInBrowser)

/**
 * The workspace's `aoxn.json`, read tolerantly. The fixture ships a small
 * manifest so the packages panel has something real to show in the browser.
 */
export const pkgManifest = (): Promise<PkgManifest> =>
  call('ide_pkg_manifest', undefined, () => {
    const raw = FIXTURE[`${FIXTURE_ROOT}/aoxn.json`]
    if (raw === undefined) return { ...EMPTY_MANIFEST, installed: [] }
    let parsed: Record<string, unknown>
    try {
      parsed = JSON.parse(raw) as Record<string, unknown>
    } catch (e) {
      return { ...formatManifest(null), parseError: String(e) }
    }
    return formatManifest({ ...parsed, hasManifest: true, hasLockfile: true })
  })

/**
 * Run a whitelisted `aoxn pkg` subcommand from the workspace root. The
 * backend refuses everything not on its list (publish, yank, cache, …);
 * the browser preview has no package manager behind it and says so.
 */
export const pkgRun = (subcommand: string, arg?: string): Promise<ExecResult> =>
  call('ide_pkg_run', { subcommand, arg: arg ?? null }, notInBrowser)

export const check = (path: string): Promise<ExecResult> =>
  call('ide_check', { path }, notInBrowser)

export const build = (path: string): Promise<ExecResult> =>
  call('ide_build', { path }, notInBrowser)

export const runProgram = (path: string): Promise<ExecResult> =>
  call('ide_run', { path }, notInBrowser)

export const buildAndRun = (path: string): Promise<ExecResult> =>
  call('ide_build_and_run', { path }, notInBrowser)

/**
 * The top-level declarations of a file and of everything it imports.
 *
 * In browser mode the fixture answers with the declarations of its own
 * files, hand-written to match what `aoxn symbols` would say about them —
 * the outline is layout work, and laying it out against a static table is
 * the point of `pnpm dev`.
 */
export const symbols = (path: string): Promise<SymbolTable> =>
  call('ide_symbols', { path }, () => fixtureSymbols(path))

/**
 * What the package manager says is installed, and what is behind.
 *
 * The fixture carries a plausible lockfile state so the packages panel has
 * something real to lay out, including one outdated row — the badge is part
 * of the design and a preview that never shows one hides half of it.
 */
export const pkgReport = (): Promise<PkgReport> =>
  call('ide_pkg_report', undefined, () => FIXTURE_PKG_REPORT)

function notInBrowser(): ExecResult {
  return {
    code: -1,
    output:
      'This is the browser preview — there is no Aoxn toolchain behind it.\n' +
      'Run `pnpm ide:dev` (or the packaged app) to build and run for real.\n',
    durationMs: 0,
    diags: [],
  }
}

/** Declarations the fixture claims, per file, in the compiler's shape. */
const FIXTURE_SYMBOLS: Record<string, SymbolInfo[]> = {
  '/preview/hello.ax': [
    sym('function', 'main', 'def main() -> int', '/preview/hello.ax', 6, 1, 9, 'int'),
    sym('function', 'fib', 'def fib(n: int) -> int', '/preview/hello.ax', 11, 1, 14, 'int', [
      { name: 'n', type: 'int' },
    ]),
  ],
  '/preview/src/util.ax': [
    sym(
      'function',
      'clamp_i',
      'def clamp_i(v: int, lo: int, hi: int) -> int',
      '/preview/src/util.ax',
      1,
      1,
      7,
      'int',
      [
        { name: 'v', type: 'int' },
        { name: 'lo', type: 'int' },
        { name: 'hi', type: 'int' },
      ],
    ),
  ],
  '/preview/src/main.ax': [
    sym('function', 'main', 'def main() -> int', '/preview/src/main.ax', 3, 1, 5, 'int'),
  ],
}

/**
 * The fixture's symbol table: the file's own declarations plus everything it
 * imports, which is what the real command returns. `src/main.ax` importing
 * `util.ax` is the interesting shape — the outline has to show a symbol
 * whose `file` is not the one on screen, or the cross-file behaviour is
 * never exercised in the preview.
 */
function fixtureSymbols(path: string): SymbolTable {
  const out: SymbolInfo[] = []
  const seen = new Set<string>()
  const add = (p: string): void => {
    if (seen.has(p)) return
    seen.add(p)
    out.push(...(FIXTURE_SYMBOLS[p] ?? []))
    // The fixture has exactly one import edge; hard-coding it is honest here
    // and beats a fake resolver nobody would believe.
    if (p === '/preview/src/main.ax') add('/preview/src/util.ax')
  }
  add(path)
  return { symbols: out }
}

/** Build one fixture symbol with the fields the compiler always emits. */
function sym(
  kind: SymbolInfo['kind'],
  name: string,
  signature: string,
  file: string,
  line: number,
  col: number,
  endLine: number,
  ret: string,
  params: NameType[] = [],
): SymbolInfo {
  return { kind, name, signature, file, line, col, endLine, ret, isExtern: false, params, fields: [] }
}

const FIXTURE_PKG_REPORT: PkgReport = {
  installed: [
    { name: 'stdlib-demo', version: '1.4.0', scope: 'prod', source: 'local' },
    { name: 'axtest', version: '0.3.1', scope: 'dev', source: 'local' },
  ],
  outdated: [{ name: 'stdlib-demo', locked: '1.4.0', newest: '1.6.0', note: '' }],
  error: null,
}

/** Open the native folder picker. Null when cancelled or in the browser. */
export async function pickFolder(): Promise<string | null> {
  if (!isNative()) return null
  const { open } = await import('@tauri-apps/plugin-dialog')
  const picked = await open({ directory: true, multiple: false, title: 'Open folder' })
  return typeof picked === 'string' ? picked : null
}

/** The folder the backend opened on launch, if any. */
export async function currentRoot(): Promise<string | null> {
  try {
    return await workspaceRoot()
  } catch {
    return null
  }
}
