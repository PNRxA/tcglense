import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { nextTick } from 'vue'
import { useThemeStore } from '../theme'

describe('theme store', () => {
  beforeEach(() => {
    localStorage.clear()
    document.documentElement.classList.remove('dark')
    setActivePinia(createPinia())
  })

  it('defaults to system when nothing is stored', () => {
    const theme = useThemeStore()
    expect(theme.theme).toBe('system')
  })

  it('reads a persisted choice', () => {
    localStorage.setItem('tcglense_theme', 'dark')
    const theme = useThemeStore()
    expect(theme.theme).toBe('dark')
    expect(theme.resolvedTheme).toBe('dark')
  })

  it('ignores an invalid stored value', () => {
    localStorage.setItem('tcglense_theme', 'rainbow')
    const theme = useThemeStore()
    expect(theme.theme).toBe('system')
  })

  // jsdom has no matchMedia, so the OS preference resolves to light.
  it('resolves system to light without an OS dark preference', () => {
    const theme = useThemeStore()
    expect(theme.theme).toBe('system')
    expect(theme.resolvedTheme).toBe('light')
  })

  it('toggles the .dark class on <html> as the resolved theme changes', async () => {
    const theme = useThemeStore()
    theme.setTheme('dark')
    await nextTick()
    expect(document.documentElement.classList.contains('dark')).toBe(true)

    theme.setTheme('light')
    await nextTick()
    expect(document.documentElement.classList.contains('dark')).toBe(false)
  })

  it('persists the chosen mode to localStorage', async () => {
    const theme = useThemeStore()
    theme.setTheme('dark')
    await nextTick()
    expect(localStorage.getItem('tcglense_theme')).toBe('dark')
  })
})
