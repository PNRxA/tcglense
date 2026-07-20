import type { Router } from 'vue-router'

// Recover from a navigation that fails because a lazy route chunk is missing.
//
// Every view except HomeView is a dynamic `import()` (see the route table), so a
// navigation fetches that route's hashed JS/CSS chunk on demand. After a production
// deploy the server no longer has the hashed chunks the *currently loaded* index.html
// references — the new build emits new hashes and drops the old files — so the next
// in-app navigation's import 404s. vue-router aborts the navigation and calls its
// onError handlers; with no recovery the click silently does nothing until the user
// manually refreshes (which fetches a fresh index.html with the new hashes). This turns
// that dead click into a hard navigation to the intended URL: the browser loads a fresh
// index.html and then the route, so the user lands where they clicked, now on the latest
// build. (A soft router retry can't help — the stale index.html only knows the old,
// now-missing hashes; only a full document load re-reads the current asset map.)

// If a hard navigation to the SAME target has *just* failed the same way, the freshly
// loaded page also couldn't find the chunk — a genuinely broken deploy, not a stale one.
// Stop instead of looping reloads forever; let the failed navigation stand so the error
// is at least visible in the console rather than the tab thrashing.
const RELOAD_GUARD_KEY = 'tcgl:chunk-reload'
const RELOAD_GUARD_WINDOW_MS = 10_000

// The message a failed dynamic import throws differs per engine; match the stable
// fragment of each. Vite's module/CSS preload helper adds its own two.
function isChunkLoadError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error)
  return (
    /Failed to fetch dynamically imported module/i.test(message) || // Chromium
    /error loading dynamically imported module/i.test(message) || // Firefox
    /Importing a module script failed/i.test(message) || // Safari/WebKit
    /Unable to preload (CSS|module)/i.test(message) // Vite's preload helper
  )
}

// sessionStorage can throw (Safari private mode, storage disabled); never let the
// bookkeeping crash the handler — a missing guard just means we don't dedupe reloads.
function readGuard(): { path?: string; at?: number } {
  try {
    return JSON.parse(sessionStorage.getItem(RELOAD_GUARD_KEY) ?? '{}')
  } catch {
    return {}
  }
}

function writeGuard(value: { path: string; at: number } | null): void {
  try {
    if (value) sessionStorage.setItem(RELOAD_GUARD_KEY, JSON.stringify(value))
    else sessionStorage.removeItem(RELOAD_GUARD_KEY)
  } catch {
    // ignore — see readGuard
  }
}

// `navigate` is injected so the router-error spec can assert the target without a real
// document navigation (jsdom can't perform one); production uses the default. It's a
// hard, full-document load — NOT router.push — for the reason in the module comment.
export function reloadOnChunkError(
  router: Router,
  navigate: (path: string) => void = (path) => window.location.assign(path),
): void {
  router.onError((error, to) => {
    if (!isChunkLoadError(error)) return

    // A failed navigation never commits, so the address bar still shows the previous
    // route — navigate to the INTENDED path, not just reload the current one.
    const target = to?.fullPath
    if (!target) return

    const now = Date.now()
    const guard = readGuard()
    if (
      guard.path === target &&
      typeof guard.at === 'number' &&
      now - guard.at < RELOAD_GUARD_WINDOW_MS
    ) {
      // We already hard-navigated here and it failed the same way — a broken deploy,
      // not a stale one. Clear the guard (so a later good deploy isn't blocked) and
      // stop, rather than reloading in a loop.
      writeGuard(null)
      return
    }

    writeGuard({ path: target, at: now })
    navigate(target)
  })
}
