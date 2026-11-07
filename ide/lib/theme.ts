'use client'

/**
 * Theme handling, ported from ZCode's `useTheme` (v3.17.2).
 *
 * The IDE offers the two Zai themes only: they are the pair the design
 * tokens are validated against. `system` follows the OS preference and
 * resolves to one of them.
 */

import { useCallback, useEffect, useState } from 'react'

export type Theme = 'zai-light' | 'zai-dark' | 'system'
export type ResolvedTheme = 'light' | 'dark'

const STORAGE_KEY = 'aoxn-theme'

export function resolveTheme(theme: Theme): ResolvedTheme {
  if (theme === 'system') {
    return typeof window !== 'undefined' &&
      window.matchMedia('(prefers-color-scheme: dark)').matches
      ? 'dark'
      : 'light'
  }
  return theme === 'zai-dark' ? 'dark' : 'light'
}

export function applyTheme(theme: Theme): void {
  if (typeof document === 'undefined') return
  const resolved = resolveTheme(theme)
  const applied = theme === 'system' ? (resolved === 'dark' ? 'zai-dark' : 'zai-light') : theme
  const root = document.documentElement
  root.classList.toggle('dark', resolved === 'dark')
  root.classList.toggle('theme-zai-light', applied === 'zai-light')
  root.classList.toggle('theme-zai-dark', applied === 'zai-dark')
  root.style.colorScheme = resolved
}

function isTheme(value: string | null): value is Theme {
  return value === 'zai-light' || value === 'zai-dark' || value === 'system'
}

export function useTheme() {
  const [theme, setThemeState] = useState<Theme>(() => {
    if (typeof window === 'undefined') return 'zai-dark'
    const saved = window.localStorage.getItem(STORAGE_KEY)
    return isTheme(saved) ? saved : 'zai-dark'
  })

  const setTheme = useCallback((next: Theme) => {
    window.localStorage.setItem(STORAGE_KEY, next)
    setThemeState(next)
    applyTheme(next)
  }, [])

  useEffect(() => {
    applyTheme(theme)
    if (theme !== 'system') return
    const media = window.matchMedia('(prefers-color-scheme: dark)')
    const handler = () => applyTheme('system')
    media.addEventListener('change', handler)
    return () => media.removeEventListener('change', handler)
  }, [theme])

  return { theme, setTheme, resolved: resolveTheme(theme) } as const
}
