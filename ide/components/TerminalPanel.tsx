'use client'

/**
 * Command panel.
 *
 * The output renderer is ZCode's `ai-elements/terminal` ANSI surface,
 * reduced to the part the IDE needs: one prompt, a scrollback of command
 * results, and the Zai terminal palette. The backend runs one command per
 * submit (no persistent PTY), and `cd` is tracked here so the next command
 * starts where the user expects.
 */

import { useCallback, useEffect, useRef, useState, type KeyboardEvent } from 'react'
import * as AnsiModule from 'ansi-to-react'
import { ChevronRightIcon, EraserIcon, TerminalSquareIcon } from 'lucide-react'
import * as api from '@/lib/bridge'
import { cn } from '@/components/lib/utils'
import { Button } from '@/components/ui/button'

type AnsiComponent = React.ComponentType<{ children?: string }>

// `ansi-to-react` ships as CJS. Node's ESM loader exposes the module object
// as `default`, Next's bundler exposes the component itself; unwrap both
// shapes or React sees an object where it expects a component.
const AnsiOutput = ((AnsiModule as unknown as { default?: { default?: unknown } }).default
  ?.default ??
  (AnsiModule as unknown as { default?: unknown }).default ??
  AnsiModule) as AnsiComponent

interface TranscriptEntry {
  id: number
  command: string
  cwd: string
  output: string
  code: number
  durationMs: number
}

export interface TerminalPanelProps {
  root: string
  className?: string
}

function joinPath(base: string, next: string): string {
  if (/^[a-zA-Z]:[\\/]/.test(next) || next.startsWith('/') || next.startsWith('\\\\')) {
    return next
  }
  const sep = base.includes('\\') ? '\\' : '/'
  const parts = `${base}${sep}${next}`.split(/[\\/]+/)
  const out: string[] = []
  for (const part of parts) {
    if (!part || part === '.') continue
    if (part === '..') {
      if (out.length > 1) out.pop()
      continue
    }
    out.push(part)
  }
  const prefix = /^[a-zA-Z]:$/.test(out[0] ?? '') ? '' : base.startsWith('/') ? '/' : ''
  return `${prefix}${out.join(sep)}`
}

export function TerminalPanel({ root, className }: TerminalPanelProps) {
  const [entries, setEntries] = useState<TranscriptEntry[]>([])
  const [draft, setDraft] = useState('')
  const [cwd, setCwd] = useState(root)
  const [busy, setBusy] = useState(false)
  const [history, setHistory] = useState<string[]>([])
  const [historyIndex, setHistoryIndex] = useState<number | null>(null)
  const scrollRef = useRef<HTMLDivElement>(null)
  const seq = useRef(0)

  useEffect(() => setCwd(root), [root])

  useEffect(() => {
    const node = scrollRef.current
    if (node) node.scrollTop = node.scrollHeight
  }, [entries, busy])

  const run = useCallback(async () => {
    const command = draft.trim()
    if (!command || busy) return
    setDraft('')
    setHistory((current) => (current[current.length - 1] === command ? current : [...current, command]))
    setHistoryIndex(null)

    if (command === 'clear' || command === 'cls') {
      setEntries([])
      return
    }
    if (command === 'help') {
      setEntries((current) => [
        ...current,
        {
          id: ++seq.current,
          command,
          cwd,
          code: 0,
          durationMs: 0,
          output:
            'Commands run in the workspace. `cd <dir>` changes the working directory for\n' +
            'the next command; `clear` empties this transcript. Everything else is passed\n' +
            'to the platform shell (cmd /C on Windows, sh -lc elsewhere).',
        },
      ])
      return
    }
    if (command === 'cd' || command.startsWith('cd ')) {
      const target = command.slice(2).trim()
      const next = target ? joinPath(cwd, target.replace(/^["']|["']$/g, '')) : root
      setEntries((current) => [
        ...current,
        { id: ++seq.current, command, cwd, code: 0, durationMs: 0, output: next },
      ])
      setCwd(next)
      return
    }

    setBusy(true)
    const startedCwd = cwd
    try {
      const result = await api.terminalExec(command, cwd)
      setEntries((current) => [
        ...current,
        {
          id: ++seq.current,
          command,
          cwd: startedCwd,
          output: result.output,
          code: result.code,
          durationMs: result.durationMs,
        },
      ])
    } catch (error) {
      setEntries((current) => [
        ...current,
        {
          id: ++seq.current,
          command,
          cwd: startedCwd,
          output: error instanceof Error ? error.message : String(error),
          code: -1,
          durationMs: 0,
        },
      ])
    } finally {
      setBusy(false)
    }
  }, [busy, cwd, draft, root])

  const handleKeyDown = useCallback(
    (event: KeyboardEvent<HTMLInputElement>) => {
      if (event.key === 'ArrowUp' && history.length > 0) {
        event.preventDefault()
        const next = historyIndex === null ? history.length - 1 : Math.max(0, historyIndex - 1)
        setHistoryIndex(next)
        setDraft(history[next] ?? '')
      } else if (event.key === 'ArrowDown') {
        if (historyIndex === null) return
        event.preventDefault()
        const next = historyIndex + 1
        if (next >= history.length) {
          setHistoryIndex(null)
          setDraft('')
        } else {
          setHistoryIndex(next)
          setDraft(history[next] ?? '')
        }
      } else if (event.key === 'Enter') {
        event.preventDefault()
        void run()
      }
    },
    [history, historyIndex, run],
  )

  return (
    <div className={cn('flex h-full min-h-0 flex-col bg-terminal-bg text-terminal-fg', className)}>
      <div className="flex h-9 flex-none items-center gap-2 border-b border-border px-2.5">
        <TerminalSquareIcon className="size-3.5 text-foreground-subtle" />
        <span className="text-ui-sm font-medium">Terminal</span>
        <span className="min-w-0 flex-1 truncate text-ui-xs text-foreground-subtlest" title={cwd}>
          {cwd}
        </span>
        <Button
          aria-label="Clear terminal"
          disabled={entries.length === 0}
          onClick={() => setEntries([])}
          size="icon-xs"
          type="button"
          variant="ghost"
        >
          <EraserIcon className="size-3" />
        </Button>
      </div>

      <div className="terminal-ansi min-h-0 flex-1 overflow-auto px-3 py-2" ref={scrollRef}>
        {entries.length === 0 ? (
          <div className="text-ui-sm text-foreground-subtlest">
            Type a command below. `help` lists the few built-ins.
          </div>
        ) : (
          entries.map((entry) => (
            <div className="mb-2" key={entry.id}>
              <div className="mb-0.5 flex items-start gap-1.5">
                <ChevronRightIcon className="mt-0.5 size-3 shrink-0 text-brand" />
                <span className="break-all text-foreground">{entry.command}</span>
                {entry.durationMs > 0 ? (
                  <span className="ml-auto shrink-0 text-ui-xs text-foreground-subtlest">
                    {entry.durationMs} ms
                  </span>
                ) : null}
              </div>
              {entry.output ? <AnsiOutput>{entry.output}</AnsiOutput> : null}
              {entry.code !== 0 ? (
                <div className="mt-0.5 text-ui-xs text-destructive">exit {entry.code}</div>
              ) : null}
            </div>
          ))
        )}
        {busy ? <div className="text-ui-sm text-foreground-subtle">running…</div> : null}
      </div>

      <div className="flex flex-none items-center gap-1.5 border-t border-border px-2.5 py-1.5">
        <span className="shrink-0 font-mono text-ui-sm text-brand">$</span>
        <input
          aria-label="Terminal command"
          autoComplete="off"
          autoCorrect="off"
          className="min-w-0 flex-1 bg-transparent font-mono text-ui-sm text-foreground outline-none placeholder:text-foreground-subtlest"
          disabled={busy}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Enter a command"
          spellCheck={false}
          value={draft}
        />
      </div>
    </div>
  )
}
