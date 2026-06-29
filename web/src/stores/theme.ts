import { computed, ref, watch } from 'vue'
import { defineStore } from 'pinia'

// 'system' follows the OS preference; 'light'/'dark' pin it explicitly.
export type Theme = 'light' | 'dark' | 'system'

// Keep this key and the resolution logic in sync with the inline no-FOUC script
// in index.html, which applies the theme before Vue mounts.
const STORAGE_KEY = 'tcglense_theme'
const MEDIA_QUERY = '(prefers-color-scheme: dark)'

function isTheme(value: unknown): value is Theme {
  return value === 'light' || value === 'dark' || value === 'system'
}

function readStored(): Theme {
  try {
    const stored = localStorage.getItem(STORAGE_KEY)
    return isTheme(stored) ? stored : 'system'
  } catch {
    // Storage unavailable (private mode, blocked): fall back to system.
    return 'system'
  }
}

function systemPrefersDark(): boolean {
  return (
    typeof window !== 'undefined' &&
    typeof window.matchMedia === 'function' &&
    window.matchMedia(MEDIA_QUERY).matches
  )
}

export const useThemeStore = defineStore('theme', () => {
  // The user's chosen mode (what we persist).
  const theme = ref<Theme>(readStored())
  // The OS preference, tracked reactively so resolvedTheme recomputes when it flips.
  const systemDark = ref(systemPrefersDark())

  // The concrete theme actually applied to the DOM.
  const resolvedTheme = computed<'light' | 'dark'>(() =>
    theme.value === 'system' ? (systemDark.value ? 'dark' : 'light') : theme.value,
  )

  function setTheme(next: Theme) {
    theme.value = next
  }

  // Persist the chosen mode (not the resolved value, so 'system' stays 'system').
  watch(theme, (value) => {
    try {
      localStorage.setItem(STORAGE_KEY, value)
    } catch {
      // Storage unavailable: still apply the theme for this session.
    }
  })

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
