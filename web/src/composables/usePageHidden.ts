import { onScopeDispose } from 'vue'

/**
 * Run `handler` the moment the page stops being visible — a tab switch, the phone locking,
 * the browser being backgrounded or closed, or a navigation away from the SPA.
 *
 * Three reasons this isn't a bare `visibilitychange` listener at the call site:
 *
 * 1. **`visibilitychange` alone isn't the whole signal.** A page can be torn down (or frozen
 *    into the back/forward cache) via `pagehide` without a visibility transition, so both are
 *    listened to. On mobile, backgrounding the browser usually fires *both*.
 * 2. **The handler must fire once per hidden period.** Since the two events overlap, a naive
 *    pair of listeners double-fires; anything with a side effect (releasing a device, saving
 *    tentative state) would then run twice. A latch fires once and re-arms when the page
 *    becomes visible again — including a `pageshow` restore out of the bfcache, which need
 *    not come with a `visibilitychange`.
 * 3. **Hidden is the last reliable moment.** A backgrounded mobile tab can be discarded
 *    without ever firing `beforeunload`/unmount, so this is where "release it / save it" work
 *    belongs — the handler should therefore do its work synchronously or best-effort, not
 *    assume it gets to await a round trip.
 *
 * Listeners are removed with the owning scope.
 */
export function onPageHidden(handler: () => void): void {
  if (typeof document === 'undefined' || typeof window === 'undefined') return

  // Latched so an overlapping visibilitychange + pagehide is one call, not two. Starts latched
  // when the scope is created on an already-hidden page: only a genuine visible → hidden
  // transition is "the user left".
  let latched = document.visibilityState === 'hidden'

  function fire() {
    if (latched) return
    latched = true
    handler()
  }

  function onVisibilityChange() {
    if (document.visibilityState === 'hidden') fire()
    else latched = false
  }

  function onPageShow() {
    latched = false
  }

  document.addEventListener('visibilitychange', onVisibilityChange)
  window.addEventListener('pagehide', fire)
  window.addEventListener('pageshow', onPageShow)
  onScopeDispose(() => {
    document.removeEventListener('visibilitychange', onVisibilityChange)
    window.removeEventListener('pagehide', fire)
    window.removeEventListener('pageshow', onPageShow)
  })
}
