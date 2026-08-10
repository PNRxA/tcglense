import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { nextTick } from 'vue'
import { ACCENT_OPTIONS, isAccent } from '@/lib/accent'
import type { User } from '@/lib/api'
import { useAuthStore } from '@/stores/auth'
import { useAccentStore } from '../accent'

const USER: User = {
  id: 1,
  email: 'accent@example.com',
  created_at: '2026-08-01T00:00:00Z',
  username: null,
  discriminator: null,
  handle: null,
  currency: 'USD',
  accent: 'teal',
}

describe('accent vocabulary', () => {
  // Pins the wire vocabulary mirrored in api/src/accent.rs (SUPPORTED_ACCENTS) — a slug
  // added on one side only is either rejected by the API or renders as the default.
  it('mirrors the server slug list, order included', () => {
    expect(ACCENT_OPTIONS.map((o) => o.value)).toEqual([
      'pink',
      'ember',
      'violet',
      'teal',
      'blue',
      'green',
    ])
  })

  it('narrows only preset slugs', () => {
    expect(isAccent('pink')).toBe(true)
    expect(isAccent('Teal')).toBe(false)
    expect(isAccent('#ff00aa')).toBe(false)
    expect(isAccent(null)).toBe(false)
  })

  // The no-FOUC script in index.html carries a third copy of the slug list (it runs
  // before any module loads, so it can't import this one). Pin it here so a preset
  // added to accent.rs + accent.ts can't silently miss the pre-mount stamp — that
  // regression would only show as a default-colored first paint.
  it('matches the inline no-FOUC allow-list in index.html', () => {
    // vitest's root is web/, and jsdom's import.meta.url is not a file: URL.
    const html = readFileSync(resolve(process.cwd(), 'index.html'), 'utf-8')
    const match = html.match(/\[([^\]]+)\]\.indexOf\(accent\)/)
    expect(match).not.toBeNull()
    const inline = match![1]!.split(',').map((s) => s.trim().replace(/^'|'$/g, ''))
    expect(inline).toEqual(ACCENT_OPTIONS.map((o) => o.value))
  })
})

describe('accent store', () => {
  beforeEach(() => {
    localStorage.clear()
    delete document.documentElement.dataset.accent
    setActivePinia(createPinia())
  })

  it('defaults to pink and stamps data-accent on <html>', () => {
    const accent = useAccentStore()
    expect(accent.accent).toBe('pink')
    expect(document.documentElement.dataset.accent).toBe('pink')
  })

  it('reads a persisted local choice', () => {
    localStorage.setItem('tcglense_accent', 'violet')
    const accent = useAccentStore()
    expect(accent.accent).toBe('violet')
    expect(document.documentElement.dataset.accent).toBe('violet')
  })

  it('ignores an invalid stored value', () => {
    localStorage.setItem('tcglense_accent', 'rainbow')
    const accent = useAccentStore()
    expect(accent.accent).toBe('pink')
  })

  it('lets the signed-in account accent win over the local choice', async () => {
    localStorage.setItem('tcglense_accent', 'violet')
    const auth = useAuthStore()
    const accent = useAccentStore()
    auth.setUser(USER)
    await nextTick()
    expect(accent.accent).toBe('teal')
    expect(document.documentElement.dataset.accent).toBe('teal')
  })

  it('mirrors the resolved accent to localStorage for the pre-mount FOUC script', async () => {
    const auth = useAuthStore()
    useAccentStore()
    auth.setUser(USER)
    await nextTick()
    expect(localStorage.getItem('tcglense_accent')).toBe('teal')
  })

  it('keeps the mirrored accent after sign-out, like the theme choice', async () => {
    const auth = useAuthStore()
    const accent = useAccentStore()
    auth.setUser(USER)
    await nextTick()
    expect(accent.accent).toBe('teal')

    auth.user = null
    await nextTick()
    expect(accent.accent).toBe('teal')
    expect(document.documentElement.dataset.accent).toBe('teal')
  })
})
