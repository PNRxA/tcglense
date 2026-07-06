import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createMemoryHistory, createRouter, type NavigationGuard } from 'vue-router'

// The guard only reads `isAuthenticated` and calls `tryRestore()`; stub the store with a
// controllable object. Hoisted so the (hoisted) vi.mock factory can close over it.
const { mockAuth } = vi.hoisted(() => ({
  mockAuth: {
    isAuthenticated: false,
    tryRestore: vi.fn<() => Promise<boolean>>(),
  },
}))

vi.mock('@/stores/auth', () => ({ useAuthStore: () => mockAuth }))
// HomeView is the router's one eager import; stub it so importing the router stays light.
vi.mock('@/views/HomeView.vue', () => ({ default: { template: '<div />' } }))

function makeRouter(guard: NavigationGuard) {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', name: 'home', component: { template: '<div />' } },
      { path: '/cards', name: 'cards', component: { template: '<div />' } },
      {
        path: '/login',
        name: 'login',
        component: { template: '<div />' },
        meta: { requiresGuest: true },
      },
      {
        path: '/profile',
        name: 'profile',
        component: { template: '<div />' },
        meta: { requiresAuth: true },
      },
    ],
  })
  router.beforeEach(guard)
  return router
}

// Fresh module (hence a fresh one-shot restore-promise cache) per test.
async function freshGuard(): Promise<NavigationGuard> {
  vi.resetModules()
  return (await import('@/router')).authGuard
}

beforeEach(() => {
  mockAuth.isAuthenticated = false
  mockAuth.tryRestore.mockReset()
  mockAuth.tryRestore.mockResolvedValue(false)
})

describe('router auth guard', () => {
  it('resolves a public navigation even while tryRestore never settles', async () => {
    // A public route must paint immediately: the guard kicks off restore but does NOT
    // block on it, so a never-resolving restore can't stall the navigation.
    mockAuth.tryRestore.mockReturnValue(new Promise<boolean>(() => {}))
    const router = makeRouter(await freshGuard())

    await router.push('/cards')

    expect(router.currentRoute.value.name).toBe('cards')
    expect(mockAuth.tryRestore).toHaveBeenCalledTimes(1)
  })

  it('awaits restore on a requiresAuth route and redirects to /login when signed out', async () => {
    mockAuth.isAuthenticated = false
    mockAuth.tryRestore.mockResolvedValue(false)
    const router = makeRouter(await freshGuard())

    await router.push('/profile')

    expect(router.currentRoute.value.name).toBe('login')
    expect(router.currentRoute.value.query.redirect).toBe('/profile')
  })

  it('awaits restore on a requiresGuest route and bounces a signed-in user home', async () => {
    mockAuth.isAuthenticated = true
    mockAuth.tryRestore.mockResolvedValue(true)
    const router = makeRouter(await freshGuard())

    await router.push('/login')

    expect(router.currentRoute.value.path).toBe('/')
  })
})
