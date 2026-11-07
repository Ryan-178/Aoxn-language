'use client'

/**
 * Explorer built on ZCode's `ai-elements/file-tree` (collapsible rows,
 * chevrons, folder icons). The flat `{path, depth}` rows the Rust scanner
 * returns are nested once here; expansion and selection stay owned by the
 * workbench so the keyboard palette and the tree agree.
 */

import { useMemo } from 'react'
import {
  FileCode2Icon,
  FileIcon,
  FileJsonIcon,
  FilePlus2Icon,
  FileTextIcon,
  FolderPlusIcon,
  FolderOpenIcon,
  ImageIcon,
  RefreshCwIcon,
  SearchIcon,
} from 'lucide-react'
import type { TreeNode } from '@/lib/bridge'
import { cn } from '@/components/lib/utils'
import { Button } from '@/components/ui/button'
import { FileTree, FileTreeFile, FileTreeFolder } from '@/components/ai-elements/file-tree'

interface NestedNode {
  node: TreeNode
  children: NestedNode[]
}

function nest(flat: TreeNode[]): NestedNode[] {
  const roots: NestedNode[] = []
  const stack: NestedNode[] = []
  for (const node of flat) {
    const entry: NestedNode = { node, children: [] }
    while (stack.length > 0 && stack[stack.length - 1].node.depth >= node.depth) stack.pop()
    if (stack.length > 0) stack[stack.length - 1].children.push(entry)
    else roots.push(entry)
    stack.push(entry)
  }
  return roots
}

function fileIcon(path: string) {
  const lower = path.toLowerCase()
  if (lower.endsWith('.ax') || lower.endsWith('.ts') || lower.endsWith('.tsx')) {
    return <FileCode2Icon className="size-4 text-brand" />
  }
  if (lower.endsWith('.json') || lower.endsWith('.toml') || lower.endsWith('.yaml') || lower.endsWith('.yml')) {
    return <FileJsonIcon className="size-4 text-icon-blue" />
  }
  if (lower.endsWith('.md') || lower.endsWith('.txt')) {
    return <FileTextIcon className="size-4 text-foreground-subtle" />
  }
  if (/\.(png|jpe?g|gif|webp|svg|ico|bmp|avif)$/i.test(lower)) {
    return <ImageIcon className="size-4 text-icon-purple" />
  }
  return <FileIcon className="size-4 text-foreground-subtle" />
}

export interface FileExplorerProps {
  root: string
  tree: TreeNode[]
  expanded: Set<string>
  selected: string | null
  onExpandedChange(next: Set<string>): void
  onSelect(path: string, isDir: boolean): void
  onNewFile(): void
  onNewFolder(): void
  onRefresh(): void
  onSearch(): void
  onOpenFolder(): void
  className?: string
}

export function FileExplorer({
  root,
  tree,
  expanded,
  selected,
  onExpandedChange,
  onSelect,
  onNewFile,
  onNewFolder,
  onRefresh,
  onSearch,
  onOpenFolder,
  className,
}: FileExplorerProps) {
  const nested = useMemo(() => nest(tree), [tree])
  const directories = useMemo(() => new Set(tree.filter((node) => node.isDir).map((node) => node.path)), [tree])
  const rootLabel = root ? (root.split(/[\\/]/).filter(Boolean).pop() ?? root) : 'Explorer'

  const renderNode = (entry: NestedNode) => {
    const { node } = entry
    if (node.isDir) {
      return (
        <FileTreeFolder key={node.path} name={node.name} path={node.path}>
          {entry.children.map(renderNode)}
        </FileTreeFolder>
      )
    }
    return (
      <FileTreeFile icon={fileIcon(node.path)} key={node.path} name={node.name} path={node.path} />
    )
  }

  return (
    <div className={cn('flex h-full min-h-0 flex-col bg-sidebar', className)}>
      <div className="flex h-9 flex-none items-center gap-1.5 border-b border-border px-2.5">
        <span className="min-w-0 flex-1 truncate text-ui-sm font-semibold text-foreground" title={root}>
          {rootLabel}
        </span>
        <Button aria-label="New file" onClick={onNewFile} size="icon-xs" type="button" variant="ghost">
          <FilePlus2Icon className="size-3.5" />
        </Button>
        <Button aria-label="New folder" onClick={onNewFolder} size="icon-xs" type="button" variant="ghost">
          <FolderPlusIcon className="size-3.5" />
        </Button>
        <Button aria-label="Search files" onClick={onSearch} size="icon-xs" type="button" variant="ghost">
          <SearchIcon className="size-3.5" />
        </Button>
        <Button aria-label="Refresh" onClick={onRefresh} size="icon-xs" type="button" variant="ghost">
          <RefreshCwIcon className="size-3.5" />
        </Button>
      </div>

      <div className="min-h-0 flex-1 overflow-auto p-1.5">
        {tree.length === 0 ? (
          <div className="flex flex-col items-center gap-3 px-3 py-10 text-center">
            <div className="flex size-10 items-center justify-center rounded-xl bg-surface text-foreground-subtle">
              <FolderOpenIcon className="size-5" />
            </div>
            <div className="text-ui-sm text-foreground-subtle">No folder open.</div>
            <Button onClick={onOpenFolder} size="sm" type="button" variant="outline">
              Open folder
            </Button>
          </div>
        ) : (
          <FileTree
            className="border-0 bg-transparent p-0 font-sans text-ui-sm"
            expanded={expanded}
            onExpandedChange={onExpandedChange}
            onSelect={(path) => onSelect(path, directories.has(path))}
            selectedPath={selected ?? undefined}
          >
            {nested.map(renderNode)}
          </FileTree>
        )}
      </div>
    </div>
  )
}
