'use client'

/**
 * The editor surface: one Monaco instance, one model per open file.
 *
 * Why a model per tab rather than one model whose text is swapped:
 * Monaco models are cheap, they own the undo stack, and keeping them alive
 * is what makes switching tabs feel instant — and, more importantly, what
 * makes Ctrl+Z undo *that file's* edits instead of the last file's.
 *
 * Monaco is loaded through `next/dynamic` with `ssr: false` because it
 * touches `window` at import time and a packaged Tauri build has no server
 * to render into.
 */

import dynamic from 'next/dynamic'
import { useCallback, useEffect, useRef } from 'react'
import type * as Monaco from 'monaco-editor'
import type { Diagnostic } from '@/lib/diagnostics'
import { LANG_AOXN, registerAoxn } from '@/lib/monaco'

const MonacoEditor = dynamic(
  () => import('@monaco-editor/react').then((m) => m.default),
  {
    ssr: false,
    loading: () => <div className="wb-placeholder">Loading the editor…</div>,
  },
)

export interface EditorProps {
  path: string
  text: string
  /** Diagnostics for THIS file, already filtered by the workbench. */
  diagnostics: Diagnostic[]
  /** A pending "reveal this line" instruction from the output panel. */
  jump?: { path: string; line: number; column: number; seq: number } | null
  /** Zai Monaco theme: `aoxn-zai-dark` or `aoxn-zai-light`. */
  theme?: string
  /** Called on every keystroke; the parent owns the dirty flag. */
  onChange(path: string, text: string): void
  onCursor(path: string, line: number, column: number): void
  onSave(path: string): void
  onGoToDefinition?(path: string, line: number, column: number): void
}

export function Editor(props: EditorProps) {
  const { path, text, diagnostics, jump } = props

  const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null)
  const monacoRef = useRef<typeof Monaco | null>(null)
  const modelRef = useRef<Monaco.editor.ITextModel | null>(null)

  // The mount handler registers command handlers exactly once, so it must
  // read the current props through a ref rather than closing over them —
  // otherwise every render would tear down and re-add the editor's
  // command registry.
  const propsRef = useRef(props)
  propsRef.current = props

  const handleMount = useCallback(
    (editor: Monaco.editor.IStandaloneCodeEditor, monaco: typeof Monaco) => {
      editorRef.current = editor
      monacoRef.current = monaco

      editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
        // The REAL path, not `model.uri` — the wrapper turns the `path` prop
        // into a parsed URI, and a Windows path round-tripped through
        // `Uri.toString()` does not compare equal to the key the workbench's
        // document map uses. Passing the prop straight through is what makes
        // Ctrl+S inside the editor save the file instead of silently no-op.
        propsRef.current.onSave(propsRef.current.path)
      })

      editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyF, () => {
        editor.getAction('actions.find')?.run()
      })

      editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyG, () => {
        editor.getAction('editor.action.startFindReplaceAction')?.run()
      })

      // Ctrl-click is where every editor puts "go to definition". There is
      // no language server yet, so this fires the workbench's handler and
      // currently does nothing — but the keybinding is where a user will
      // press it, and adding the server later should not move it.
      editor.onMouseDown((e) => {
        if (!(e.event.ctrlKey || e.event.metaKey)) return
        const pos = e.target.position
        const model = editor.getModel()
        if (pos && model) {
          propsRef.current.onGoToDefinition?.(model.uri.toString(), pos.lineNumber, pos.column)
        }
      })

      editor.onDidChangeCursorPosition((e) => {
        // Same rule as the save handler above: the prop is the identity the
        // workbench knows, the model URI is not.
        propsRef.current.onCursor(propsRef.current.path, e.position.lineNumber, e.position.column)
      })

      modelRef.current = editor.getModel()
    },
    [],
  )

  // Diagnostics become editor markers. `null` clears them, which is what
  // makes a stale red squiggle disappear the moment the file is fixed.
  useEffect(() => {
    const monaco = monacoRef.current
    const editor = editorRef.current
    if (!monaco || !editor) return
    const model = editor.getModel()
    if (!model) return

    monaco.editor.setModelMarkers(
      model,
      'aoxn',
      diagnostics
        .filter((d) => d.line > 0)
        .map((d) => ({
          startLineNumber: d.line,
          endLineNumber: d.line,
          startColumn: Math.max(1, d.column),
          endColumn: Math.max(1, d.column) + 1,
          message: d.message,
          severity:
            d.severity === 'error'
              ? monaco.MarkerSeverity.Error
              : monaco.MarkerSeverity.Warning,
        })),
    )
  }, [diagnostics, path])

  const handleChange = useCallback((next: string | undefined) => {
    propsRef.current.onChange(propsRef.current.path, next ?? '')
  }, [])

  // Reveal a line: this is how clicking a diagnostic in the output panel
  // lands the caret on the error. The `seq` in the instruction matters —
  // jumping to the same line twice must fire twice, and without a changing
  // key the effect would not run the second time.
  useEffect(() => {
    const editor = editorRef.current
    if (!editor || !jump || jump.path !== path || jump.line <= 0) return
    const pos = { lineNumber: jump.line, column: jump.column }
    editor.setPosition(pos)
    editor.revealLineInCenter(jump.line, 1 /* Immediate */)
    editor.focus()
  }, [jump, path])

  return (
    <MonacoEditor
      path={path}
      language={languageFor(path)}
      value={text}
      theme={props.theme ?? 'aoxn-zai-dark'}
      onChange={handleChange}
      // The Aoxn language and the aoxn-dark theme are registered HERE, in
      // beforeMount, which runs after Monaco is loaded but BEFORE the
      // wrapper creates the editor and its first model. Registering in a
      // page-level effect instead raced the editor's mount (v0.31.0): on a
      // cold load the editor could come up in Monaco's light default theme.
      // Both calls are idempotent, so the page-level one is gone.
      beforeMount={(monaco) => registerAoxn(monaco)}
      onMount={handleMount}
      // Keep the per-file model (and its undo stack) across tab switches;
      // `value` only updates the editor's contents, never recreates it.
      keepCurrentModel
      saveViewState={false}
      options={{
        fontSize: 13,
        fontFamily: 'var(--font-mono)',
        lineHeight: 20,
        fontLigatures: true,
        minimap: { enabled: false },
        scrollBeyondLastLine: false,
        renderWhitespace: 'selection',
        smoothScrolling: true,
        cursorBlinking: 'smooth',
        padding: { top: 10, bottom: 24 },
        automaticLayout: true,
        tabSize: 4,
        insertSpaces: true,
        bracketPairColorization: { enabled: true },
        guides: { indentation: true, bracketPairs: false },
        scrollbar: {
          verticalScrollbarSize: 12,
          horizontalScrollbarSize: 12,
          useShadows: false,
        },
        fixedOverflowWidgets: true,
        wordWrap: 'off',
        occurrencesHighlight: 'singleFile',
        // No language server for Aoxn, so every feature that would call one
        // is off: leaving it on produces a spinner and no results.
        quickSuggestions: false,
        suggestOnTriggerCharacters: false,
        parameterHints: { enabled: false },
        hover: { enabled: 'off' },
        folding: true,
        showFoldingControls: 'mouseover',
        readOnly: false,
        domReadOnly: false,
      }}
    />
  )
}

/** Aoxn and TypeScript get grammars; everything else is plain text. */
export function languageFor(path: string): string {
  const p = path.toLowerCase()
  if (p.endsWith('.ax')) return LANG_AOXN
  if (p.endsWith('.ts') || p.endsWith('.tsx')) return 'typescript'
  if (p.endsWith('.json')) return 'json'
  if (p.endsWith('.md')) return 'markdown'
  if (p.endsWith('.toml')) return 'ini'
  if (p.endsWith('.rs')) return 'rust'
  if (p.endsWith('.css')) return 'css'
  if (p.endsWith('.html')) return 'html'
  if (p.endsWith('.yml') || p.endsWith('.yaml')) return 'yaml'
  if (p.endsWith('.sh')) return 'shell'
  return 'plaintext'
}
