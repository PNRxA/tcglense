import { ref, watch, type Ref } from 'vue'

/**
 * A ref seeded from localStorage and persisted back whenever it changes — for
 * personal display preferences (theme, card size). The read is guarded so blocked
 * storage (private mode) falls back to `fallback`, and `isValid` rejects a stored
 * value that isn't a legal `T` (a stale or hand-edited key), so the ref is always a
 * valid `T`. Writes are best-effort: a storage failure still honours the choice for
 * the session.
 */
export function persistedRef<T>(
  key: string,
  fallback: T,
  isValid: (value: unknown) => value is T,
): Ref<T> {
  function read(): T {
    try {
      const stored = localStorage.getItem(key)
      return isValid(stored) ? stored : fallback
    } catch {
      // Storage unavailable (private mode, blocked): fall back to the default.
      return fallback
    }
  }

  const state = ref(read()) as Ref<T>

  watch(state, (value) => {
    try {
      localStorage.setItem(key, String(value))
    } catch {
      // Storage unavailable: still honour the choice for this session.
    }
  })

  return state
}
