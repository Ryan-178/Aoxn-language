'use client'

/**
 * Git change list.
 *
 * The status shape comes from the Rust `git status --porcelain -z` parser;
 * this component only decides how to present it. Clicking a row asks the
 * workbench for a side-by-side diff, which Monaco already renders.
 */

import { useCallback, useEffect, useState } from 'react'
import {
  FilePlus2Icon,
  FileQuestionIcon,
  FileSymlinkIcon,
  FileX2Icon,
  GitBranchIcon,
  RefreshCwIcon,
  TriangleAlertIcon,
} from 'lucide-react'
import * as api from '@/lib/bridge'
import { cn } from '@/components/lib/utils'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'

const STATUS_META: Record<api.GitChange['status'], { label: string; className: string; icon: typeof FilePlus2Icon }> = {
  modified: {
    label: 'M',
    className: 'text-git-modified',
    icon: FileSymlinkIcon,
  },
  added: {
    label: 'A',
    className: 'text-git-added',
    icon: FilePlus2Icon,
  },
  deleted: {
    label: 'D',
    className: 'text-git-deleted',
    icon: FileX2Icon,
  },
  renamed: {
    label: 'R',
    className: 'text-git-renamed',
    icon: FileSymlinkIcon,
  },
  untracked: {
    label: 'U',
    className: 'text-git-untracked',
    icon: FileQuestionIcon,
  },
  ignored: {
    label: 'I',
    className: 'text-git-ignored',
    icon: FileQuestionIcon,
  },
  conflicted: {
    label: '!',
    className: 'text-destructive',
    icon: TriangleAlertIcon,
  },
}

export interface GitPanelProps {
  onOpenDiff(path: string): void
  onReveal?(path: string): void
  /** Keep the title-bar branch badge in sync with this panel's refresh. */
  onStatus?(status: api.GitStatus): void
  className?: string
}

export function GitPanel({ onOpenDiff, onStatus, className }: GitPanelProps) {
  const [state, setState] = useState<api.GitStatus | null>(null)
  const [loading, setLoading] = useState(true)

  const refresh = useCallback(async () => {
    setLoading(true)
    let next: api.GitStatus
    try {
      next = await api.gitStatus()
    } catch (error) {
      next = {
        isRepo: false,
        branch: '',
        changes: [],
        error: error instanceof Error ? error.message : String(error),
      }
    }
    setState(next)
    onStatus?.(next)
    setLoading(false)
  }, [onStatus])

  useEffect(() => {
    void refresh()
  }, [refresh])

  return (
    <div className={cn('flex h-full min-h-0 flex-col bg-panel', className)}>
      <div className="flex h-9 flex-none items-center gap-2 border-b border-border px-2.5">
        <GitBranchIcon className="size-3.5 text-foreground-subtle" />
        <span className="text-ui-sm font-medium">Source Control</span>
        {state?.branch ? (
          <Badge className="max-w-32 truncate" variant="secondary">
            {state.branch}
          </Badge>
        ) : null}
        <span className="flex-1" />
        <Button
          aria-label="Refresh git status"
          disabled={loading}
          onClick={() => void refresh()}
          size="icon-xs"
          type="button"
          variant="ghost"
        >
          <RefreshCwIcon className={cn('size-3', loading && 'animate-spin')} />
        </Button>
      </div>

      <div className="min-h-0 flex-1 overflow-auto px-1.5 py-1.5">
        {loading && !state ? (
          <div className="flex items-center gap-2 px-2 py-3 text-ui-sm text-foreground-subtle">
            <Spinner className="size-3.5" /> Reading git status…
          </div>
        ) : state?.error ? (
          <div className="m-1 rounded-lg border border-destructive/30 bg-destructive-surface px-2.5 py-2 text-ui-sm text-destructive-surface-foreground">
            {state.error}
          </div>
        ) : !state?.isRepo ? (
          <div className="px-2 py-6 text-center text-ui-sm text-foreground-subtle">
            This folder is not a git repository yet.
          </div>
        ) : state.changes.length === 0 ? (
          <div className="px-2 py-6 text-center text-ui-sm text-foreground-subtle">
            Working tree clean.
          </div>
        ) : (
          <div className="flex flex-col gap-0.5">
            {state.changes.map((change) => {
              const meta = STATUS_META[change.status]
              const Icon = meta.icon
              const leaf = change.path.split('/').pop() ?? change.path
              const parent = change.path.slice(0, Math.max(0, change.path.length - leaf.length - 1))
              return (
                <button
                  className="flex w-full items-center gap-1.5 rounded-md px-2 py-1 text-left transition-colors hover:bg-surface-hover"
                  key={`${change.path}-${change.staged}`}
                  onClick={() => onOpenDiff(change.path)}
                  title={change.path}
                  type="button"
                >
                  <Icon className={cn('size-3.5 shrink-0', meta.className)} />
                  <span className="min-w-0 flex-1 truncate">
                    <span className="text-ui-sm text-foreground">{leaf}</span>
                    {parent ? (
                      <span className="ml-1 text-ui-xs text-foreground-subtlest">{parent}</span>
                    ) : null}
                  </span>
                  {change.staged ? (
                    <span className="size-1.5 shrink-0 rounded-full bg-success" title="Staged" />
                  ) : null}
                  <span
                    className={cn('w-3 shrink-0 text-center font-mono text-ui-xs', meta.className)}
                  >
                    {meta.label}
                  </span>
                </button>
              )
            })}
          </div>
        )}
      </div>
    </div>
  )
}
