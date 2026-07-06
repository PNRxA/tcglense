import type { RouteLocationRaw, Router } from 'vue-router'

// Route-chunk prefetch: warm the JS bundle behind a route the user is likely to open
// next, so the dynamic import() overlaps the pointer-to-click gap instead of blocking
// the navigation on a slow uplink. JS chunks only — never data or images (Scryfall's
// no-bulk-download guideline). Every helper is best-effort: a warm that fails just
// means the chunk loads on navigation as it would have anyway, so nothing here throws.

// Resolve `to` and kick off the import() behind each lazily-loaded route component,
// ignoring the result. Wrapped so an unresolvable location or a rejected import can
// never surface to the caller — a hover/focus handler must not throw. Safe with the
// stub/memory routers used in component tests: eager component objects aren't
// functions, so they're skipped and this is a silent no-op.
export function prefetchRouteChunks(router: Router, to: RouteLocationRaw): void {
  try {
    const resolved = router.resolve(to)
    for (const record of resolved.matched) {
      for (const component of Object.values(record.components ?? {})) {
        // A lazy route factory is `() => import(...)`; an eager component is an object.
        if (typeof component === 'function') {
          ;(component as () => Promise<unknown>)().catch(() => {})
        }
      }
    }
  } catch {
    // Unresolvable location — nothing to warm.
  }
}

// Warm a batch of routes (plus any extra import factories, e.g. the card-detail dialog)
// once the browser is idle, so first-hover navigations are already primed without
// competing with the initial render. requestIdleCallback where available, a 2s timeout
// fallback otherwise (Safari has no rIC).
export function scheduleIdleWarm(
  router: Router,
  locations: RouteLocationRaw[],
  extraLoaders?: Array<() => Promise<unknown>>,
): void {
  const run = () => {
    for (const location of locations) prefetchRouteChunks(router, location)
    for (const load of extraLoaders ?? []) load().catch(() => {})
  }
  const ric = (globalThis as { requestIdleCallback?: (cb: () => void) => void }).requestIdleCallback
  if (typeof ric === 'function') ric(run)
  else setTimeout(run, 2000)
}
