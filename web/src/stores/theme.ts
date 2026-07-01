import { computed, ref, watch } from 'vue'
import { defineStore } from 'pinia'
import { persistedRef } from '@/lib/persistedRef'

// 'system' follows the OS preference; 'light'/'dark' pin it explicitly.
export type Theme = 'light' | 'dark' | 'system'

// Keep this key and the resolution logic in sync with the inline no-FOUC script
// in index.html, which applies the theme before Vue mounts.
const STORAGE_KEY = 'tcglense_theme'
const MEDIA_QUERY = '(prefers-color-scheme: dark)'

function isTheme(value: unknown): value is Theme {
  return value === 'light' || value === 'dark' || value === 'system'
}

function systemPrefersDark(): boolean {
  return (
    typeof window !== 'undefined' &&
    typeof window.matchMedia === 'function' &&
    window.matchMedia(MEDIA_QUERY).matches
  )
}

export const useThemeStore = defineStore('theme', () => {
  // The user's chosen mode (what we persist — not the resolved value, so 'system'
  // stays 'system').
  const theme = persistedRef<Theme>(STORAGE_KEY, 'system', isTheme)
  // The OS preference, tracked reactively so resolvedTheme recomputes when it flips.
  const systemDark = ref(systemPrefersDark())

  // The concrete theme actually applied to the DOM.
  const resolvedTheme = computed<'light' | 'dark'>(() =>
    theme.value === 'system' ? (systemDark.value ? 'dark' : 'light') : theme.value,
  )

  function setTheme(next: Theme) {
    theme.value = next
  }

  // Reflect the resolved theme onto <html> via the `.dark` class Tailwind keys off.
  watch(
    resolvedTheme,
    (value) => {
      if (typeof document !== 'undefined') {
        document.documentElement.classList.toggle('dark', value === 'dark')
      }
    },
    { immediate: true },
  )

  // Track the OS preference so 'system' mode follows it live. The store is an
  // app-lifetime singleton, so the listener never needs removing.
  if (typeof window !== 'undefined' && typeof window.matchMedia === 'function') {
    window.matchMedia(MEDIA_QUERY).addEventListener('change', (event) => {
      systemDark.value = event.matches
    })
  }

  return { theme, resolvedTheme, setTheme }
})
