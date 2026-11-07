/**
 * Tests for the Zai theme resolver.
 *
 * `applyTheme` touches `document`, but `resolveTheme` is the decision the
 * rest of the UI depends on (Monaco theme name, color-scheme, the dark
 * class). Keeping it pure is what lets this run under `node --test` with no
 * DOM shim.
 */

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { resolveTheme } from '../lib/theme.ts'

test('the two explicit Zai themes resolve without a DOM', () => {
  assert.equal(resolveTheme('zai-dark'), 'dark')
  assert.equal(resolveTheme('zai-light'), 'light')
})

test('system follows the OS preference and cleans up', () => {
  const original = globalThis.window
  try {
    ;(globalThis as unknown as { window: unknown }).window = {
      matchMedia: () => ({ matches: true }),
    }
    assert.equal(resolveTheme('system'), 'dark')
    ;(globalThis as unknown as { window: unknown }).window = {
      matchMedia: () => ({ matches: false }),
    }
    assert.equal(resolveTheme('system'), 'light')
  } finally {
    ;(globalThis as unknown as { window: unknown }).window = original
  }
})
