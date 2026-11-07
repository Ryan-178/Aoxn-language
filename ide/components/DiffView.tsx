'use client'

import dynamic from 'next/dynamic'
import { registerAoxn } from '@/lib/monaco'
import { languageFor } from '@/components/Editor'

const MonacoDiffEditor = dynamic(
  () => import('@monaco-editor/react').then((module) => module.DiffEditor),
  {
    ssr: false,
    loading: () => <div className="wb-placeholder">Loading diff…</div>,
  },
)

export interface DiffViewProps {
  path: string
  original: string
  modified: string
  theme: string
}

export function DiffView({ path, original, modified, theme }: DiffViewProps) {
  return (
    <MonacoDiffEditor
      beforeMount={(monaco) => registerAoxn(monaco)}
      language={languageFor(path)}
      modified={modified}
      original={original}
      theme={theme}
      options={{
        readOnly: true,
        renderSideBySide: true,
        minimap: { enabled: false },
        scrollBeyondLastLine: false,
        automaticLayout: true,
        fontSize: 13,
        fontFamily: 'var(--font-mono)',
        lineHeight: 20,
        renderOverviewRuler: false,
        scrollbar: { verticalScrollbarSize: 12, horizontalScrollbarSize: 12, useShadows: false },
        padding: { top: 10, bottom: 24 },
      }}
    />
  )
}
