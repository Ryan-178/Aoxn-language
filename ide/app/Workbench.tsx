'use client'

/**
 * The Aoxn IDE workbench.
 *
 * The shape is the one every editor since VS Code converged on: an activity
 * bar, a side panel, a tabbed editor, a bottom panel and a status bar. That
 * is not a fashion choice — it is the arrangement that maps onto what a
 * programmer already has in their hands, and re-inventing it would make the
 * IDE harder to use than the thing it replaces.
 *
 * State lives here and nowhere else. Documents are a `Map` keyed by path,
 * each holding its text, its last-saved text and the cursor position; the
 * tab order is an array of paths. There is no store library: the app has
 * one screen, one owner and no async data layer to normalise.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  BracesIcon,
  CheckIcon,
  CircleAlertIcon,
  Code2Icon,
  CommandIcon,
  FileCode2Icon,
  FilesIcon,
  FolderOpenIcon,
  GitBranchIcon,
  HammerIcon,
  ListTreeIcon,
  MoonIcon,
  PackageIcon,
  PanelBottomIcon,
  PlayIcon,
  RefreshCwIcon,
  SearchIcon,
  SettingsIcon,
  SparklesIcon,
  SquareTerminalIcon,
  SunIcon,
  WrenchIcon,
  XIcon,
} from 'lucide-react'
import { Editor, type EditorProps } from '@/components/Editor'
import { ChatPanel } from '@/components/ChatPanel'
import { DiffView } from '@/components/DiffView'
import { FileExplorer } from '@/components/FileExplorer'
import { GitPanel } from '@/components/GitPanel'
import { PreviewPanel } from '@/components/PreviewPanel'
import { TerminalPanel } from '@/components/TerminalPanel'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from '@/components/ui/resizable'
import { Separator } from '@/components/ui/separator'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Kbd } from '@/components/ui/kbd'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'
import { cn } from '@/components/lib/utils'
import { useTheme } from '@/lib/theme'
import * as api from '@/lib/bridge'
import type {
  ExecResult,
  GitStatus,
  PkgReport,
  SymbolTable,
  ToolchainInfo,
  TreeNode,
} from '@/lib/bridge'
import { joinEntry, validateEntryName } from '@/lib/paths'
import {
  formatDep,
  formatInstalled,
  formatReport,
  formatUpgrade,
  validatePkgName,
  withUpgrades,
  type PkgManifest,
} from '@/lib/pkg'
import {
  basename,
  diagnosticsByFile,
  diagnosticsFrom,
  parseDiagnostic,
  type Diagnostic,
} from '@/lib/diagnostics'
import {
  definitionOf,
  outlineFor,
  search as searchSymbols,
  symbolLabel,
  symbolOrigin,
  wordAt,
  type OutlineRow,
} from '@/lib/symbols'

/** Which sidebar view the activity bar is showing. */
type SideView = 'files' | 'packages' | 'outline'

/** The bottom dock's tabs, in ZCode order: output, terminal, git, preview. */
type BottomTab = 'output' | 'terminal' | 'git' | 'preview'

/** A file compared against HEAD, opened in Monaco's diff editor. */
interface DiffTarget {
  path: string
  original: string
  modified: string
  binary: boolean
}

/** What the new-file / new-folder / package prompts can ask for. */
type PromptMode = 'file' | 'folder' | 'pkgadd' | 'pkgwhy' | 'pkgremove'

interface Doc {
  path: string
  text: string
  /** What was on disk at the last save or open; the dirty test is `!==`. */
  saved: string
  cursor: { line: number; column: number }
}

/** A pending "reveal this line" instruction, consumed by the Editor. */
interface Jump {
  path: string
  line: number
  column: number
  /** Bumped so two jumps to the same spot still fire. */
  seq: number
}

type LogKind = 'cmd' | 'out' | 'err' | 'meta'

interface LogEntry {
  id: number
  kind: LogKind
  text: string
}

export default function Workbench() {
  const [docs, setDocs] = useState<Map<string, Doc>>(() => new Map())
  const [tabs, setTabs] = useState<string[]>([])
  const [active, setActive] = useState<string | null>(null)

  const [tree, setTree] = useState<TreeNode[]>([])
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set())
  const [root, setRoot] = useState<string>('')
  const [selected, setSelected] = useState<string | null>(null)

  const [showSidebar, setShowSidebar] = useState(true)
  const [showPanel, setShowPanel] = useState(true)
  const [view, setView] = useState<SideView>('files')
  const [pkg, setPkg] = useState<PkgManifest | null>(null)
  /** What the package manager says is installed, and what is behind. */
  const [pkgInfo, setPkgInfo] = useState<PkgReport | null>(null)
  const [busy, setBusy] = useState(false)
  const [log, setLog] = useState<LogEntry[]>([])
  const [toolchain, setToolchain] = useState<ToolchainInfo | null>(null)
  const [palette, setPalette] = useState<{
    mode: 'file' | 'command' | 'symbol'
    query: string
  } | null>(null)
  const [toast, setToast] = useState('')
  const [diagnostics, setDiagnostics] = useState<Diagnostic[]>([])
  const [jump, setJump] = useState<Jump | null>(null)
  const [prompt, setPrompt] = useState<{ mode: PromptMode; dir: string } | null>(null)
  /**
   * The compiler's symbol table for the file the user is looking at, plus
   * every file it imports. Refetched when the active file or the workspace
   * changes — NOT on every keystroke, because this is a process launch and
   * the outline does not need to be keystroke-accurate to be useful.
   */
  const [symbols, setSymbols] = useState<SymbolTable>({ symbols: [] })
  /** Why the outline is empty, when it is empty for a reason worth saying. */
  const [symbolError, setSymbolError] = useState<string>('')

  // ---- ZCode shell state ----
  const { theme, setTheme, resolved } = useTheme()
  const monacoTheme = resolved === 'dark' ? 'aoxn-zai-dark' : 'aoxn-zai-light'
  const [showAssistant, setShowAssistant] = useState(true)
  const [bottomTab, setBottomTab] = useState<BottomTab>('output')
  const [diff, setDiff] = useState<DiffTarget | null>(null)
  const [previewPath, setPreviewPath] = useState<string | null>(null)
  const [git, setGit] = useState<GitStatus | null>(null)

  const logSeq = useRef(0)
  /**
   * The toolchain is serialised through a ref, not just the `busy` state:
   * an auto-check fired by a save must not race the build that triggered
   * the save, and a state update is always one render behind the ref.
   */
  const busyRef = useRef(false)

  const appendLog = useCallback((kind: LogKind, text: string) => {
    setLog((prev) => [...prev, { id: ++logSeq.current, kind, text }].slice(-2000))
  }, [])

  const say = useCallback((text: string) => {
    setToast(text)
    window.setTimeout(() => setToast((t) => (t === text ? '' : t)), 2600)
  }, [])

  const refreshGit = useCallback(async () => {
    try {
      setGit(await api.gitStatus())
    } catch {
      setGit(null)
    }
  }, [])

  /** Open one changed file in Monaco's diff editor (HEAD vs working tree). */
  const openDiff = useCallback(
    async (path: string) => {
      try {
        const target = await api.gitDiff(path)
        setDiff(target)
        setPreviewPath(path)
      } catch (e) {
        appendLog('err', String(e))
        say(`Cannot diff ${basename(path)}`)
      }
    },
    [appendLog, say],
  )

  const showBottom = useCallback((tab: BottomTab) => {
    setBottomTab(tab)
    setShowPanel(true)
  }, [])

  const refreshToolchain = useCallback(async () => {
    try {
      setToolchain(await api.toolchain())
    } catch {
      setToolchain(null)
    }
  }, [])

  /** Re-read `aoxn.json` + `aox_modules/` for the packages panel. */
  const refreshPkg = useCallback(async () => {
    try {
      setPkg(await api.pkgManifest())
    } catch {
      setPkg(null)
    }
    // The report is a SEPARATE read (`aoxn list`/`outdated`), and it can
    // fail where the manifest read succeeded — no lockfile yet, a registry
    // that did not answer. That is a normal state the panel reports, not an
    // error that blanks the whole view.
    try {
      setPkgInfo(formatReport(await api.pkgReport()))
    } catch {
      setPkgInfo(null)
    }
  }, [])

  /**
   * Ask the compiler for the declarations of a file and its imports.
   *
   * Only for `.ax`/`.ts`/`.tsx`: `aoxn symbols` on anything else reports a
   * parse error, and an outline of a JSON file's "declarations" would be
   * nonsense. A failure is kept as a message rather than an empty table,
   * because "this file does not parse" and "this file declares nothing" are
   * different facts and the reader needs to know which one they are looking
   * at.
   */
  const refreshSymbols = useCallback(async (path: string | null) => {
    if (!path || !/\.(ax|ts|tsx)$/i.test(path)) {
      setSymbols({ symbols: [] })
      setSymbolError('')
      return
    }
    try {
      setSymbols(await api.symbols(path))
      setSymbolError('')
    } catch (e) {
      setSymbols({ symbols: [] })
      setSymbolError(String(e).replace(/^Error:\s*/, '').split('\n')[0])
    }
  }, [])

  // ---- boot ----
  useEffect(() => {
    void (async () => {
      await refreshToolchain()
      await refreshPkg()
      void refreshGit()
      try {
        // The root comes from the backend, not from the tree: an EMPTY
        // folder is a legal project, and "no tree rows" must not read as
        // "no folder open".
        const r = await api.currentRoot()
        if (r) setRoot(r)
        const nodes = await api.scan()
        setTree(nodes)
        setExpanded(new Set(nodes.filter((n) => n.isDir && n.depth === 0).map((n) => n.path)))
        if (!r) say('No folder open — use the folder button in the activity bar.')
      } catch (e) {
        appendLog('err', String(e))
      }
    })()
  }, [refreshToolchain, refreshPkg, refreshGit, say, appendLog])

  // ---- documents ----

  /**
   * Quiet type-check after a save of an `.ax` file: the red squiggles
   * refresh without the user pressing F7. It logs one meta line — silence
   * would look like nothing happened — and it never runs while a manual
   * command owns the toolchain (the `busy` ref).
   */
  const autoCheck = useCallback(
    async (path: string) => {
      if (!path.toLowerCase().endsWith('.ax')) return
      if (busyRef.current || !toolchain?.compilerFound) return
      busyRef.current = true
      try {
        const result = await api.check(path)
        const found = diagnosticsFrom(result)
        setDiagnostics(found)
        appendLog(
          'meta',
          `• auto-check ${basename(path)}: ${
            found.length === 0 ? 'clean' : `${found.length} problem${found.length === 1 ? '' : 's'}`
          } · ${result.durationMs} ms`,
        )
      } catch {
        // The manual Check (F7) reports toolchain problems honestly; an
        // auto-check that cannot run says nothing.
      } finally {
        busyRef.current = false
      }
    },
    [toolchain, appendLog],
  )

  const openFile = useCallback(
    async (path: string, atLine = 0) => {
      try {
        const file = await api.readFile(path)
        setDocs((prev) => {
          const next = new Map(prev)
          if (!next.has(path)) {
            next.set(path, { path, text: file.text, saved: file.text, cursor: { line: 1, column: 1 } })
          }
          return next
        })
        setTabs((prev) => (prev.includes(path) ? prev : [...prev, path]))
        setActive(path)
        setSelected(path)
        setPreviewPath(path)
        setDiff(null)
        if (atLine > 0) setJump({ path, line: atLine, column: 1, seq: Date.now() })
      } catch (e) {
        appendLog('err', String(e))
        say(`Cannot open ${basename(path)}`)
      }
    },
    [appendLog, say],
  )

  /**
   * Re-derive the symbol table when the file on screen changes.
   *
   * This is a compiler invocation, so it is keyed on `active` and on the
   * workspace — not on the text. An outline that updates keystroke by
   * keystroke would be a process launch per keystroke, and a declaration
   * only moves when the file is saved anyway.
   */
  useEffect(() => {
    void refreshSymbols(active)
  }, [active, root, refreshSymbols])

  /** Open a declaration, from the outline, the symbol search or Ctrl+click. */
  const revealSymbol = useCallback(
    async (path: string, line: number, column: number) => {
      await openFile(path, line)
      // `openFile` jumps to column 1; a definition wants the declaration's
      // own column so the caret sits on the name, not before it.
      setJump({ path, line, column, seq: Date.now() })
    },
    [openFile],
  )

  /**
   * Resolve the identifier under the caret and open its declaration.
   *
   * Uses the symbol table of the CURRENT file and its imports. A name that
   * is a local variable is not there — Aoxn has no way to export a local's
   * position — so the honest answer is a short message rather than a silent
   * no-op, which is what this handler used to be.
   */
  const goToDefinition = useCallback(
    async (line: number, column: number) => {
      if (!active) return
      const doc = docs.get(active)
      if (!doc) return
      const name = wordAt(doc.text, line, column)
      if (!name) return
      const target = definitionOf(symbols, name, active)
      if (!target) {
        say(`No declaration found for '${name}'.`)
        return
      }
      await revealSymbol(target.path, target.line, target.column)
    },
    [active, docs, symbols, say, revealSymbol],
  )

  const saveFile = useCallback(
    async (path: string) => {
      const doc = docs.get(path)
      if (!doc || doc.text === doc.saved) return true
      try {
        await api.saveFile(path, doc.text)
        setDocs((prev) => {
          const next = new Map(prev)
          const d = next.get(path)
          if (d) next.set(path, { ...d, saved: d.text })
          return next
        })
        say(`Saved ${basename(path)}`)
        void autoCheck(path)
        void refreshGit()
        return true
      } catch (e) {
        appendLog('err', String(e))
        say(`Cannot save ${basename(path)}`)
        return false
      }
    },
    [docs, appendLog, say, autoCheck, refreshGit],
  )

  const closeTab = useCallback((path: string) => {
    setTabs((prev) => {
      const idx = prev.indexOf(path)
      const next = prev.filter((p) => p !== path)
      if (path === active) setActive(next[Math.max(0, idx - 1)] ?? null)
      return next
    })
  }, [active])

  // ---- toolchain ----

  const refreshTree = useCallback(async () => {
    try {
      setTree(await api.scan())
    } catch (e) {
      appendLog('err', String(e))
    }
  }, [appendLog])

  const runTool = useCallback(
    async (what: 'check' | 'build' | 'run', path: string | null) => {
      if (!path) {
        say('Open a file first.')
        return
      }
      if (busyRef.current) {
        say('A command is already running.')
        return
      }
      busyRef.current = true
      if (docs.get(path)?.text !== docs.get(path)?.saved) {
        appendLog('meta', `• ${basename(path)} has unsaved changes — saving first`)
        await saveFile(path)
      }
      setBusy(true)
      showBottom('output')
      const verb =
        what === 'check' ? `aoxn check ${basename(path)}` : `aoxn ${what} ${basename(path)}`
      appendLog('cmd', `❯ ${verb}`)
      try {
        const result: ExecResult =
          what === 'check'
            ? await api.check(path)
            : what === 'build'
              ? await api.build(path)
              : await api.runProgram(path)
        for (const line of result.output.split('\n')) {
          if (line.length) appendLog('out', line)
        }
        appendLog(
          result.code === 0 ? 'meta' : 'err',
          `${what} ${result.code === 0 ? 'succeeded' : 'failed'} · exit ${result.code} · ${result.durationMs} ms`,
        )
        setDiagnostics(diagnosticsFrom(result))
        // A build (or a cached run) drops an executable beside the source;
        // the explorer should show it without a manual refresh.
        if (what !== 'check') void refreshTree()
      } catch (e) {
        appendLog('err', String(e))
      } finally {
        busyRef.current = false
        setBusy(false)
      }
    },
    [docs, saveFile, appendLog, refreshTree, showBottom],
  )

  // Clicking a diagnostic in the log opens the file it names and puts the
  // caret on that line. This is the whole reason the log keeps the compiler's
  // text verbatim rather than a prettified version of it.
  const jumpToLogLine = useCallback(
    async (raw: string) => {
      const d = parseDiagnostic(raw)
      if (!d || !d.file) return
      const path = resolveAgainstRoot(d.file, root)
      await openFile(path, Math.max(1, d.line))
      say(`${basename(path)} : ${d.line}`)
    },
    [root, openFile, say],
  )

  // ---- workspace ----

  const openFolder = useCallback(async () => {
    // No path is passed: the backend opens a NATIVE folder dialog and the
    // user picks there, so the webview cannot name a workspace itself.
    try {
      const nodes = await api.openFolder()
      setTree(nodes)
      const opened = await api.workspaceRoot()
      setRoot(opened ?? '')
      setExpanded(new Set(nodes.filter((n) => n.isDir && n.depth === 0).map((n) => n.path)))
      appendLog('meta', `• opened ${opened ?? ''}`)
      await refreshPkg()
    } catch (e) {
      appendLog('err', String(e))
    }
  }, [appendLog, refreshPkg, refreshGit])

  /** Run `aoxn doctor` into the output panel — the toolchain's own
   *  self-check, and the first thing to try when the status bar says the
   *  compiler or clang is missing. */
  const runDoctor = useCallback(async () => {
    if (busyRef.current) {
      say('A command is already running.')
      return
    }
    busyRef.current = true
    setBusy(true)
    showBottom('output')
    appendLog('cmd', '❯ aoxn doctor')
    try {
      const result = await api.doctor()
      for (const line of result.output.split('\n')) {
        if (line.length) appendLog('out', line)
      }
      appendLog(
        result.code === 0 ? 'meta' : 'err',
        `doctor ${result.code === 0 ? 'succeeded' : 'failed'} · exit ${result.code} · ${result.durationMs} ms`,
      )
      await refreshToolchain()
    } catch (e) {
      appendLog('err', String(e))
    } finally {
      busyRef.current = false
      setBusy(false)
    }
  }, [appendLog, refreshToolchain, showBottom])

  /**
   * Run a whitelisted `aoxn pkg` subcommand from the workspace root. The
   * output is the package manager's own, verbatim; mutating subcommands
   * re-read the manifest (and the explorer) when they finish.
   */
  const runPkg = useCallback(
    async (sub: string, arg?: string) => {
      if (busyRef.current) {
        say('A command is already running.')
        return
      }
      busyRef.current = true
      setBusy(true)
      showBottom('output')
      appendLog('cmd', `❯ aoxn pkg ${sub}${arg ? ` ${arg}` : ''}`)
      try {
        const result = await api.pkgRun(sub, arg)
        for (const line of result.output.split('\n')) {
          if (line.length) appendLog('out', line)
        }
        appendLog(
          result.code === 0 ? 'meta' : 'err',
          `pkg ${sub} ${result.code === 0 ? 'succeeded' : 'failed'} · exit ${result.code} · ${result.durationMs} ms`,
        )
        if (['init', 'install', 'update', 'add', 'remove', 'uninstall'].includes(sub)) {
          await refreshPkg()
          await refreshTree()
        }
      } catch (e) {
        appendLog('err', String(e))
      } finally {
        busyRef.current = false
        setBusy(false)
      }
    },
    [appendLog, refreshPkg, refreshTree, showBottom],
  )

  // ---- creating entries ----

  /** The folder a "new file / new folder" prompt starts from: the selected
   *  directory, the selected file's parent, or the workspace root. */
  const baseDir = useCallback((): string => {
    if (!root) return ''
    const node = selected ? tree.find((n) => n.path === selected) : null
    if (!node) return root
    return node.isDir ? node.path : node.path.replace(/[\\/][^\\/]*$/, '')
  }, [root, selected, tree])

  const startPrompt = useCallback(
    (mode: PromptMode) => {
      if (!root) {
        say('Open a folder first.')
        return
      }
      setPrompt({ mode, dir: baseDir() })
    },
    [root, baseDir, say],
  )

  /** Create the entry, adopt the tree the backend returns, and (for files)
   *  open the one the TREE names — its spelling is the one every other
   *  command agrees on, which matters when canonicalise reshapes the path. */
  const createEntry = useCallback(
    async (kind: 'file' | 'folder', dir: string, name: string) => {
      const target = joinEntry(dir, name)
      const nodes = kind === 'file' ? await api.newFile(target) : await api.newDir(target)
      const norm = (p: string) => p.replace(/\\/g, '/')
      setTree(nodes)
      setExpanded((prev) => new Set(prev).add(dir))
      setSelected(target)
      void refreshGit()
      if (kind === 'file') {
        const created = nodes.find((n) => !n.isDir && norm(n.path) === norm(target))
        await openFile(created ? created.path : target)
      }
      say(`Created ${name.trim()}`)
    },
    [openFile, say, refreshGit],
  )

  /** Dispatch a submitted prompt to the action its mode stands for. */
  const confirmPrompt = useCallback(
    (mode: PromptMode, dir: string, name: string): Promise<void> => {
      switch (mode) {
        case 'file':
          return createEntry('file', dir, name)
        case 'folder':
          return createEntry('folder', dir, name)
        case 'pkgadd':
          return runPkg('add', name)
        case 'pkgwhy':
          return runPkg('why', name)
        case 'pkgremove':
          return runPkg('remove', name)
      }
    },
    [createEntry, runPkg],
  )

  // ---- derived ----

  const activeDoc = active ? (docs.get(active) ?? null) : null
  const diagsByFile = useMemo(() => diagnosticsByFile(diagnostics), [diagnostics])
  const activeDiags = useMemo(() => {
    if (!active) return []
    return diagsByFile.get(basename(active)) ?? []
  }, [active, diagsByFile])

  /**
   * The outline of the file on screen: its own declarations, in order.
   *
   * Derived from the same symbol table that answers "go to definition", so
   * the two cannot disagree about what is declared where. The table spans
   * every import, which is why the outline filters to the current file —
   * see `lib/symbols.ts`.
   */
  const outline = useMemo(
    () => (active ? outlineFor(symbols, active) : []),
    [symbols, active],
  )

  const mainPanelIds = useMemo(
    () => [
      ...(showSidebar ? ['sidebar'] : []),
      'center',
      ...(showAssistant ? ['assistant'] : []),
    ],
    [showSidebar, showAssistant],
  )
  const centerPanelIds = useMemo(
    () => (showPanel ? ['editor', 'bottom'] : ['editor']),
    [showPanel],
  )

  // ---- commands ----

  const runCommand = useCallback(
    (id: string) => {
      switch (id) {
        case 'run':
        case 'build':
        case 'check':
          void runTool(id, active)
          break
        case 'save':
          if (active) void saveFile(active)
          break
        case 'newfile':
          startPrompt('file')
          break
        case 'newfolder':
          startPrompt('folder')
          break
        case 'packages':
          setView('packages')
          setShowSidebar(true)
          break
        case 'outline':
          setView('outline')
          setShowSidebar(true)
          break
        case 'gotosymbol':
          setPalette({ mode: 'symbol', query: '' })
          break
        case 'files':
          setView('files')
          setShowSidebar(true)
          break
        case 'pkginstall':
          setView('packages')
          void runPkg('install')
          break
        case 'pkgupdate':
          setView('packages')
          void runPkg('update')
          break
        case 'pkgoutdated':
          setView('packages')
          void runPkg('outdated')
          break
        case 'pkgtree':
          setView('packages')
          void runPkg('tree')
          break
        case 'pkgaudit':
          setView('packages')
          void runPkg('audit')
          break
        case 'doctor':
          void runDoctor()
          break
        case 'refresh':
          void refreshTree()
          break
        case 'openfolder':
          void openFolder()
          break
        case 'togglepanel':
          setShowPanel((v) => !v)
          break
        case 'clearlog':
          setLog([])
          break
        case 'terminal':
          showBottom('terminal')
          break
        case 'git':
          showBottom('git')
          break
        case 'preview':
          showBottom('preview')
          break
        case 'assistant':
          setShowAssistant((v) => !v)
          break
        case 'theme':
          setTheme(theme === 'zai-dark' ? 'zai-light' : 'zai-dark')
          break
        case 'diff':
          if (active) void openDiff(active)
          break
        default:
          break
      }
    },
    [
      active,
      runTool,
      saveFile,
      startPrompt,
      runPkg,
      runDoctor,
      refreshTree,
      openFolder,
      showBottom,
      openDiff,
      setTheme,
      theme,
    ],
  )

  // ---- keyboard ----

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (palette || prompt) return // the palette / prompt owns the keyboard while open
      const mod = e.ctrlKey || e.metaKey
      const k = e.key.toLowerCase()
      // The SHIFT variants are tested FIRST. `mod && k === 'p'` also matches
      // Ctrl+Shift+P — `e.key` is `'P'` there and lowercasing makes it
      // indistinguishable from `'p'` — so with the plain test first the
      // command palette was unreachable: Ctrl+Shift+P opened the FILE list.
      if (mod && e.shiftKey && k === 'p') {
        e.preventDefault()
        setPalette({ mode: 'command', query: '' })
      } else if (mod && e.shiftKey && k === 'o') {
        e.preventDefault()
        setPalette({ mode: 'symbol', query: '' })
      } else if (mod && k === 'p') {
        e.preventDefault()
        setPalette({ mode: 'file', query: '' })
      } else if (mod && k === 'o') {
        e.preventDefault()
        void openFolder()
      } else if (mod && k === 'b') {
        e.preventDefault()
        setShowSidebar((v) => !v)
      } else if (mod && k === 'j') {
        e.preventDefault()
        setShowPanel((v) => !v)
      } else if (e.ctrlKey && e.key === '`') {
        e.preventDefault()
        showBottom('terminal')
      } else if (mod && e.shiftKey && k === 'a') {
        e.preventDefault()
        setShowAssistant((v) => !v)
      } else if (mod && e.shiftKey && k === 'g') {
        e.preventDefault()
        showBottom('git')
      } else if (mod && k === 's') {
        e.preventDefault()
        if (active) void saveFile(active)
      } else if (e.key === 'F5') {
        e.preventDefault()
        void runTool('run', active)
      } else if (e.key === 'F6') {
        e.preventDefault()
        void runTool('build', active)
      } else if (e.key === 'F7') {
        e.preventDefault()
        void runTool('check', active)
      } else if (e.key === 'Escape') {
        setPalette(null)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [active, palette, prompt, saveFile, runTool, openFolder, showBottom])

  // ---- editor callbacks (stable identities keep Monaco's registry sane) ----

  const onEditorChange: EditorProps['onChange'] = useCallback((path, text) => {
    setDocs((prev) => {
      const d = prev.get(path)
      if (!d || d.text === text) return prev
      const next = new Map(prev)
      next.set(path, { ...d, text })
      return next
    })
  }, [])

  const onEditorCursor: EditorProps['onCursor'] = useCallback((path, line, column) => {
    setDocs((prev) => {
      const d = prev.get(path)
      if (!d || (d.cursor.line === line && d.cursor.column === column)) return prev
      const next = new Map(prev)
      next.set(path, { ...d, cursor: { line, column } })
      return next
    })
  }, [])

  const onEditorSave = useCallback(
    (path: string) => {
      void saveFile(path)
    },
    [saveFile],
  )

  const onGoToDefinition = useCallback(
    (_path: string, line: number, column: number) => {
      void goToDefinition(line, column)
    },
    [goToDefinition],
  )

  return (
    <TooltipProvider delayDuration={350}>
      <div className="wb-root">
        <header className="wb-titlebar">
          <div className="flex min-w-0 items-center gap-2">
            <div className="flex size-7 shrink-0 items-center justify-center rounded-lg bg-foreground text-ui-xs font-bold tracking-tight text-background">
              ax
            </div>
            <div className="flex min-w-0 flex-col leading-tight">
              <span className="text-ui-sm font-semibold text-foreground">Aoxn IDE</span>
              <span className="max-w-52 truncate text-ui-xs text-foreground-subtlest" title={root}>
                {root ? basename(root) || root : 'No folder open'}
              </span>
            </div>
            {git?.isRepo ? (
              <Badge className="ml-1 hidden max-w-36 truncate md:inline-flex" variant="secondary">
                <GitBranchIcon className="size-2.5" /> {git.branch || 'detached'}
              </Badge>
            ) : null}
          </div>

          <button
            className="mx-auto hidden h-7 w-[min(420px,36vw)] items-center gap-2 rounded-lg border border-border bg-surface px-2.5 text-ui-sm text-foreground-subtle transition-colors hover:border-border-hover hover:text-foreground md:flex"
            onClick={() => setPalette({ mode: 'file', query: '' })}
            type="button"
          >
            <SearchIcon className="size-3.5" />
            <span className="flex-1 text-left">Search files or commands</span>
            <Kbd>Ctrl</Kbd>
            <Kbd>P</Kbd>
          </button>

          <div className="flex items-center gap-0.5">
            <ChromeButton label="Type check (F7)" onClick={() => void runTool('check', active)}>
              <CheckIcon className="size-3.5" />
            </ChromeButton>
            <ChromeButton label="Build (F6)" onClick={() => void runTool('build', active)}>
              <HammerIcon className="size-3.5" />
            </ChromeButton>
            <ChromeButton label="Run (F5)" onClick={() => void runTool('run', active)}>
              <PlayIcon className="size-3.5" />
            </ChromeButton>
            <Separator className="mx-1 h-4" orientation="vertical" />
            <ChromeButton
              active={showAssistant}
              label="Assistant (Ctrl+Shift+A)"
              onClick={() => setShowAssistant((value) => !value)}
            >
              <SparklesIcon className="size-3.5" />
            </ChromeButton>
            <ChromeButton
              label="Toggle theme"
              onClick={() => setTheme(theme === 'zai-dark' ? 'zai-light' : 'zai-dark')}
            >
              {resolved === 'dark' ? <SunIcon className="size-3.5" /> : <MoonIcon className="size-3.5" />}
            </ChromeButton>
            <ChromeButton
              label="Command palette (Ctrl+Shift+P)"
              onClick={() => setPalette({ mode: 'command', query: '' })}
            >
              <CommandIcon className="size-3.5" />
            </ChromeButton>
          </div>
        </header>

        <div className="wb-main">
          <aside className="flex w-11 flex-none flex-col items-center gap-1 border-r border-border bg-header py-2">
            <RailButton
              active={showSidebar && view === 'files'}
              label="Explorer (Ctrl+B)"
              onClick={() => {
                setView('files')
                setShowSidebar(true)
              }}
            >
              <FilesIcon className="size-4" />
            </RailButton>
            <RailButton
              active={showSidebar && view === 'outline'}
              label="Outline"
              onClick={() => {
                setView('outline')
                setShowSidebar(true)
              }}
            >
              <ListTreeIcon className="size-4" />
            </RailButton>
            <RailButton
              active={showSidebar && view === 'packages'}
              label="Packages"
              onClick={() => {
                setView('packages')
                setShowSidebar(true)
              }}
            >
              <PackageIcon className="size-4" />
            </RailButton>
            <div className="my-1 h-px w-5 bg-border" />
            <RailButton label="Search files (Ctrl+P)" onClick={() => setPalette({ mode: 'file', query: '' })}>
              <SearchIcon className="size-4" />
            </RailButton>
            <RailButton
              label="Go to symbol (Ctrl+Shift+O)"
              onClick={() => setPalette({ mode: 'symbol', query: '' })}
            >
              <BracesIcon className="size-4" />
            </RailButton>
            <RailButton
              active={showPanel && bottomTab === 'git'}
              label="Source control (Ctrl+Shift+G)"
              onClick={() => showBottom('git')}
            >
              <GitBranchIcon className="size-4" />
            </RailButton>
            <RailButton
              active={showPanel && bottomTab === 'terminal'}
              label="Terminal (Ctrl+`)"
              onClick={() => showBottom('terminal')}
            >
              <SquareTerminalIcon className="size-4" />
            </RailButton>
            <RailButton
              active={showPanel && bottomTab === 'preview'}
              label="File preview"
              onClick={() => showBottom('preview')}
            >
              <Code2Icon className="size-4" />
            </RailButton>
            <div className="flex-1" />
            <RailButton label="Run (F5)" onClick={() => void runTool('run', active)}>
              <PlayIcon className="size-4" />
            </RailButton>
            <RailButton label="Open folder (Ctrl+O)" onClick={() => void openFolder()}>
              <FolderOpenIcon className="size-4" />
            </RailButton>
            <RailButton label="Toolchain doctor" onClick={() => void runDoctor()}>
              <WrenchIcon className="size-4" />
            </RailButton>
            <RailButton
              label="Commands (Ctrl+Shift+P)"
              onClick={() => setPalette({ mode: 'command', query: '' })}
            >
              <SettingsIcon className="size-4" />
            </RailButton>
          </aside>

          <ResizablePanelGroup
            className="min-w-0 flex-1"
            defaultLayout={showSidebar && showAssistant ? { sidebar: 18, center: 56, assistant: 26 } : undefined}
            layoutId="aoxn-workbench-main-zcode"
            orientation="horizontal"
            panelIds={mainPanelIds}
          >
            {showSidebar ? (
              <>
                <ResizablePanel defaultSize="18%" id="sidebar" maxSize="32%" minSize="14%">
                  <div className="h-full min-h-0">
                    {view === 'files' ? (
                      <FileExplorer
                        expanded={expanded}
                        onExpandedChange={setExpanded}
                        onNewFile={() => startPrompt('file')}
                        onNewFolder={() => startPrompt('folder')}
                        onOpenFolder={() => void openFolder()}
                        onRefresh={() => void refreshTree()}
                        onSearch={() => setPalette({ mode: 'file', query: '' })}
                        onSelect={(path, isDir) => {
                          setSelected(path)
                          if (!isDir) void openFile(path)
                        }}
                        root={root}
                        selected={selected}
                        tree={tree}
                      />
                    ) : view === 'outline' ? (
                      <OutlinePanel
                        busy={busy}
                        error={symbolError}
                        onJump={(line) => {
                          if (active) void revealSymbol(active, line, 1)
                        }}
                        onRefresh={() => void refreshSymbols(active)}
                        onSearch={() => setPalette({ mode: 'symbol', query: '' })}
                        path={active}
                        rows={outline}
                      />
                    ) : (
                      <PackagesPanel
                        busy={busy}
                        onAdd={() => startPrompt('pkgadd')}
                        onRefresh={() => void refreshPkg()}
                        onRemove={() => startPrompt('pkgremove')}
                        onRun={(sub, arg) => void runPkg(sub, arg)}
                        onWhy={() => startPrompt('pkgwhy')}
                        pkg={pkg}
                        report={pkgInfo}
                      />
                    )}
                  </div>
                </ResizablePanel>
                <ResizableHandle />
              </>
            ) : null}

            <ResizablePanel id="center" minSize="30%">
              <div className="flex h-full min-h-0 flex-col bg-panel">
                <div className="flex h-9 flex-none items-center gap-1 border-b border-border bg-header px-1.5">
                  {diff ? (
                    <div className="flex h-7 max-w-64 items-center gap-1.5 rounded-md bg-tab-active px-2 text-ui-sm text-foreground">
                      <Code2Icon className="size-3.5 text-brand" />
                      <span className="truncate">Diff · {basename(diff.path)}</span>
                      <button
                        className="ml-1 rounded-sm p-0.5 text-foreground-subtle hover:bg-surface-hover hover:text-foreground"
                        onClick={() => setDiff(null)}
                        title="Close diff"
                        type="button"
                      >
                        <XIcon className="size-3" />
                      </button>
                    </div>
                  ) : (
                    <>
                      {tabs.map((path) => {
                        const doc = docs.get(path)
                        if (!doc) return null
                        const dirty = doc.text !== doc.saved
                        return (
                          <div
                            className={cn(
                              'group flex h-7 max-w-56 items-center gap-1.5 rounded-md px-2 text-ui-sm transition-colors',
                              path === active
                                ? 'bg-tab-active text-foreground'
                                : 'text-foreground-subtle hover:bg-surface-hover hover:text-foreground',
                            )}
                            key={path}
                            onClick={() => {
                              setActive(path)
                              setDiff(null)
                            }}
                            title={path}
                          >
                            <FileCode2Icon className="size-3.5 shrink-0 text-brand" />
                            <span className="truncate">{basename(path)}</span>
                            {dirty ? (
                              <span
                                className="size-1.5 shrink-0 rounded-full bg-brand"
                                onClick={(event) => {
                                  event.stopPropagation()
                                  void saveFile(path)
                                }}
                                title="Unsaved — click to save"
                              />
                            ) : null}
                            <button
                              className="rounded-sm p-0.5 opacity-0 hover:bg-surface-hover group-hover:opacity-100"
                              onClick={(event) => {
                                event.stopPropagation()
                                closeTab(path)
                              }}
                              title="Close"
                              type="button"
                            >
                              <XIcon className="size-3" />
                            </button>
                          </div>
                        )
                      })}
                      <button
                        aria-label="New file"
                        className="flex size-7 items-center justify-center rounded-md text-foreground-subtle hover:bg-surface-hover hover:text-foreground"
                        onClick={() => startPrompt('file')}
                        type="button"
                      >
                        <FilesIcon className="size-3.5" />
                      </button>
                    </>
                  )}
                  <div className="flex-1" />
                  <ChromeButton label="Run (F5)" onClick={() => void runTool('run', active)}>
                    <PlayIcon className="size-3.5" />
                  </ChromeButton>
                </div>

                <ResizablePanelGroup
                  className="min-h-0 flex-1"
                  defaultLayout={showPanel ? { editor: 72, bottom: 28 } : undefined}
                  layoutId="aoxn-workbench-center-zcode"
                  orientation="vertical"
                  panelIds={centerPanelIds}
                >
                  <ResizablePanel id="editor" minSize="30%">
                    <div className="wb-editor-surface h-full">
                      {diff ? (
                        <DiffView
                          modified={diff.modified}
                          original={diff.original}
                          path={diff.path}
                          theme={monacoTheme}
                        />
                      ) : activeDoc ? (
                        <Editor
                          diagnostics={activeDiags}
                          jump={jump}
                          onChange={onEditorChange}
                          onCursor={onEditorCursor}
                          onGoToDefinition={onGoToDefinition}
                          onSave={onEditorSave}
                          path={activeDoc.path}
                          text={activeDoc.text}
                          theme={monacoTheme}
                        />
                      ) : (
                        <div className="wb-empty">
                          <div className="max-w-xl text-center">
                            <div className="mx-auto mb-4 flex size-14 items-center justify-center rounded-2xl bg-accent text-brand">
                              <SparklesIcon className="size-6" />
                            </div>
                            <h1 className="text-ui-xl font-semibold text-foreground">Aoxn IDE</h1>
                            <p className="mt-1 text-ui-sm text-foreground-subtle">
                              An editor and build runner for the Aoxn language, with a ZCode-style
                              assistant and workspace panels.
                            </p>
                            <div className="mt-5 grid grid-cols-2 gap-x-6 gap-y-1.5 text-left text-ui-sm">
                              <span className="text-foreground-subtle">Files</span>
                              <span className="flex justify-end gap-1"><Kbd>Ctrl</Kbd><Kbd>P</Kbd></span>
                              <span className="text-foreground-subtle">Commands</span>
                              <span className="flex justify-end gap-1"><Kbd>Ctrl</Kbd><Kbd>Shift</Kbd><Kbd>P</Kbd></span>
                              <span className="text-foreground-subtle">Run · Build · Check</span>
                              <span className="flex justify-end gap-1"><Kbd>F5</Kbd><Kbd>F6</Kbd><Kbd>F7</Kbd></span>
                              <span className="text-foreground-subtle">Terminal · Assistant</span>
                              <span className="flex justify-end gap-1"><Kbd>Ctrl</Kbd><Kbd>`</Kbd><Kbd>Ctrl</Kbd><Kbd>Shift</Kbd><Kbd>A</Kbd></span>
                            </div>
                            <div className="mt-5 flex justify-center gap-2">
                              <Button onClick={() => void openFolder()} type="button">
                                Open folder
                              </Button>
                              <Button
                                onClick={() => setPalette({ mode: 'command', query: '' })}
                                type="button"
                                variant="outline"
                              >
                                Commands
                              </Button>
                            </div>
                          </div>
                        </div>
                      )}
                    </div>
                  </ResizablePanel>

                  {showPanel ? (
                    <>
                      <ResizableHandle />
                      <ResizablePanel defaultSize="28%" id="bottom" maxSize="65%" minSize="12%">
                        <div className="h-full min-h-0">
                          <Tabs
                            className="flex h-full min-h-0 flex-col gap-0"
                            onValueChange={(value) => setBottomTab(value as BottomTab)}
                            value={bottomTab}
                          >
                            <div className="flex h-8 flex-none items-center border-b border-border bg-header px-2">
                              <TabsList className="h-7 bg-transparent p-0" variant="line">
                                <TabsTrigger value="output">Output</TabsTrigger>
                                <TabsTrigger value="terminal">Terminal</TabsTrigger>
                                <TabsTrigger value="git">
                                  Source Control
                                  {git && git.changes.length > 0 ? (
                                    <Badge className="ml-1 h-4 px-1.5 text-ui-2xs" variant="secondary">
                                      {git.changes.length}
                                    </Badge>
                                  ) : null}
                                </TabsTrigger>
                                <TabsTrigger value="preview">Preview</TabsTrigger>
                              </TabsList>
                              <div className="flex-1" />
                              {bottomTab === 'output' ? (
                                <Button
                                  aria-label="Clear output"
                                  disabled={log.length === 0}
                                  onClick={() => setLog([])}
                                  size="icon-xs"
                                  type="button"
                                  variant="ghost"
                                >
                                  <XIcon className="size-3" />
                                </Button>
                              ) : null}
                              <Button
                                aria-label="Hide panel"
                                onClick={() => setShowPanel(false)}
                                size="icon-xs"
                                type="button"
                                variant="ghost"
                              >
                                <PanelBottomIcon className="size-3" />
                              </Button>
                            </div>
                            <div className="min-h-0 flex-1">
                              <TabsContent className="m-0 h-full min-h-0" value="output">
                                <OutputPanel log={log} onJump={jumpToLogLine} />
                              </TabsContent>
                              <TabsContent
                                className="m-0 h-full min-h-0 data-[state=inactive]:hidden"
                                forceMount
                                value="terminal"
                              >
                                <TerminalPanel root={root} />
                              </TabsContent>
                              <TabsContent className="m-0 h-full min-h-0" value="git">
                                <GitPanel
                                  onOpenDiff={(path) => void openDiff(path)}
                                  onStatus={setGit}
                                />
                              </TabsContent>
                              <TabsContent className="m-0 h-full min-h-0" value="preview">
                                {previewPath || active ? (
                                  <PreviewPanel
                                    path={(previewPath ?? active) as string}
                                    text={
                                      activeDoc && (previewPath ?? active) === activeDoc.path
                                        ? activeDoc.text
                                        : undefined
                                    }
                                  />
                                ) : (
                                  <div className="wb-empty">
                                    <div className="text-ui-sm text-foreground-subtle">
                                      Open a file to preview it.
                                    </div>
                                  </div>
                                )}
                              </TabsContent>
                            </div>
                          </Tabs>
                        </div>
                      </ResizablePanel>
                    </>
                  ) : null}
                </ResizablePanelGroup>
              </div>
            </ResizablePanel>

            {showAssistant ? (
              <>
                <ResizableHandle />
                <ResizablePanel defaultSize="26%" id="assistant" maxSize="45%" minSize="18%">
                  <div className="h-full min-h-0 border-l border-border">
                    <ChatPanel activePath={activeDoc?.path ?? null} activeText={activeDoc?.text ?? ''} />
                  </div>
                </ResizablePanel>
              </>
            ) : null}
          </ResizablePanelGroup>
        </div>

        <StatusBar
          active={activeDoc}
          busy={busy}
          errorCount={diagnostics.length}
          onBuild={() => void runTool('build', active)}
          onCheck={() => void runTool('check', active)}
          onDoctor={() => void runDoctor()}
          onOpenFolder={() => void openFolder()}
          onRun={() => void runTool('run', active)}
          onSave={() => active && void saveFile(active)}
          onTogglePanel={() => setShowPanel((value) => !value)}
          showPanel={showPanel}
          toolchain={toolchain}
        />

        {palette ? (
          <CommandPalette
            busy={busy}
            mode={palette.mode}
            onClose={() => setPalette(null)}
            onCommand={(id) => {
              setPalette(null)
              runCommand(id)
            }}
            onPickFile={(path) => {
              setPalette(null)
              void openFile(path)
            }}
            onPickSymbol={(path, line, column) => {
              setPalette(null)
              void revealSymbol(path, line, column)
            }}
            onQuery={(query) => setPalette({ ...palette, query })}
            query={palette.query}
            symbols={symbols}
            tree={tree}
          />
        ) : null}

        {prompt ? (
          <PromptDialog
            dir={prompt.dir}
            mode={prompt.mode}
            onCancel={() => setPrompt(null)}
            onConfirm={(name) => confirmPrompt(prompt.mode, prompt.dir, name)}
          />
        ) : null}

        {toast ? (
          <div className="fixed bottom-7 left-1/2 z-[70] -translate-x-1/2 rounded-lg border border-border bg-popover px-3.5 py-2 text-ui-sm text-popover-foreground shadow-md">
            {toast}
          </div>
        ) : null}
      </div>
    </TooltipProvider>
  )
}

// ---- chrome buttons ----

function RailButton({
  active = false,
  label,
  onClick,
  children,
}: {
  active?: boolean
  label: string
  onClick(): void
  children: React.ReactNode
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          aria-label={label}
          aria-pressed={active}
          className={cn(
            'flex size-8 items-center justify-center rounded-lg transition-colors',
            active
              ? 'bg-selected text-foreground'
              : 'text-foreground-subtle hover:bg-surface-hover hover:text-foreground',
          )}
          onClick={onClick}
          type="button"
        >
          {children}
        </button>
      </TooltipTrigger>
      <TooltipContent side="right">{label}</TooltipContent>
    </Tooltip>
  )
}

function ChromeButton({
  active = false,
  label,
  onClick,
  children,
}: {
  active?: boolean
  label: string
  onClick(): void
  children: React.ReactNode
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          aria-label={label}
          aria-pressed={active}
          className="wb-chrome-button"
          data-active={active}
          onClick={onClick}
          type="button"
        >
          {children}
        </button>
      </TooltipTrigger>
      <TooltipContent side="bottom">{label}</TooltipContent>
    </Tooltip>
  )
}

// ---- outline panel ----

/**
 * The declarations of the file on screen, as the compiler reported them.
 *
 * Every row is a jump: clicking one puts the caret on the declaration's
 * line. That is the whole feature — the list exists so a reader can see
 * what a file declares without scrolling it, and reach any of it in one
 * click.
 *
 * The panel is explicit about the three states it can be in, because they
 * look identical if they are not stated: no file open, a file that does not
 * parse, and a file that declares nothing. The middle one matters most —
 * `aoxn symbols` refuses a broken file on purpose (half an outline sends
 * the reader to a declaration that is not there), so the panel says WHY it
 * is empty instead of showing an empty list that reads like "no
 * declarations".
 */
function OutlinePanel({
  rows,
  error,
  busy,
  path,
  onJump,
  onRefresh,
  onSearch,
}: {
  rows: OutlineRow[]
  error: string
  busy: boolean
  path: string | null
  onJump(line: number): void
  onRefresh(): void
  onSearch(): void
}) {
  return (
    <div className="flex h-full min-h-0 flex-col bg-sidebar">
      <div className="flex h-9 flex-none items-center gap-1.5 border-b border-border px-2.5">
        <BracesIcon className="size-3.5 text-foreground-subtle" />
        <span className="min-w-0 flex-1 truncate text-ui-sm font-semibold" title={path ?? ''}>
          {path ? basename(path) : 'Outline'}
        </span>
        <Button aria-label="Search declarations" onClick={onSearch} size="icon-xs" type="button" variant="ghost">
          <SearchIcon className="size-3.5" />
        </Button>
        <Button aria-label="Refresh outline" onClick={onRefresh} size="icon-xs" type="button" variant="ghost">
          <RefreshCwIcon className={cn('size-3.5', busy && 'animate-spin')} />
        </Button>
      </div>
      <div className="min-h-0 flex-1 overflow-auto p-1.5">
        {!path ? (
          <div className="px-2 py-6 text-center text-ui-sm text-foreground-subtle">
            Open a file to see what it declares.
          </div>
        ) : error ? (
          <div className="m-1 flex items-start gap-2 rounded-lg border border-destructive/30 bg-destructive-surface px-2.5 py-2 text-ui-sm text-destructive-surface-foreground">
            <CircleAlertIcon className="mt-0.5 size-3.5 shrink-0" />
            <span>{error}</span>
          </div>
        ) : rows.length === 0 ? (
          <div className="px-2 py-6 text-center text-ui-sm text-foreground-subtle">
            {busy ? 'Asking the compiler…' : 'No top-level declarations in this file.'}
          </div>
        ) : (
          <div className="flex flex-col gap-0.5" role="list">
            {rows.map(({ symbol }) => (
              <button
                className="flex w-full items-center gap-2 rounded-md px-2 py-1 text-left transition-colors hover:bg-surface-hover"
                key={`${symbol.file}:${symbol.line}:${symbol.name}`}
                onClick={() => onJump(symbol.line)}
                role="listitem"
                title={symbol.signature}
                type="button"
              >
                <span className="flex size-4 shrink-0 items-center justify-center rounded-sm bg-surface font-mono text-ui-2xs text-foreground-subtle">
                  {symbol.kind === 'struct' ? 'S' : 'ƒ'}
                </span>
                <span className="min-w-0 flex-1 truncate text-ui-sm text-foreground">
                  {symbol.name}
                </span>
                <span className="shrink-0 font-mono text-ui-xs text-foreground-subtlest">
                  {symbol.line}
                </span>
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}

// ---- output panel ----

function OutputPanel({
  log,
  onJump,
}: {
  log: LogEntry[]
  onJump(raw: string): void
}) {
  return (
    <div className="flex h-full min-h-0 flex-col bg-terminal-bg">
      <div className="min-h-0 flex-1 overflow-auto px-3 py-2 font-mono text-ui-sm leading-relaxed">
        {log.length === 0 ? (
          <div className="text-ui-sm text-foreground-subtlest">
            Nothing yet — F5 runs the current file, F7 type-checks it without invoking clang.
          </div>
        ) : (
          log.map((entry) => {
            const diagnostic = parseDiagnostic(entry.text)
            const clickable = diagnostic !== null && diagnostic.file.length > 0
            return (
              <div
                className={cn(
                  'whitespace-pre-wrap break-words',
                  entry.kind === 'cmd' && 'font-semibold text-foreground',
                  entry.kind === 'err' && 'text-destructive',
                  entry.kind === 'meta' && 'text-foreground-subtle',
                  clickable && 'cursor-pointer underline decoration-dotted underline-offset-2',
                )}
                key={entry.id}
                onClick={clickable ? () => onJump(entry.text) : undefined}
                title={clickable ? 'Go to this line' : undefined}
              >
                {entry.text}
              </div>
            )
          })
        )}
      </div>
    </div>
  )
}

// ---- status bar ----

function StatusBar(props: {
  toolchain: ToolchainInfo | null
  busy: boolean
  errorCount: number
  active: Doc | null
  showPanel: boolean
  onTogglePanel(): void
  onCheck(): void
  onBuild(): void
  onRun(): void
  onSave(): void
  onDoctor(): void
  onOpenFolder(): void
}) {
  const { toolchain, busy, errorCount, active, showPanel } = props
  const lines = active ? active.text.split('\n').length : 0
  const dirty = active ? active.text !== active.saved : false

  return (
    <footer className="wb-statusbar">
      <span className="flex items-center gap-1">
        {busy ? (
          <span className="flex items-center gap-1 px-1">
            <RefreshCwIcon className="size-3 animate-spin" /> working
          </span>
        ) : (
          <span className="px-1">ready</span>
        )}
        {dirty ? (
          <button
            className="flex items-center gap-1 rounded-sm px-1 py-0.5 hover:bg-surface-hover"
            onClick={props.onSave}
            title="Save (Ctrl+S)"
            type="button"
          >
            <span className="size-1.5 rounded-full bg-brand" /> unsaved
          </button>
        ) : null}
        {errorCount > 0 ? (
          <button
            className="flex items-center gap-1 rounded-sm px-1 py-0.5 text-destructive hover:bg-surface-hover"
            onClick={props.onCheck}
            title="Show the problems in the current file"
            type="button"
          >
            <CircleAlertIcon className="size-3" /> {errorCount}
          </button>
        ) : null}
      </span>

      <span className="flex-1" />

      <span className="flex items-center gap-1">
        {active ? (
          <span className="px-1">
            Ln {active.cursor.line}, Col {active.cursor.column}
          </span>
        ) : null}
        {active ? (
          <span className="hidden px-1 sm:inline">
            {lines} {lines === 1 ? 'line' : 'lines'}
          </span>
        ) : null}
        <button
          className="rounded-sm px-1 py-0.5 hover:bg-surface-hover"
          onClick={props.onCheck}
          title="Type check (F7)"
          type="button"
        >
          Check
        </button>
        <button
          className="rounded-sm px-1 py-0.5 hover:bg-surface-hover"
          onClick={props.onBuild}
          title="Build (F6)"
          type="button"
        >
          Build
        </button>
        <button
          className="rounded-sm px-1 py-0.5 hover:bg-surface-hover"
          onClick={props.onRun}
          title="Run (F5)"
          type="button"
        >
          Run
        </button>
        <button
          className="rounded-sm px-1 py-0.5 hover:bg-surface-hover"
          onClick={props.onTogglePanel}
          title="Toggle panel (Ctrl+J)"
          type="button"
        >
          Panel {showPanel ? '▾' : '▸'}
        </button>
        {toolchain && !toolchain.compilerFound ? (
          <button
            className="flex items-center gap-1 rounded-sm px-1 py-0.5 text-warning hover:bg-surface-hover"
            onClick={props.onDoctor}
            title="Run aoxn doctor — the toolchain's own self-check"
            type="button"
          >
            <CircleAlertIcon className="size-3" /> compiler not found
          </button>
        ) : toolchain && !toolchain.clangFound ? (
          <button
            className="flex items-center gap-1 rounded-sm px-1 py-0.5 text-warning hover:bg-surface-hover"
            onClick={props.onDoctor}
            title="Run aoxn doctor — builds fail without clang"
            type="button"
          >
            <CircleAlertIcon className="size-3" /> clang not found
          </button>
        ) : toolchain ? (
          <span className="flex items-center gap-1 px-1" title={toolchain.version || toolchain.compiler}>
            <CheckIcon className="size-3 text-success" /> {toolchain.compiler}
          </span>
        ) : null}
      </span>
    </footer>
  )
}

// ---- command palette / quick open ----

const COMMANDS: { id: string; title: string }[] = [
  { id: 'run', title: 'Run the current file' },
  { id: 'build', title: 'Build the current file' },
  { id: 'check', title: 'Type check the current file' },
  { id: 'save', title: 'Save' },
  { id: 'newfile', title: 'New file…' },
  { id: 'newfolder', title: 'New folder…' },
  { id: 'packages', title: 'Show the packages panel' },
  { id: 'outline', title: 'Show the outline (the file’s declarations)' },
  { id: 'gotosymbol', title: 'Go to symbol… (Ctrl+Shift+O)' },
  { id: 'pkginstall', title: 'Packages: install (aoxn pkg install)' },
  { id: 'pkgupdate', title: 'Packages: update (aoxn pkg update)' },
  { id: 'pkgoutdated', title: 'Packages: list outdated (aoxn pkg outdated)' },
  { id: 'pkgtree', title: 'Packages: show the dependency tree (aoxn pkg tree)' },
  { id: 'pkgaudit', title: 'Packages: audit the dependency set (aoxn pkg audit)' },
  { id: 'doctor', title: 'Run aoxn doctor (toolchain self-check)' },
  { id: 'refresh', title: 'Refresh the explorer' },
  { id: 'openfolder', title: 'Open folder…' },
  { id: 'togglepanel', title: 'Toggle the bottom panel' },
  { id: 'clearlog', title: 'Clear the output panel' },
  { id: 'terminal', title: 'Show the terminal' },
  { id: 'git', title: 'Show source control' },
  { id: 'preview', title: 'Show the file preview' },
  { id: 'assistant', title: 'Toggle the assistant panel' },
  { id: 'diff', title: 'Diff the current file against HEAD' },
  { id: 'theme', title: 'Toggle the light / dark theme' },
]

function CommandPalette({
  mode,
  query,
  tree,
  symbols,
  busy,
  onQuery,
  onClose,
  onPickFile,
  onPickSymbol,
  onCommand,
}: {
  mode: 'file' | 'command' | 'symbol'
  query: string
  tree: TreeNode[]
  symbols: SymbolTable
  busy: boolean
  onQuery(q: string): void
  onClose(): void
  onPickFile(path: string): void
  onPickSymbol(path: string, line: number, column: number): void
  onCommand(id: string): void
}) {
  const [index, setIndex] = useState(0)
  const q = query.toLowerCase()

  const rows = useMemo(() => {
    if (mode === 'file') {
      return tree
        .filter((n) => !n.isDir && n.path.toLowerCase().includes(q))
        .slice(0, 60)
        .map((n) => ({ key: n.path, label: n.name, hint: '', pick: () => onPickFile(n.path) }))
    }
    if (mode === 'symbol') {
      // Ranked, not filtered — `lib/symbols.ts` owns the ordering so it can
      // be tested without a render. The origin column is what makes a
      // cross-file result usable: the reader sees which file it will open
      // before pressing Enter.
      return searchSymbols(symbols, query, 60).map((s) => ({
        key: `${s.file}:${s.line}:${s.name}`,
        label: symbolLabel(s),
        hint: symbolOrigin(s),
        pick: () => onPickSymbol(s.file, s.line, Math.max(1, s.col)),
      }))
    }
    return COMMANDS.filter((c) => c.title.toLowerCase().includes(q))
      .slice(0, 60)
      .map((c) => ({ key: c.id, label: c.title, hint: '', pick: () => onCommand(c.id) }))
  }, [mode, tree, symbols, query, q, onPickFile, onPickSymbol, onCommand])

  useEffect(() => setIndex(0), [query, mode])

  return (
    <div
      className="fixed inset-0 z-[60] flex items-start justify-center bg-black/30 pt-[12vh] backdrop-blur-[1px]"
      onMouseDown={onClose}
    >
      <div
        className="w-[min(680px,92vw)] overflow-hidden rounded-xl border border-border bg-popover shadow-xl"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <input
          // eslint-disable-next-line jsx-a11y/no-autofocus -- a quick-open has to take focus
          autoFocus
          className="h-11 w-full border-b border-border bg-transparent px-3.5 text-ui-base text-foreground outline-none placeholder:text-foreground-subtlest"
          onChange={(event) => onQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'ArrowDown') {
              event.preventDefault()
              setIndex((current) => Math.min(rows.length - 1, current + 1))
            } else if (event.key === 'ArrowUp') {
              event.preventDefault()
              setIndex((current) => Math.max(0, current - 1))
            } else if (event.key === 'Enter') {
              event.preventDefault()
              rows[index]?.pick()
            } else if (event.key === 'Escape') {
              onClose()
            }
          }}
          placeholder={
            mode === 'file'
              ? 'Search files by name…'
              : mode === 'symbol'
                ? 'Search declarations by name…'
                : 'Type a command…'
          }
          spellCheck={false}
          value={query}
        />
        <div className="max-h-[50vh] overflow-auto p-1.5">
          {rows.length === 0 ? (
            <div className="px-3 py-4 text-center text-ui-sm text-foreground-subtle">No matches.</div>
          ) : null}
          {rows.map((row, i) => (
            <div
              className={cn(
                'flex cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-ui-sm',
                i === index ? 'bg-selected text-foreground' : 'text-foreground-subtle',
              )}
              key={row.key}
              onClick={row.pick}
              onMouseEnter={() => setIndex(i)}
            >
              {mode === 'file' ? (
                <FileCode2Icon className="size-3.5 shrink-0 text-brand" />
              ) : mode === 'symbol' ? (
                <BracesIcon className="size-3.5 shrink-0 text-icon-purple" />
              ) : (
                <CommandIcon className="size-3.5 shrink-0 text-foreground-subtle" />
              )}
              <span className="min-w-0 flex-1 truncate">{row.label}</span>
              {row.hint ? (
                <span className="shrink-0 text-ui-xs text-foreground-subtlest">{row.hint}</span>
              ) : null}
            </div>
          ))}
        </div>
        <div className="border-t border-border px-3 py-1.5 text-ui-xs text-foreground-subtlest">
          {mode === 'file'
            ? 'Enter opens the file'
            : mode === 'symbol'
              ? 'Enter jumps to the declaration'
              : 'Enter runs the command'}
          {busy ? ' · a build is running' : ''}
        </div>
      </div>
    </div>
  )
}

// ---- prompt (new file / new folder / package name) ----

/** Title, placeholder and validation for each prompt mode. */
const PROMPT_SPEC: Record<
  PromptMode,
  { title: (dir: string) => string; placeholder: string; validate(v: string): string | null; hint: string }
> = {
  file: {
    title: (dir) => `New file in ${basename(dir) || dir}`,
    placeholder: 'name.ax — subfolders allowed, e.g. src/util.ax',
    validate: validateEntryName,
    hint: 'Enter creates · Esc cancels',
  },
  folder: {
    title: (dir) => `New folder in ${basename(dir) || dir}`,
    placeholder: 'folder name — nesting allowed, e.g. gen/lib',
    validate: validateEntryName,
    hint: 'Enter creates · Esc cancels',
  },
  pkgadd: {
    title: () => 'Add a package (aoxn pkg add)',
    placeholder: 'name or name@req, e.g. http@^2',
    validate: validatePkgName,
    hint: 'Enter runs aoxn pkg add · Esc cancels',
  },
  pkgwhy: {
    title: () => 'Why is a package installed? (aoxn pkg why)',
    placeholder: 'package name, e.g. http',
    validate: validatePkgName,
    hint: 'Enter runs aoxn pkg why · Esc cancels',
  },
  pkgremove: {
    title: () => 'Remove a package (aoxn pkg remove)',
    placeholder: 'package name, e.g. http',
    validate: validatePkgName,
    hint: 'Enter runs aoxn pkg remove · Esc cancels',
  },
}

/**
 * A one-input dialog. The input owns its error state: validation
 * (`lib/paths.ts` / `lib/pkg.ts`) and backend refusals ("already exists")
 * surface as a line in the dialog, not as a toast that vanishes before the
 * user has read it.
 */
function PromptDialog({
  mode,
  dir,
  onConfirm,
  onCancel,
}: {
  mode: PromptMode
  dir: string
  onConfirm(name: string): Promise<void>
  onCancel(): void
}) {
  const [value, setValue] = useState('')
  const [error, setError] = useState('')
  const spec = PROMPT_SPEC[mode]

  const submit = async () => {
    const invalid = spec.validate(value)
    if (invalid) {
      setError(invalid)
      return
    }
    try {
      await onConfirm(value)
      onCancel() // done — the state is already refreshed
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ''))
    }
  }

  return (
    <div
      className="fixed inset-0 z-[60] flex items-start justify-center bg-black/30 pt-[18vh] backdrop-blur-[1px]"
      onMouseDown={onCancel}
    >
      <div
        className="w-[min(520px,92vw)] rounded-xl border border-border bg-popover p-4 shadow-xl"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="mb-3 text-ui-base font-semibold text-foreground">{spec.title(dir)}</div>
        <Input
          // eslint-disable-next-line jsx-a11y/no-autofocus -- a prompt has to take focus
          autoFocus
          onChange={(event) => {
            setValue(event.target.value)
            setError('')
          }}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault()
              void submit()
            } else if (event.key === 'Escape') {
              onCancel()
            }
          }}
          placeholder={spec.placeholder}
          spellCheck={false}
          value={value}
        />
        {error ? <div className="mt-2 text-ui-sm text-destructive">{error}</div> : null}
        <div className="mt-2 text-ui-xs text-foreground-subtlest">{spec.hint}</div>
      </div>
    </div>
  )
}

// ---- packages panel ----

/**
 * The packages view: the workspace's `aoxn.json` at the top, the installed
 * packages under it, and the whitelisted `aoxn pkg` verbs as buttons. Every
 * command's output goes to the output panel verbatim — the panel is a
 * remote control for the real package manager, not a reimplementation.
 */
function PackagesPanel({
  pkg,
  report,
  busy,
  onRun,
  onAdd,
  onWhy,
  onRemove,
  onRefresh,
}: {
  pkg: PkgManifest | null
  report: PkgReport | null
  busy: boolean
  onRun(sub: string, arg?: string): void
  onAdd(): void
  onWhy(): void
  onRemove(): void
  onRefresh(): void
}) {
  return (
    <div className="flex h-full min-h-0 flex-col bg-sidebar">
      <div className="flex h-9 flex-none items-center gap-1.5 border-b border-border px-2.5">
        <PackageIcon className="size-3.5 text-foreground-subtle" />
        <span className="min-w-0 flex-1 truncate text-ui-sm font-semibold">Packages</span>
        <Button aria-label="Re-read aoxn.json" onClick={onRefresh} size="icon-xs" type="button" variant="ghost">
          <RefreshCwIcon className="size-3.5" />
        </Button>
      </div>
      <div className="min-h-0 flex-1 overflow-auto p-2">
        {!pkg ? (
          <div className="px-2 py-6 text-center text-ui-sm text-foreground-subtle">
            Not read yet.
          </div>
        ) : !pkg.hasManifest ? (
          <div className="flex flex-col items-center gap-3 px-3 py-8 text-center">
            <div className="text-ui-sm text-foreground-subtle">
              No aoxn.json in this folder. Init one to start tracking dependencies.
            </div>
            <Button disabled={busy} onClick={() => onRun('init')} size="sm" type="button" variant="outline">
              Init
            </Button>
          </div>
        ) : (
          <div className="flex flex-col gap-3">
            <div className="flex items-center gap-1.5">
              <span className="min-w-0 truncate text-ui-sm font-semibold" title="package name">
                {pkg.name || '(unnamed)'}
              </span>
              {pkg.version ? (
                <span className="text-ui-xs text-foreground-subtlest">v{pkg.version}</span>
              ) : null}
              <span className="flex-1" />
              <Badge variant={pkg.hasLockfile ? 'secondary' : 'outline'}>
                {pkg.hasLockfile ? 'aoxn.lock' : 'no lockfile'}
              </Badge>
            </div>

            {pkg.parseError ? (
              <div className="rounded-lg border border-destructive/30 bg-destructive-surface px-2.5 py-2 text-ui-sm text-destructive-surface-foreground">
                {pkg.parseError}
              </div>
            ) : null}

            <div>
              <div className="mb-1 px-1 text-ui-xs font-medium uppercase tracking-wide text-foreground-subtlest">
                Dependencies
              </div>
              {pkg.dependencies.length === 0 ? (
                <div className="px-1 py-2 text-ui-sm text-foreground-subtle">None yet.</div>
              ) : (
                <div className="flex flex-col gap-0.5">
                  {pkg.dependencies.map((dep) => (
                    <div
                      className="flex items-center gap-1.5 rounded-md px-2 py-1"
                      key={dep.name}
                      title={formatDep(dep)}
                    >
                      <span className="min-w-0 flex-1 truncate text-ui-sm">{dep.name}</span>
                      <span className="text-ui-xs text-foreground-subtlest">{dep.req}</span>
                      {dep.dev ? <Badge variant="outline">dev</Badge> : null}
                    </div>
                  ))}
                </div>
              )}
            </div>

            {report?.error ? (
              <div className="rounded-lg border border-destructive/30 bg-destructive-surface px-2.5 py-2 text-ui-sm text-destructive-surface-foreground">
                {report.error}
              </div>
            ) : null}

            <div>
              <div className="mb-1 px-1 text-ui-xs font-medium uppercase tracking-wide text-foreground-subtlest">
                Installed ({report ? report.installed.length : 0})
              </div>
              {!report ? (
                <div className="px-1 py-2 text-ui-sm text-foreground-subtle">Reading the lockfile…</div>
              ) : report.installed.length === 0 ? (
                <div className="px-1 py-2 text-ui-sm text-foreground-subtle">Nothing installed.</div>
              ) : (
                <div className="flex flex-col gap-0.5">
                  {withUpgrades(report).map((item) => {
                    const arrow = formatUpgrade(item.version, item.upgrade ?? '')
                    return (
                      <div
                        className="flex items-center gap-1.5 rounded-md px-2 py-1"
                        key={item.name}
                        title={formatInstalled(item, item.upgrade)}
                      >
                        <span className="min-w-0 flex-1 truncate text-ui-sm">{item.name}</span>
                        <span className="text-ui-xs text-foreground-subtlest">{item.version}</span>
                        {item.scope === 'dev' ? <Badge variant="outline">dev</Badge> : null}
                        {arrow ? <Badge variant="secondary">{arrow}</Badge> : null}
                      </div>
                    )
                  })}
                </div>
              )}
            </div>

            {report && report.outdated.length > 0 ? (
              <div>
                <div className="mb-1 px-1 text-ui-xs font-medium uppercase tracking-wide text-foreground-subtlest">
                  Upgradable ({report.outdated.length})
                </div>
                <div className="flex flex-col gap-0.5">
                  {report.outdated.map((item) => (
                    <div
                      className="flex items-center gap-1.5 rounded-md px-2 py-1"
                      key={item.name}
                      title={item.note || undefined}
                    >
                      <span className="min-w-0 flex-1 truncate text-ui-sm">{item.name}</span>
                      <Badge variant="secondary">{formatUpgrade(item.locked, item.newest)}</Badge>
                    </div>
                  ))}
                </div>
              </div>
            ) : null}

            <div className="grid grid-cols-2 gap-1">
              <Button disabled={busy} onClick={onAdd} size="sm" type="button" variant="outline">
                Add…
              </Button>
              <Button disabled={busy} onClick={() => onRun('install')} size="sm" type="button" variant="outline">
                Install
              </Button>
              <Button disabled={busy} onClick={() => onRun('update')} size="sm" type="button" variant="outline">
                Update
              </Button>
              <Button disabled={busy} onClick={() => onRun('outdated')} size="sm" type="button" variant="outline">
                Outdated
              </Button>
              <Button disabled={busy} onClick={() => onRun('tree')} size="sm" type="button" variant="outline">
                Tree
              </Button>
              <Button disabled={busy} onClick={() => onRun('audit')} size="sm" type="button" variant="outline">
                Audit
              </Button>
              <Button disabled={busy} onClick={onWhy} size="sm" type="button" variant="outline">
                Why…
              </Button>
              <Button disabled={busy} onClick={onRemove} size="sm" type="button" variant="outline">
                Remove…
              </Button>
            </div>
            <div className="px-1 text-ui-xs leading-relaxed text-foreground-subtlest">
              Commands run the real <code className="font-mono">aoxn pkg</code> in this folder;
              publish / yank / cache are not offered here.
            </div>
          </div>
        )}
      </div>
    </div>
  )
}

/**
 * The compiler echoes back the path it was given, which is an absolute
 * native path, while the tree keys models by its own spelling. When the two
 * agree, use it; otherwise fall back to matching on the file name alone,
 * which is what makes an error inside an *import* still find its tab.
 */
function resolveAgainstRoot(file: string, root: string): string {
  if (root && file.startsWith(root)) return file
  const name = basename(file)
  return file.includes('/') || file.includes('\\') ? file : `${root}/${name}`
}