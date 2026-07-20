import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import { reloadOnChunkError } from '@/router/reloadOnChunkError'

// A memory-history router whose one lazy route rejects with a controllable error, so we
// can drive the real `router.onError` path exactly as a failed chunk import would. The
// hard navigation is captured by an injected spy (jsdom can't perform a real one).
function makeRouter(loaderError: () => unknown): {
  router: Router
  navigate: ReturnType<typeof vi.fn>
} {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', name: 'home', component: { template: '<div />' } },
      {
        path: '/lazy',
        name: 'lazy',
        component: () => Promise.reject(loaderError()),
      },
    ],
  })
  const navigate = vi.fn<(path: string) => void>()
  reloadOnChunkError(router, navigate)
  return { router, navigate }
}

// The message a failed dynamic import throws in Chromium.
const chunkError = () =>
  new Error('Failed to fetch dynamically imported module: /assets/Lazy-abc123.js')

beforeEach(() => {
  sessionStorage.clear()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('reloadOnChunkError', () => {
  it('hard-navigates to the intended URL when a route chunk fails to load', async () => {
    const { router, navigate } = makeRouter(chunkError)

    await router.push('/lazy?tab=x').catch(() => {})

    // The intended full path — not the current (previous) route, since a failed
    // navigation never commits and leaves the address bar on the old URL.
    expect(navigate).toHaveBeenCalledExactlyOnceWith('/lazy?tab=x')
  })

  it('recognises the Firefox, Safari, and Vite preload messages too', async () => {
    for (const message of [
      'error loading dynamically imported module',
      'Importing a module script failed.',
      'Unable to preload CSS for /assets/Lazy-abc123.css',
    ]) {
      sessionStorage.clear()
      const { router, navigate } = makeRouter(() => new Error(message))
      await router.push('/lazy').catch(() => {})
      expect(navigate).toHaveBeenCalledWith('/lazy')
    }
  })

  it('ignores a non-chunk navigation error (no reload)', async () => {
    const { router, navigate } = makeRouter(() => new Error('boom: something unrelated'))

    await router.push('/lazy').catch(() => {})

    expect(navigate).not.toHaveBeenCalled()
  })

  it('does not loop: a second immediate failure for the same URL is not reloaded again', async () => {
    const { router, navigate } = makeRouter(chunkError)

    // First failure hard-navigates; the (simulated) fresh page fails the same way, which
    // means a broken deploy rather than a stale one — the second attempt must NOT reload.
    await router.push('/lazy').catch(() => {})
    await router.push('/lazy').catch(() => {})

    expect(navigate).toHaveBeenCalledTimes(1)
  })

  it('reloads again once the loop-guard window has elapsed', async () => {
    vi.useFakeTimers()
    const { router, navigate } = makeRouter(chunkError)

    await router.push('/lazy').catch(() => {})
    expect(navigate).toHaveBeenCalledTimes(1)

    // Past the 10s guard window a repeat failure is treated as a fresh stale-chunk event.
    vi.setSystemTime(Date.now() + 11_000)
    await router.push('/lazy').catch(() => {})
    expect(navigate).toHaveBeenCalledTimes(2)
  })
})
