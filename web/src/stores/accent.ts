import { computed, watch } from 'vue'
import { defineStore } from 'pinia'
import { persistedRef } from '@/lib/persistedRef'
import { type Accent, DEFAULT_ACCENT, isAccent } from '@/lib/accent'
import { useAuthStore } from '@/stores/auth'

// Keep this key and the validation in sync with the inline no-FOUC script in
// index.html, which stamps `data-accent` before Vue mounts.
const STORAGE_KEY = 'tcglense_accent'

export const useAccentStore = defineStore('accent', () => {
  const auth = useAuthStore()

  // The device-local choice: the guest accent when signed out, and the last-seen
  // resolved accent otherwise (see the mirror in the watch below).
  const localAccent = persistedRef<Accent>(STORAGE_KEY, DEFAULT_ACCENT, isAccent)

  // The account's server-persisted accent wins while signed in (like the currency
  // preference); the local value covers guests and the pre-restore window.
  const accent = computed<Accent>(() => {
    const server = auth.user?.accent
    return isAccent(server) ? server : localAccent.value
  })

  // Reflect the accent onto <html> as the `data-accent` attribute main.css's preset
  // token blocks key off — and mirror it into the local ref, so the next boot's
  // no-FOUC script paints the account's accent before auth restores. The mirror
  // deliberately survives logout, like the theme choice: the device keeps its look
  // until another account's accent is seen. Changing the accent is server-side only
  // (useSetAccentMutation via the settings page) — there is no guest picker.
  watch(
    accent,
    (value) => {
      if (typeof document !== 'undefined') {
        document.documentElement.dataset.accent = value
      }
      localAccent.value = value
    },
    { immediate: true },
  )

  return { accent }
})
