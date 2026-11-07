'use client'

import dynamic from 'next/dynamic'
import { useEffect } from 'react'
import { loader } from '@monaco-editor/react'
import type * as Monaco from 'monaco-editor'

// Monaco is imported on the client only (it touches `window` at module
// scope), and the workbench is loaded the same way so the static export has
// no server component trying to render a 5 MB editor.
const Workbench = dynamic(() => import('./Workbench'), {
  ssr: false,
  loading: () => (
    <div className="wb-placeholder" style={{ height: '100vh' }}>
      Starting the Aoxn IDE…
    </div>
  ),
})

export default function Page() {
  useEffect(() => {
    void (async () => {
      // `@monaco-editor/react` loads Monaco from jsdelivr by DEFAULT. That
      // is fine for a website and wrong for a desktop app: the IDE has to
      // work on a machine with no network, which is the normal condition for
      // someone compiling Aoxn on a locked-down box. Pointing the loader at
      // the bundled copy is one line and removes the dependency entirely.
      //
      // The Aoxn language and the theme are registered by the Editor itself
      // (in `beforeMount`), which runs strictly between Monaco's load and
      // the first model's creation — a page-level `registerAoxn` here raced
      // the editor's mount in v0.31.0 and let the default theme flash.
      const monaco = await import('monaco-editor')
      loader.config({ monaco: monaco as unknown as typeof Monaco })
    })()
  }, [])

  return <Workbench />
}
