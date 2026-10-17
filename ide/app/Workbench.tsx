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
import { Editor, ErrorBadge, type EditorProps } from '@/components/Editor'
import {
  IconCheck,
  IconChevron,
  IconClose,
  IconFile,
  IconFiles,
  IconFolder,
  IconFolderOpen,
  IconGear,
  IconNewFile,
  IconOutline,
  IconPackage,
  IconRefresh,
  IconRun,
  IconSave,
  IconSearch,
  IconSource,
  IconStethoscope,
  IconSymbol,
  IconWarning,
} from '@/components/icons'
import * as api from '@/lib/bridge'
import type {
  ExecResult,
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
  }, [refreshToolchain, refreshPkg, say, appendLog])

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
        return true
      } catch (e) {
        appendLog('err', String(e))
        say(`Cannot save ${basename(path)}`)
        return false
      }
    },
    [docs, appendLog, say, autoCheck],
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
      setShowPanel(true)
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
    [docs, saveFile, appendLog, refreshTree],
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
  }, [appendLog, refreshPkg])

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
    setShowPanel(true)
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
  }, [appendLog, refreshToolchain])

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
      setShowPanel(true)
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
    [appendLog, refreshPkg, refreshTree],
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
      if (kind === 'file') {
        const created = nodes.find((n) => !n.isDir && norm(n.path) === norm(target))
        await openFile(created ? created.path : target)
      }
      say(`Created ${name.trim()}`)
    },
    [openFile, say],
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
   * The rows the explorer should actually draw.
   *
   * The tree arrives FLAT with a depth column, so "is this row inside a
   * collapsed folder" is a single left-to-right pass: track the depth at
   * which we entered a collapsed folder and skip everything deeper until a
   * row at or above that depth brings us back out.
   */
  const visibleTree = useMemo(() => {
    const out: TreeNode[] = []
    let hiddenBelow = -1
    for (const n of tree) {
      if (hiddenBelow >= 0 && n.depth > hiddenBelow) continue
      hiddenBelow = -1
      out.push(n)
      if (n.isDir && !expanded.has(n.path)) hiddenBelow = n.depth
    }
    return out
  }, [tree, expanded])

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
        default:
          break
      }
    },
    [active, runTool, saveFile, startPrompt, runPkg, runDoctor, refreshTree, openFolder],
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
  }, [active, palette, prompt, saveFile, runTool, openFolder])

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
    <div className="app">
      <nav className="activity">
        <div className="activity__brand" title="Aoxn IDE">
          ax
        </div>
        <button
          className={`activity__item ${showSidebar && view === 'files' ? 'activity__item--on' : ''}`}
          onClick={() => {
            setView('files')
            setShowSidebar(true)
          }}
          title="Explorer (Ctrl+B)"
        >
          <IconFiles />
        </button>
        <button
          className={`activity__item ${showSidebar && view === 'packages' ? 'activity__item--on' : ''}`}
          onClick={() => {
            setView('packages')
            setShowSidebar(true)
          }}
          title="Packages — aoxn.json and aoxn pkg"
        >
          <IconPackage />
        </button>
        <button
          className={`activity__item ${showSidebar && view === 'outline' ? 'activity__item--on' : ''}`}
          onClick={() => {
            setView('outline')
            setShowSidebar(true)
          }}
          title="Outline — the declarations the compiler found"
        >
          <IconOutline />
        </button>
        <button
          className="activity__item"
          onClick={() => setPalette({ mode: 'file', query: '' })}
          title="Go to file (Ctrl+P)"
        >
          <IconSearch />
        </button>
        <button
          className="activity__item"
          onClick={() => setPalette({ mode: 'symbol', query: '' })}
          title="Go to symbol (Ctrl+Shift+O)"
        >
          <IconSymbol />
        </button>
        <button
          className="activity__item"
          onClick={() => setPalette({ mode: 'command', query: '' })}
          title="Commands (Ctrl+Shift+P)"
        >
          <IconSource />
        </button>
        <button
          className="activity__item"
          onClick={() => void runTool('run', active)}
          title="Run (F5)"
        >
          <IconRun />
        </button>
        <div style={{ flex: 1 }} />
        <button className="activity__item" onClick={() => void openFolder()} title="Open folder (Ctrl+O)">
          <IconFolderOpen />
        </button>
        <button
          className="activity__item"
          onClick={() => setPalette({ mode: 'command', query: '' })}
          title="Commands"
        >
          <IconGear />
        </button>
      </nav>

      <div className="app__body">
        <aside className="sidebar" hidden={!showSidebar}>
          {view === 'files' ? (
            <>
              <div className="sidebar__title">
                <span title={root}>{root ? basename(root) || root : 'Explorer'}</span>
                <span className="sidebar__actions">
                  <button className="iconbtn" title="New file" onClick={() => startPrompt('file')}>
                    <IconNewFile />
                  </button>
                  <button className="iconbtn" title="New folder" onClick={() => startPrompt('folder')}>
                    <IconFolder />
                  </button>
                  <button className="iconbtn" title="Refresh" onClick={() => void refreshTree()}>
                    <IconRefresh />
                  </button>
                </span>
              </div>
              <div className="tree">
                <div className="tree__label">Explorer</div>
                {tree.length === 0 ? (
                  <div className="tree__empty">
                    No folder open.{'\n'}Click the folder icon in the activity bar to choose one.
                  </div>
                ) : (
                  visibleTree.map((node) => (
                    <TreeRow
                      key={node.path}
                      node={node}
                      open={expanded.has(node.path)}
                      selected={selected === node.path}
                      onToggle={() =>
                        setExpanded((prev) => {
                          const next = new Set(prev)
                          if (next.has(node.path)) next.delete(node.path)
                          else next.add(node.path)
                          return next
                        })
                      }
                      onOpen={() => {
                        if (node.isDir) return
                        void openFile(node.path)
                      }}
                    />
                  ))
                )}
              </div>
            </>
          ) : view === 'outline' ? (
            <OutlinePanel
              rows={outline}
              error={symbolError}
              busy={busy}
              path={active}
              onJump={(line) => {
                if (active) void revealSymbol(active, line, 1)
              }}
              onRefresh={() => void refreshSymbols(active)}
              onSearch={() => setPalette({ mode: 'symbol', query: '' })}
            />
          ) : (
            <PackagesPanel
              pkg={pkg}
              report={pkgInfo}
              busy={busy}
              onRun={(sub, arg) => void runPkg(sub, arg)}
              onAdd={() => startPrompt('pkgadd')}
              onWhy={() => startPrompt('pkgwhy')}
              onRemove={() => startPrompt('pkgremove')}
              onRefresh={() => void refreshPkg()}
            />
          )}
        </aside>

        <div className="app__main">
          <div className="tabs">
            {tabs.map((path) => {
              const d = docs.get(path)
              if (!d) return null
              const dirty = d.text !== d.saved
              return (
                <div
                  key={path}
                  className={`tab ${path === active ? 'tab--on' : ''}`}
                  onClick={() => setActive(path)}
                  title={path}
                >
                  <span className="tab__name">{basename(path)}</span>
                  {dirty ? (
                    <span
                      className="tab__dirty"
                      title="Unsaved — click to save"
                      onClick={(e) => {
                        e.stopPropagation()
                        void saveFile(path)
                      }}
                    />
                  ) : null}
                  <button
                    className="tab__close"
                    title="Close"
                    onClick={(e) => {
                      e.stopPropagation()
                      closeTab(path)
                    }}
                  >
                    <IconClose />
                  </button>
                </div>
              )
            })}
          </div>

          <div className="editor">
            {activeDoc ? (
              <Editor
                path={activeDoc.path}
                text={activeDoc.text}
                diagnostics={activeDiags}
                jump={jump}
                onChange={onEditorChange}
                onCursor={onEditorCursor}
                onSave={onEditorSave}
                onGoToDefinition={onGoToDefinition}
              />
            ) : (
              <div className="editor__placeholder">
                <div>
                  <div style={{ fontSize: 16, marginBottom: 12, color: 'var(--fg-dim)', letterSpacing: 1 }}>
                    Aoxn IDE
                  </div>
                  <div>
                    <kbd>Ctrl</kbd>+<kbd>P</kbd> go to file · <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>P</kbd>{' '}
                    commands · <kbd>Ctrl</kbd>+<kbd>O</kbd> open folder
                  </div>
                  <div style={{ marginTop: 4 }}>
                    <kbd>F5</kbd> run · <kbd>F6</kbd> build · <kbd>F7</kbd> type check ·{' '}
                    <kbd>Ctrl</kbd>+<kbd>S</kbd> save
                  </div>
                </div>
              </div>
            )}
          </div>

          <OutputPanel
            hidden={!showPanel}
            log={log}
            onClear={() => setLog([])}
            onClose={() => setShowPanel(false)}
            onJump={jumpToLogLine}
          />
        </div>
      </div>

      <StatusBar
        toolchain={toolchain}
        busy={busy}
        errorCount={diagnostics.length}
        active={activeDoc}
        showPanel={showPanel}
        onTogglePanel={() => setShowPanel((v) => !v)}
        onCheck={() => void runTool('check', active)}
        onBuild={() => void runTool('build', active)}
        onRun={() => void runTool('run', active)}
        onSave={() => active && void saveFile(active)}
        onDoctor={() => void runDoctor()}
        onOpenFolder={() => void openFolder()}
      />

      {palette ? (
        <CommandPalette
          mode={palette.mode}
          query={palette.query}
          tree={tree}
          symbols={symbols}
          busy={busy}
          onQuery={(q) => setPalette({ ...palette, query: q })}
          onClose={() => setPalette(null)}
          onPickFile={(p) => {
            setPalette(null)
            void openFile(p)
          }}
          onPickSymbol={(p, line, col) => {
            setPalette(null)
            void revealSymbol(p, line, col)
          }}
          onCommand={(id) => {
            setPalette(null)
            runCommand(id)
          }}
        />
      ) : null}

      {prompt ? (
        <PromptDialog
          mode={prompt.mode}
          dir={prompt.dir}
          onCancel={() => setPrompt(null)}
          onConfirm={(name) => confirmPrompt(prompt.mode, prompt.dir, name)}
        />
      ) : null}

      {toast ? (
        <div
          role="status"
          style={{
            position: 'fixed',
            bottom: 34,
            left: '50%',
            transform: 'translateX(-50%)',
            background: 'var(--bg-active)',
            border: '1px solid var(--border-strong)',
            borderRadius: 5,
            padding: '6px 14px',
            zIndex: 60,
          }}
        >
          {toast}
        </div>
      ) : null}
    </div>
  )
}

// ---- explorer row ----

function TreeRow({
  node,
  open,
  selected,
  onToggle,
  onOpen,
}: {
  node: TreeNode
  open: boolean
  selected: boolean
  onToggle(): void
  onOpen(): void
}) {
  return (
    <div
      className={`tree__row ${selected ? 'tree__row--on' : ''}`}
      style={{ paddingLeft: 8 + node.depth * 12 }}
      onClick={() => (node.isDir ? onToggle() : onOpen())}
      title={node.path}
      role="treeitem"
      aria-expanded={node.isDir ? open : undefined}
      aria-selected={selected}
    >
      <span className={`tree__twisty ${open ? 'tree__twisty--open' : ''}`}>
        {node.isDir ? <IconChevron /> : null}
      </span>
      {node.isDir ? <IconFolder /> : <IconFile />}
      <span className="tree__name">{node.name}</span>
    </div>
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
    <div className="tree">
      <div className="sidebar__title">
        <span title={path ?? ''}>{path ? basename(path) : 'Outline'}</span>
        <span className="sidebar__actions">
          <button
            className="iconbtn"
            title="Search declarations (Ctrl+Shift+O)"
            onClick={onSearch}
          >
            <IconSymbol />
          </button>
          <button className="iconbtn" title="Re-read the declarations" onClick={onRefresh}>
            <IconRefresh />
          </button>
        </span>
      </div>
      <div className="tree__label">Outline</div>
      {!path ? (
        <div className="tree__empty">Open a file to see what it declares.</div>
      ) : error ? (
        <>
          <div className="pkg__error">{error}</div>
          <div className="tree__empty">
            The compiler could not read this file, so there is no outline. The message
            above is its own.
          </div>
        </>
      ) : rows.length === 0 ? (
        <div className="tree__empty">
          {busy ? 'Asking the compiler…' : 'No top-level declarations in this file.'}
        </div>
      ) : (
        rows.map(({ symbol }) => (
          <div
            key={`${symbol.file}:${symbol.line}:${symbol.name}`}
            className="tree__row outline__row"
            onClick={() => onJump(symbol.line)}
            title={symbol.signature}
            // `listitem`, not `treeitem`: the outline is a flat list of the
            // current file's declarations, not a tree, and sharing the
            // explorer's role makes the two indistinguishable to assistive
            // technology and to anything selecting by role.
            role="listitem"
          >
            <span className="outline__kind">{symbol.kind === 'struct' ? 'S' : 'ƒ'}</span>
            <span className="tree__name">{symbol.name}</span>
            <span className="outline__sig">
              {symbol.signature.replace(/^(extern )?def /, '').replace(/^struct /, '')}
            </span>
            <span className="outline__line">{symbol.line}</span>
          </div>
        ))
      )}
    </div>
  )
}

// ---- output panel ----

function OutputPanel({
  hidden,
  log,
  onClear,
  onClose,
  onJump,
}: {
  hidden: boolean
  log: LogEntry[]
  onClear(): void
  onClose(): void
  onJump(raw: string): void
}) {
  return (
    <section className="panel" hidden={hidden}>
      <div className="panel__head">
        <span className="panel__tab panel__tab--on">Output</span>
        <span className="panel__spacer" />
        <button className="iconbtn" title="Clear" onClick={onClear}>
          <IconClose />
        </button>
        <button className="iconbtn" title="Hide panel (Ctrl+J)" onClick={onClose}>
          <IconChevron />
        </button>
      </div>
      <div className="panel__body">
        {log.length === 0 ? (
          <div className="logempty">
            Nothing yet — F5 runs the current file, F7 type-checks it without invoking clang.
          </div>
        ) : (
          log.map((e) => {
            const d = parseDiagnostic(e.text)
            const clickable = d !== null && d.file.length > 0
            return (
              <span
                key={e.id}
                className={
                  e.kind === 'cmd'
                    ? 'logline logline--cmd'
                    : e.kind === 'err'
                      ? 'logline logline--err'
                      : e.kind === 'meta'
                        ? 'logline logline--meta'
                        : 'logline'
                }
                style={clickable ? { cursor: 'pointer', textDecoration: 'underline dotted' } : undefined}
                title={clickable ? 'Go to this line' : undefined}
                onClick={clickable ? () => onJump(e.text) : undefined}
              >
                {e.text}
              </span>
            )
          })
        )}
      </div>
    </section>
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
    <footer className="status">
      <span className="status__group">
        {busy ? (
          <span className="status__item">
            <IconRefresh className="spin" /> working
          </span>
        ) : (
          <span className="status__item">ready</span>
        )}
        {dirty ? (
          <button className="status__item" onClick={props.onSave} title="Save (Ctrl+S)">
            <IconSave /> unsaved
          </button>
        ) : null}
        <ErrorBadge count={errorCount} />
      </span>

      <span className="status__group status__group--push">
        {active ? (
          <span className="status__item">
            Ln {active.cursor.line}, Col {active.cursor.column}
          </span>
        ) : null}
        {active ? (
          <span className="status__item">
            {lines} {lines === 1 ? 'line' : 'lines'}
          </span>
        ) : null}
        <button className="status__item" onClick={props.onCheck} title="Type check (F7)">
          Check
        </button>
        <button className="status__item" onClick={props.onBuild} title="Build (F6)">
          Build
        </button>
        <button className="status__item" onClick={props.onRun} title="Run (F5)">
          Run
        </button>
        <button className="status__item" onClick={props.onTogglePanel} title="Toggle panel (Ctrl+J)">
          Output {showPanel ? '▾' : '▸'}
        </button>
        {toolchain && !toolchain.compilerFound ? (
          <button
            className="status__item status__item--warn"
            onClick={props.onDoctor}
            title="Run aoxn doctor — the toolchain's own self-check"
          >
            <IconWarning /> compiler not found
          </button>
        ) : toolchain && !toolchain.clangFound ? (
          <button
            className="status__item status__item--warn"
            onClick={props.onDoctor}
            title="Run aoxn doctor — builds fail without clang"
          >
            <IconWarning /> clang not found
          </button>
        ) : toolchain ? (
          <span className="status__item" title={toolchain.version || toolchain.compiler}>
            <IconCheck /> {toolchain.compiler}
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
  { id: 'togglepanel', title: 'Toggle the output panel' },
  { id: 'clearlog', title: 'Clear the output panel' },
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
    <div className="scrim" onMouseDown={onClose}>
      <div className="palette" onMouseDown={(e) => e.stopPropagation()}>
        <input
          className="palette__input"
          // eslint-disable-next-line jsx-a11y/no-autofocus -- a quick-open has to take focus
          autoFocus
          value={query}
          spellCheck={false}
          placeholder={
            mode === 'file'
              ? 'Search files by name…'
              : mode === 'symbol'
                ? 'Search declarations by name…'
                : 'Type a command…'
          }
          onChange={(e) => onQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'ArrowDown') {
              e.preventDefault()
              setIndex((i) => Math.min(rows.length - 1, i + 1))
            } else if (e.key === 'ArrowUp') {
              e.preventDefault()
              setIndex((i) => Math.max(0, i - 1))
            } else if (e.key === 'Enter') {
              e.preventDefault()
              rows[index]?.pick()
            } else if (e.key === 'Escape') {
              onClose()
            }
          }}
        />
        <div className="palette__list">
          {rows.length === 0 ? <div className="palette__hint">No matches.</div> : null}
          {rows.map((r, i) => (
            <div
              key={r.key}
              className={`palette__row ${i === index ? 'palette__row--on' : ''}`}
              onMouseEnter={() => setIndex(i)}
              onClick={r.pick}
            >
              {mode === 'file' ? <IconFile /> : mode === 'symbol' ? <IconSymbol /> : <IconSource />}
              <span className="palette__label">{r.label}</span>
              {r.hint ? <span className="palette__origin">{r.hint}</span> : null}
            </div>
          ))}
        </div>
        <div className="palette__hint">
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
    <div className="scrim" onMouseDown={onCancel}>
      <div className="palette" onMouseDown={(e) => e.stopPropagation()}>
        <div className="palette__title">{spec.title(dir)}</div>
        <input
          className="palette__input"
          // eslint-disable-next-line jsx-a11y/no-autofocus -- a prompt has to take focus
          autoFocus
          value={value}
          spellCheck={false}
          placeholder={spec.placeholder}
          onChange={(e) => {
            setValue(e.target.value)
            setError('')
          }}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault()
              void submit()
            } else if (e.key === 'Escape') {
              onCancel()
            }
          }}
        />
        {error ? <div className="palette__hint palette__hint--err">{error}</div> : null}
        <div className="palette__hint">{spec.hint}</div>
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
  if (!pkg) {
    return (
      <div className="tree">
        <div className="tree__label">Packages</div>
        <div className="tree__empty">
          Not read yet.{'\n'}Use the ↻ button to read aoxn.json.
        </div>
      </div>
    )
  }
  if (!pkg.hasManifest) {
    return (
      <div className="tree">
        <div className="tree__label">Packages</div>
        <div className="tree__empty">
          No aoxn.json in this folder.{'\n'}Init one to start tracking dependencies.
        </div>
        <div className="pkg__actions">
          <button className="pkgbtn" disabled={busy} onClick={() => onRun('init')}>
            Init
          </button>
          <button className="pkgbtn" title="Re-read aoxn.json" onClick={onRefresh}>
            <IconRefresh />
          </button>
        </div>
      </div>
    )
  }
  return (
    <div className="tree">
      <div className="tree__label">Packages</div>
      <div className="pkg__head">
        <span className="pkg__name" title="package name">
          {pkg.name || '(unnamed)'}
        </span>
        {pkg.version ? <span className="pkg__req">v{pkg.version}</span> : null}
        <span className={`pkg__badge ${pkg.hasLockfile ? '' : 'pkg__badge--dim'}`}>
          {pkg.hasLockfile ? 'aoxn.lock ✓' : 'no lockfile'}
        </span>
      </div>

      {pkg.parseError ? (
        <div className="pkg__error">{pkg.parseError}</div>
      ) : null}

      <div className="pkg__section">Dependencies</div>
      {pkg.dependencies.length === 0 ? (
        <div className="tree__empty">None yet — Add one below.</div>
      ) : (
        pkg.dependencies.map((d) => (
          <div key={`${d.name}`} className="pkg__row" title={formatDep(d)}>
            <span className="pkg__name">{d.name}</span>
            <span className="pkg__req">{d.req}</span>
            {d.dev ? <span className="pkg__badge">dev</span> : null}
          </div>
        ))
      )}

      {/*
        The RESOLVED inventory, from `aoxn list --json` — not the directory
        names above it. This is the section that answers "what is actually
        installed", and an upgrade marker sits next to a package whose
        registry has something newer. When the report could not be read, the
        panel says so instead of showing an empty list that reads like
        "nothing is installed".
      */}
      {report?.error ? <div className="pkg__error">{report.error}</div> : null}
      <div className="pkg__section">
        Installed ({report ? report.installed.length : 0})
      </div>
      {!report ? (
        <div className="tree__empty">Reading the lockfile…</div>
      ) : report.installed.length === 0 ? (
        <div className="tree__empty">Nothing installed — run Install.</div>
      ) : (
        withUpgrades(report).map((p) => {
          const arrow = formatUpgrade(p.version, p.upgrade ?? '')
          return (
            <div key={p.name} className="pkg__row" title={formatInstalled(p, p.upgrade)}>
              <span className="pkg__name">{p.name}</span>
              <span className="pkg__req">{p.version}</span>
              {p.scope === 'dev' ? <span className="pkg__badge">dev</span> : null}
              {arrow ? <span className="pkg__upgrade">{arrow}</span> : null}
            </div>
          )
        })
      )}

      {report && report.outdated.length > 0 ? (
        <>
          <div className="pkg__section">Upgradable ({report.outdated.length})</div>
          {report.outdated.map((o) => (
            <div key={o.name} className="pkg__row" title={o.note || undefined}>
              <span className="pkg__name">{o.name}</span>
              <span className="pkg__upgrade">
                {formatUpgrade(o.locked, o.newest)}
              </span>
            </div>
          ))}
        </>
      ) : null}

      <div className="pkg__actions">
        <button className="pkgbtn" disabled={busy} onClick={onAdd}>
          Add…
        </button>
        <button className="pkgbtn" disabled={busy} onClick={() => onRun('install')} title="aoxn pkg install">
          Install
        </button>
        <button className="pkgbtn" disabled={busy} onClick={() => onRun('update')} title="aoxn pkg update">
          Update
        </button>
        <button className="pkgbtn" disabled={busy} onClick={() => onRun('outdated')} title="aoxn pkg outdated">
          Outdated
        </button>
      </div>
      <div className="pkg__actions">
        <button className="pkgbtn" disabled={busy} onClick={() => onRun('tree')} title="aoxn pkg tree">
          Tree
        </button>
        <button className="pkgbtn" disabled={busy} onClick={() => onRun('audit')} title="aoxn pkg audit">
          Audit
        </button>
        <button className="pkgbtn" disabled={busy} onClick={onWhy} title="aoxn pkg why <name>">
          Why…
        </button>
        <button className="pkgbtn" disabled={busy} onClick={onRemove} title="aoxn pkg remove <name>">
          Remove…
        </button>
        <button className="pkgbtn" title="Re-read aoxn.json" onClick={onRefresh}>
          <IconRefresh />
        </button>
      </div>
      <div className="pkg__note">
        Commands run the real <code>aoxn pkg</code> in this folder; publish /
        yank / cache are deliberately not offered here.
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