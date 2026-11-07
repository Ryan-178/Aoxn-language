'use client'

/**
 * File preview for the bottom panel: images, PDFs, markdown and plain text.
 * The surrounding chrome mirrors ZCode's preview pane (breadcrumb on top,
 * content on a paper surface); the renderer is deliberately narrow because
 * the editor already handles source files.
 */

import { useEffect, useMemo, useState } from 'react'
import { FileTextIcon, ImageIcon, RotateCwIcon } from 'lucide-react'
import * as api from '@/lib/bridge'
import { cn } from '@/components/lib/utils'
import { Button } from '@/components/ui/button'
import { MessageResponse } from '@/components/ai-elements/message'

const IMAGE_EXT = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'ico', 'bmp', 'avif'])
const PDF_EXT = new Set(['pdf'])
const MARKDOWN_EXT = new Set(['md', 'markdown'])

export interface PreviewPanelProps {
  path: string
  /** Already-loaded editor text, when the file is open and unmodified is fine. */
  text?: string
  className?: string
}

function extension(path: string): string {
  const leaf = path.split(/[\\/]/).pop() ?? path
  const dot = leaf.lastIndexOf('.')
  return dot < 0 ? '' : leaf.slice(dot + 1).toLowerCase()
}

export function PreviewPanel({ path, text, className }: PreviewPanelProps) {
  const ext = useMemo(() => extension(path), [path])
  const [bytes, setBytes] = useState<api.FileBytes | null>(null)
  const [loadedText, setLoadedText] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [seq, setSeq] = useState(0)

  const isImage = IMAGE_EXT.has(ext)
  const isPdf = PDF_EXT.has(ext)
  const isMarkdown = MARKDOWN_EXT.has(ext)

  useEffect(() => {
    let alive = true
    setError(null)
    setBytes(null)
    setLoadedText(null)
    setLoading(true)
    void (async () => {
      try {
        if (isImage || isPdf) {
          const result = await api.readBytes(path)
          if (alive) setBytes(result)
        } else if (text !== undefined) {
          if (alive) setLoadedText(text)
        } else {
          const result = await api.readFile(path)
          if (alive) setLoadedText(result.text)
        }
      } catch (e) {
        if (alive) setError(e instanceof Error ? e.message : String(e))
      } finally {
        if (alive) setLoading(false)
      }
    })()
    return () => {
      alive = false
    }
  }, [isImage, isPdf, path, seq, text])

  const dataUrl = bytes ? `data:${bytes.mime};base64,${bytes.base64}` : null
  const leaf = path.split(/[\\/]/).pop() ?? path

  return (
    <div className={cn('flex h-full min-h-0 flex-col bg-panel', className)}>
      <div className="flex h-9 flex-none items-center gap-2 border-b border-border px-2.5">
        {isImage ? (
          <ImageIcon className="size-3.5 text-foreground-subtle" />
        ) : (
          <FileTextIcon className="size-3.5 text-foreground-subtle" />
        )}
        <span className="text-ui-sm font-medium">{leaf}</span>
        <span className="min-w-0 flex-1 truncate text-ui-xs text-foreground-subtlest" title={path}>
          {path}
        </span>
        <Button
          aria-label="Reload preview"
          onClick={() => setSeq((value) => value + 1)}
          size="icon-xs"
          type="button"
          variant="ghost"
        >
          <RotateCwIcon className={cn('size-3', loading && 'animate-spin')} />
        </Button>
      </div>

      <div className="min-h-0 flex-1 overflow-auto">
        {error ? (
          <div className="m-3 rounded-lg border border-destructive/30 bg-destructive-surface px-3 py-2 text-ui-sm text-destructive-surface-foreground">
            {error}
          </div>
        ) : loading ? (
          <div className="p-4 text-ui-sm text-foreground-subtle">Loading preview…</div>
        ) : isImage && dataUrl ? (
          <div className="flex min-h-full items-center justify-center bg-background p-4">
            {/* eslint-disable-next-line @next/next/no-img-element */}
            <img
              alt={leaf}
              className="max-h-full max-w-full rounded-md object-contain shadow-sm"
              src={dataUrl}
            />
          </div>
        ) : isPdf && dataUrl ? (
          <iframe className="h-full min-h-96 w-full border-0" src={dataUrl} title={leaf} />
        ) : isMarkdown && loadedText !== null ? (
          <div className="mx-auto max-w-3xl px-6 py-5">
            <MessageResponse>{loadedText}</MessageResponse>
          </div>
        ) : loadedText !== null ? (
          <pre className="whitespace-pre-wrap break-words p-4 font-mono text-ui-sm leading-relaxed text-foreground">
            {loadedText}
          </pre>
        ) : bytes ? (
          <div className="p-4 text-ui-sm text-foreground-subtle">
            Binary file · {bytes.size.toLocaleString()} bytes · {bytes.mime}
          </div>
        ) : (
          <div className="p-4 text-ui-sm text-foreground-subtle">Nothing to preview.</div>
        )}
      </div>
    </div>
  )
}
