import { describe, expect, it, vi } from 'vitest'

// Importing the real router constructs it (and eager-imports HomeView) and registers the
// auth guard. Stub HomeView so the import stays light, and the auth store so the
// module-level guard registration has something to resolve if ever invoked. `resolve()`
// is synchronous and does NOT run navigation guards, so these assertions probe only the
// route table, never the guard.
vi.mock('@/views/HomeView.vue', () => ({ default: { template: '<div />' } }))
vi.mock('@/stores/auth', () => ({ useAuthStore: () => ({ isAuthenticated: false }) }))

describe('router catch-all (404)', () => {
  it('resolves an unrouted path to the not-found route', async () => {
    const router = (await import('@/router')).default
    const resolved = router.resolve('/definitely/not/a/real/path')
    expect(resolved.name).toBe('not-found')
  })

  it('still resolves a real path to its own route, not the catch-all', async () => {
    const router = (await import('@/router')).default
    expect(router.resolve('/cards').name).toBe('cards')
    expect(router.resolve('/').name).toBe('home')
    // A known prefix with an unknown tail is a real parametric route (the view handles
    // the empty result), not a 404 — the catch-all only claims fully unrouted paths.
    expect(router.resolve('/cards/mtg/sets/zzz').name).toBe('set')
  })
})
