import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createMemoryHistory, createRouter, type NavigationGuard } from 'vue-router'

// The guard only reads `isAuthenticated` / `restoreRecoverable` and calls `tryRestore()`;
// stub the store with a controllable object. Hoisted so the (hoisted) vi.mock factory can
// close over it.
const { mockAuth } = vi.hoisted(() => ({
  mockAuth: {
    isAuthenticated: false,
    restoreRecoverable: false,
    prepareForRegistrationCompletion: vi.fn<() => void>(),
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
        path: '/complete-registration',
        name: 'complete-registration',
        component: { template: '<div />' },
      },
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
  mockAuth.restoreRecoverable = false
  mockAuth.prepareForRegistrationCompletion.mockReset()
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

  it('never starts restore on an initial registration-completion navigation', async () => {
    mockAuth.tryRestore.mockReturnValue(new Promise<boolean>(() => {}))
    const router = makeRouter(await freshGuard())

    await router.push('/complete-registration?token=secret')
    // useSecretToken scrubs the credential with a same-route replace. That must not
    // begin a second preparation epoch that could invalidate a nearby form submit.
    await router.replace('/complete-registration')

    expect(router.currentRoute.value.name).toBe('complete-registration')
    expect(mockAuth.tryRestore).not.toHaveBeenCalled()
    expect(mockAuth.prepareForRegistrationCompletion).toHaveBeenCalledTimes(1)
  })

  it('does not wait for an older pending background restore before completion', async () => {
    mockAuth.tryRestore.mockReturnValue(new Promise<boolean>(() => {}))
    const router = makeRouter(await freshGuard())
    await router.push('/cards')

    await router.push('/complete-registration?token=secret')

    expect(router.currentRoute.value.name).toBe('complete-registration')
    expect(mockAuth.tryRestore).toHaveBeenCalledTimes(1)
    expect(mockAuth.prepareForRegistrationCompletion).toHaveBeenCalledTimes(1)
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

  it('retries the restore after a TRANSIENT failure, at most once per cooldown', async () => {
    // A boot-time network blip must not pin the SPA to signed-out for the whole
    // session: the cookie may still be valid, so a later guarded navigation
    // re-attempts the restore instead of reusing the failed one — throttled, so
    // a redirect chain doesn't fire a refresh POST per hop.
    vi.useFakeTimers()
    try {
      mockAuth.isAuthenticated = false
      mockAuth.restoreRecoverable = true
      mockAuth.tryRestore.mockResolvedValue(false)
      const router = makeRouter(await freshGuard())

      // /profile re-arms the failed restore; the /login redirect consumes the
      // retry; the cooldown then holds.
      await router.push('/profile')
      expect(router.currentRoute.value.name).toBe('login')
      expect(mockAuth.tryRestore).toHaveBeenCalledTimes(2)

      // Within the cooldown: the settled failure is reused, no new attempt.
      await router.push('/profile')
      expect(mockAuth.tryRestore).toHaveBeenCalledTimes(2)

      // Past the cooldown: the next guarded navigation retries again.
      vi.setSystemTime(Date.now() + 6_000)
      await router.push('/profile')
      expect(mockAuth.tryRestore).toHaveBeenCalledTimes(3)
    } finally {
      vi.useRealTimers()
    }
  })

  it('keeps the settled restore after a DEFINITIVE failure (no refresh per navigation)', async () => {
    // A hard 401 cleared the cookie: signed-out visitors must not pay a refresh
    // POST on every guarded navigation.
    mockAuth.isAuthenticated = false
    mockAuth.restoreRecoverable = false
    mockAuth.tryRestore.mockResolvedValue(false)
    const router = makeRouter(await freshGuard())

    await router.push('/profile')
    await router.push('/profile')

    expect(mockAuth.tryRestore).toHaveBeenCalledTimes(1)
  })
})
