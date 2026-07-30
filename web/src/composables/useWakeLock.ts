import { onScopeDispose, ref, watch, type Ref } from 'vue'

/**
 * Hold a Screen Wake Lock while a condition is true — so a life counter sitting on the table
 * between turns doesn't dim and lock itself mid-game.
 *
 * Three things make this more than a one-line `navigator.wakeLock.request()`:
 *
 * 1. **The lock does not survive backgrounding.** The browser releases it whenever the page
 *    stops being visible (tab switch, phone locked by hand, app switcher), and it does *not*
 *    come back on its own. So a `visibilitychange` listener re-requests it — without that, the
 *    counter keeps the screen awake right up until the first time you check something else, and
 *    then silently stops for the rest of the game.
 * 2. **A request can only come from a visible page.** Re-requesting while hidden throws, so the
 *    re-acquire is gated on `visibilityState === 'visible'`.
 *    A rejected request is not an error worth surfacing — a screen that dims is a papercut, not
 *    a broken feature — so failures set {@link WakeLock.active} back to `false` and are
 *    otherwise swallowed.
 * 3. **Support is partial.** Firefox and iOS Safari before 16.4 have no `wakeLock` at all, so
 *    {@link WakeLock.supported} lets the UI say "keep the screen on" honestly rather than
 *    showing a control that does nothing.
 *
 * The lock is released when `enabled` goes false and on scope disposal, so leaving the counter
 * always hands the screen back.
 *
 * ```ts
 * const { active, supported } = useWakeLock(() => session.value?.status === 'active')
 * ```
 */
export interface WakeLock {
  /** Whether this browser exposes the Screen Wake Lock API at all. */
  supported: boolean
  /** Whether a lock is held right now (false while hidden, or after a rejected request). */
  active: Ref<boolean>
}

/** The slice of the Wake Lock API this composable uses, so tests can stub it. */
interface WakeLockSentinelLike {
  released: boolean
  release: () => Promise<void>
  addEventListener: (type: 'release', listener: () => void) => void
}

interface WakeLockNavigator {
  wakeLock?: { request: (type: 'screen') => Promise<WakeLockSentinelLike> }
}

export function useWakeLock(enabled: () => boolean): WakeLock {
  const nav = typeof navigator === 'undefined' ? undefined : (navigator as WakeLockNavigator)
  const supported = Boolean(nav?.wakeLock)
  const active = ref(false)

  let sentinel: WakeLockSentinelLike | null = null
  // Requests are async, so a fast enable → disable could otherwise land a lock after we
  // meant to drop it. Every request checks this generation before keeping its sentinel.
  let generation = 0

  async function acquire() {
    if (!nav?.wakeLock || sentinel) return
    if (typeof document !== 'undefined' && document.visibilityState !== 'visible') return
    const requested = ++generation
    try {
      const next = await nav.wakeLock.request('screen')
      if (requested !== generation || !enabled()) {
        // Superseded (or no longer wanted) while awaiting — drop it rather than leak it.
        await next.release().catch(() => {})
        return
      }
      sentinel = next
      active.value = true
      // The browser can release the lock on its own; reflect that instead of claiming a
      // lock we no longer hold.
      next.addEventListener('release', () => {
        if (sentinel === next) {
          sentinel = null
          active.value = false
        }
      })
    } catch {
      // Denied, unsupported in this context, or the document went hidden mid-request. A dim
      // screen is a papercut, not a failure worth interrupting a game for.
      active.value = false
    }
  }

  async function release() {
    generation += 1
    const held = sentinel
    sentinel = null
    active.value = false
    if (held && !held.released) await held.release().catch(() => {})
  }

  function onVisibilityChange() {
    if (!enabled()) return
    // Re-take the lock the browser dropped when we were backgrounded.
    if (document.visibilityState === 'visible') void acquire()
  }

  if (supported && typeof document !== 'undefined') {
    document.addEventListener('visibilitychange', onVisibilityChange)
    onScopeDispose(() => document.removeEventListener('visibilitychange', onVisibilityChange))
  }

  watch(
    enabled,
    (on) => {
      if (on) void acquire()
      else void release()
    },
    { immediate: true },
  )

  onScopeDispose(() => void release())

  return { supported, active }
}
