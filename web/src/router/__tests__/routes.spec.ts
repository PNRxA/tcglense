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

  it('routes the play hub and a room code apart, statics first', async () => {
    const router = (await import('@/router')).default
    // The hub's own path must not be swallowed by the `:code` sibling — a static segment
    // outranks a param at the same depth, and this pins that ordering stays that way.
    expect(router.resolve('/tools/mtg/play').name).toBe('play-hub')
    expect(router.resolve('/tools/mtg/play/ABC234').name).toBe('play-room')
    // Neither page requires an account: a guest with an invite link is the point.
    const room = router.resolve('/tools/mtg/play/ABC234')
    expect(room.matched[room.matched.length - 1]?.meta.requiresAuth).toBeUndefined()
  })

  it('keeps the card scanner on an authenticated route', async () => {
    const router = (await import('@/router')).default
    const resolved = router.resolve('/scan')

    expect(resolved.name).toBe('scan')
    expect(resolved.matched[resolved.matched.length - 1]?.meta.requiresAuth).toBe(true)
  })
})
