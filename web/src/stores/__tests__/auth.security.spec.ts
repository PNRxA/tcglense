import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

// Mock the API client but keep the real `ApiError` class so `instanceof` checks in
// the store behave exactly as in production.
vi.mock('@/lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api')>()
  return {
    ...actual,
    completeRegistration: vi.fn<typeof actual.completeRegistration>(),
    login: vi.fn<typeof actual.login>(),
    register: vi.fn<typeof actual.register>(),
    logout: vi.fn<typeof actual.logout>(),
    refresh: vi.fn<typeof actual.refresh>(),
    resetPassword: vi.fn<typeof actual.resetPassword>(),
  }
})

import { ApiError, completeRegistration, login, logout, refresh, resetPassword } from '@/lib/api'
import type { User } from '@/lib/api'
import { useAuthStore } from '@/stores/auth'

const USER: User = {
  id: 1,
  email: 'ash@pallet.town',
  created_at: '2026-01-01T00:00:00Z',
  username: null,
  discriminator: null,
  handle: null,
}

const OTHER_USER: User = {
  ...USER,
  id: 2,
  email: 'misty@cerulean.gym',
}

beforeEach(() => {
  setActivePinia(createPinia())
  vi.mocked(completeRegistration).mockReset()
  vi.mocked(login).mockReset()
  vi.mocked(logout).mockReset()
  vi.mocked(refresh).mockReset()
  vi.mocked(resetPassword).mockReset()
})

describe('auth store: token is never persisted to web storage', () => {
  it('keeps the access token in memory only on login', async () => {
    const setItem = vi.spyOn(Storage.prototype, 'setItem')
    vi.mocked(login).mockResolvedValue({ access_token: 'secret-access-token', user: USER })

    const store = useAuthStore()
    await store.login({ email: USER.email, password: 'password123' })

    expect(store.accessToken).toBe('secret-access-token')
    // The long-lived secret must not be written anywhere an XSS could read it.
    const leaked = setItem.mock.calls.some(
      ([, value]) => typeof value === 'string' && value.includes('secret-access-token'),
    )
    expect(leaked).toBe(false)
    setItem.mockRestore()
  })
})

describe('auth store: single-flight refresh', () => {
  it('coalesces concurrent refreshes into one rotation of the single-use cookie', async () => {
    let resolveRefresh!: (value: { access_token: string; user: User }) => void
    vi.mocked(refresh).mockReturnValue(
      new Promise((resolve) => {
        resolveRefresh = resolve
      }),
    )

    const store = useAuthStore()
    const first = store.refresh()
    const second = store.refresh()
    await Promise.resolve()

    // Both callers share one in-flight request — the rotating cookie is submitted
    // exactly once (a parallel submit would look like reuse and burn the session).
    expect(vi.mocked(refresh)).toHaveBeenCalledTimes(1)

    resolveRefresh({ access_token: 'rotated', user: USER })
    expect(await first).toBe(true)
    expect(await second).toBe(true)
    expect(store.accessToken).toBe('rotated')
    expect(store.user).toEqual(USER)
  })

  it('replaces the cached identity together with the token after a cross-tab account switch', async () => {
    vi.mocked(refresh).mockResolvedValue({ access_token: 'account-b-token', user: OTHER_USER })

    const store = useAuthStore()
    store.accessToken = 'account-a-token'
    store.user = USER

    expect(await store.refresh()).toBe(true)
    expect(store.accessToken).toBe('account-b-token')
    expect(store.user).toEqual(OTHER_USER)
  })

  it('clears session state only after the 401 retry also fails', async () => {
    vi.useFakeTimers()
    try {
      vi.mocked(refresh).mockRejectedValue(new ApiError('invalid refresh token', 401))

      const store = useAuthStore()
      store.accessToken = 'stale'
      store.user = USER

      const result = store.refresh()
      await vi.advanceTimersByTimeAsync(500)
      expect(await result).toBe(false)

      // One delayed retry (the benign loser-of-a-rotation case), then the
      // session is definitively dead and local state clears.
      expect(vi.mocked(refresh)).toHaveBeenCalledTimes(2)
      expect(store.accessToken).toBeNull()
      expect(store.user).toBeNull()
      expect(store.restoreRecoverable).toBe(false)
    } finally {
      vi.useRealTimers()
    }
  })

  it('recovers when the 401 was the benign loser of a concurrent rotation', async () => {
    vi.useFakeTimers()
    try {
      // The losing tab of a concurrent double-submit gets a 401 while the
      // winner's rotated cookie lands in the shared jar; the delayed retry
      // must pick that cookie up instead of signing the tab out.
      vi.mocked(refresh)
        .mockRejectedValueOnce(new ApiError('refresh token superseded', 401))
        .mockResolvedValueOnce({ access_token: 'winner-cookie-token', user: USER })

      const store = useAuthStore()
      store.accessToken = 'stale'
      store.user = USER

      const result = store.refresh()
      await vi.advanceTimersByTimeAsync(500)
      expect(await result).toBe(true)

      expect(store.accessToken).toBe('winner-cookie-token')
      expect(store.user).toEqual(USER)
      expect(store.restoreRecoverable).toBe(false)
    } finally {
      vi.useRealTimers()
    }
  })

  it('stays recoverable when the winner cookie has not landed before the 401 retry', async () => {
    vi.useFakeTimers()
    try {
      // Responses on separate tab connections are not ordered: even after the
      // delay, the retry can capture the old cookie before the winner's
      // Set-Cookie lands and receive Superseded again.
      vi.mocked(refresh).mockRejectedValue(new ApiError('refresh token superseded', 401))

      const store = useAuthStore()
      store.accessToken = 'stale'
      store.user = USER

      const result = store.refresh()
      await vi.advanceTimersByTimeAsync(500)
      expect(await result).toBe(false)

      expect(vi.mocked(refresh)).toHaveBeenCalledTimes(2)
      expect(store.accessToken).toBe('stale')
      expect(store.user).toEqual(USER)
      expect(store.restoreRecoverable).toBe(true)
    } finally {
      vi.useRealTimers()
    }
  })

  it('keeps session state when a refresh fails transiently', async () => {
    // A 5xx / network blip says nothing about the session: the httpOnly cookie
    // is still valid server-side, so the in-memory session must survive — the
    // old catch-all clear here is what painted signed-out chrome during every
    // API hiccup ("logged out for no reason", issue #417).
    vi.mocked(refresh).mockRejectedValue(new ApiError('bad gateway', 502))

    const store = useAuthStore()
    store.accessToken = 'stale'
    store.user = USER

    expect(await store.refresh()).toBe(false)
    // No retry for non-401s (they resolve nothing about the cookie)…
    expect(vi.mocked(refresh)).toHaveBeenCalledTimes(1)
    // …and the signed-in posture survives for the next attempt to restore.
    expect(store.user).toEqual(USER)
    expect(store.isAuthenticated).toBe(true)
    expect(store.restoreRecoverable).toBe(true)
  })
})

describe('auth store: cross-tab refresh coordination', () => {
  it('rotates inside the tcglense-refresh Web Lock when the API is available', async () => {
    // Serializing refreshes across tabs is what stops two tabs submitting the same
    // single-use cookie at once (the race that logged users out). Stub the Web Locks
    // API (jsdom has none) and prove the rotation runs inside the named lock.
    const request = vi.fn<
      (name: string, opts: unknown, fn: () => Promise<unknown>) => Promise<unknown>
    >((_name, _opts, fn) => fn())
    Object.defineProperty(navigator, 'locks', { value: { request }, configurable: true })
    try {
      vi.mocked(refresh).mockResolvedValue({ access_token: 'locked-fresh', user: USER })

      const store = useAuthStore()
      expect(await store.refresh()).toBe(true)

      expect(store.accessToken).toBe('locked-fresh')
      expect(request).toHaveBeenCalledTimes(1)
      expect(request).toHaveBeenCalledWith(
        'tcglense-refresh',
        expect.anything(),
        expect.any(Function),
      )
    } finally {
      Reflect.deleteProperty(navigator, 'locks')
    }
  })

  it('falls open to an unsynchronized refresh when Web Locks is unavailable', async () => {
    // Older browsers (Safari < 15.4) expose no navigator.locks; refresh must still
    // work — the server-side fix keeps an unsynchronized refresh race-safe.
    expect('locks' in navigator).toBe(false)
    vi.mocked(refresh).mockResolvedValue({ access_token: 'unlocked-fresh', user: USER })

    const store = useAuthStore()
    expect(await store.refresh()).toBe(true)
    expect(store.accessToken).toBe('unlocked-fresh')
  })
})

describe('auth store: authFetch refresh-and-retry', () => {
  it('refreshes once on a 401 and retries the call for the same account', async () => {
    vi.mocked(refresh).mockResolvedValue({ access_token: 'fresh', user: USER })

    const store = useAuthStore()
    store.accessToken = 'stale'
    store.user = USER

    const call = vi
      .fn<(token: string) => Promise<string>>()
      .mockRejectedValueOnce(new ApiError('expired', 401))
      .mockResolvedValueOnce('ok')

    await expect(store.authFetch(call)).resolves.toBe('ok')
    expect(call).toHaveBeenCalledTimes(2)
    expect(vi.mocked(refresh)).toHaveBeenCalledTimes(1)
    expect(store.user).toEqual(USER)
  })

  it('never replays an operation when refresh switches to another account', async () => {
    vi.mocked(refresh).mockResolvedValue({ access_token: 'account-b-token', user: OTHER_USER })

    const store = useAuthStore()
    store.accessToken = 'account-a-token'
    store.user = USER

    const call = vi
      .fn<(token: string) => Promise<string>>()
      .mockRejectedValueOnce(new ApiError('expired', 401))
      .mockResolvedValueOnce('must-not-run')

    await expect(store.authFetch(call)).rejects.toMatchObject({ status: 401 })
    expect(call).toHaveBeenCalledTimes(1)
    expect(store.accessToken).toBe('account-b-token')
    expect(store.user).toEqual(OTHER_USER)
  })

  it('clears local state when the retry still 401s — WITHOUT revoking the cookie', async () => {
    vi.mocked(refresh).mockResolvedValue({ access_token: 'fresh', user: USER })
    vi.mocked(logout).mockResolvedValue(undefined)

    const store = useAuthStore()
    store.accessToken = 'stale'
    store.user = USER

    const call = vi
      .fn<(token: string) => Promise<string>>()
      .mockRejectedValue(new ApiError('expired', 401))

    await expect(store.authFetch(call)).rejects.toMatchObject({ status: 401 })
    // Persistent 401 after a fresh token: drop the in-memory session…
    expect(store.accessToken).toBeNull()
    expect(store.user).toBeNull()
    // …but never POST /api/auth/logout here: that would revoke the httpOnly
    // refresh cookie over what may be an infra-injected 401 (proxy/WAF blip,
    // mid-deploy mismatch), turning it into a permanent browser-wide logout.
    // A genuinely dead session gets its cookie cleared by the server on the
    // next refresh 401 anyway.
    expect(vi.mocked(logout)).not.toHaveBeenCalled()
    // The refresh succeeded, so the cookie is valid and the session is
    // recoverable: the router guard re-attempts a restore instead of stranding
    // the user signed-out until a manual reload.
    expect(store.restoreRecoverable).toBe(true)
  })

  it('does not refresh on non-401 errors', async () => {
    const store = useAuthStore()
    store.accessToken = 'valid'

    const call = vi
      .fn<(token: string) => Promise<string>>()
      .mockRejectedValue(new ApiError('server error', 500))

    await expect(store.authFetch(call)).rejects.toMatchObject({ status: 500 })
    expect(vi.mocked(refresh)).not.toHaveBeenCalled()
    expect(call).toHaveBeenCalledTimes(1)
  })
})

describe('auth store: sessionResolved latch', () => {
  it('flips true once the initial restore settles', async () => {
    vi.mocked(refresh).mockResolvedValue({ access_token: 'fresh', user: USER })

    const store = useAuthStore()
    expect(store.sessionResolved).toBe(false)
    await store.tryRestore()
    expect(store.sessionResolved).toBe(true)
  })

  it('degrades to resolved via the watchdog when the restore never settles', async () => {
    vi.useFakeTimers()
    try {
      // A half-open socket: the refresh POST neither resolves nor rejects.
      vi.mocked(refresh).mockReturnValue(new Promise(() => {}))

      const store = useAuthStore()
      store.tryRestore()
      expect(store.sessionResolved).toBe(false)

      // Before the ceiling: still waiting.
      await vi.advanceTimersByTimeAsync(9_000)
      expect(store.sessionResolved).toBe(false)

      // Past the 10s ceiling: the watchdog un-gates the signed-out UI.
      await vi.advanceTimersByTimeAsync(2_000)
      expect(store.sessionResolved).toBe(true)
      expect(store.isAuthenticated).toBe(false)
    } finally {
      vi.useRealTimers()
    }
  })
})

describe('auth store: logout always clears local state', () => {
  it('clears state even when the server logout call fails', async () => {
    vi.mocked(logout).mockRejectedValue(new Error('network down'))

    const store = useAuthStore()
    store.accessToken = 'x'
    store.user = USER

    await store.logout()
    expect(store.accessToken).toBeNull()
    expect(store.user).toBeNull()
  })

  it('orders logout after an older refresh and ignores that refresh result locally', async () => {
    const events: string[] = []
    let resolveRefresh!: (value: { access_token: string; user: User }) => void
    vi.mocked(refresh).mockReturnValue(
      new Promise((resolve) => {
        resolveRefresh = (value) => {
          events.push('refresh-response')
          resolve(value)
        }
      }),
    )
    vi.mocked(logout).mockImplementation(async () => {
      events.push('logout')
    })

    const store = useAuthStore()
    store.accessToken = 'old-token'
    store.user = USER
    const refreshing = store.refresh()
    await Promise.resolve()
    const loggingOut = store.logout()

    expect(store.accessToken).toBeNull()
    expect(vi.mocked(logout)).not.toHaveBeenCalled()
    resolveRefresh({ access_token: 'late-token', user: USER })

    await expect(refreshing).resolves.toBe(false)
    await loggingOut
    expect(events).toEqual(['refresh-response', 'logout'])
    expect(store.accessToken).toBeNull()
    expect(store.user).toBeNull()
  })
})

describe('auth store: registration completion ordering', () => {
  it('waits out and ignores an older restore before adopting the completed account', async () => {
    const NEW_USER = { ...OTHER_USER, id: 3, email: 'brock@pewter.gym' }
    let resolveRefresh!: (value: { access_token: string; user: User }) => void
    let resolveCompletion!: (value: { access_token: string; user: User }) => void
    vi.mocked(refresh).mockReturnValue(
      new Promise((resolve) => {
        resolveRefresh = resolve
      }),
    )
    vi.mocked(completeRegistration).mockReturnValue(
      new Promise((resolve) => {
        resolveCompletion = resolve
      }),
    )

    const store = useAuthStore()
    const restoring = store.tryRestore()
    await Promise.resolve()
    store.prepareForRegistrationCompletion()
    const completing = store.completeRegistration({
      token: 'email-token',
      password: 'correct horse battery staple',
      username: null,
    })

    expect(store.sessionResolved).toBe(true)
    expect(vi.mocked(completeRegistration)).not.toHaveBeenCalled()
    resolveRefresh({ access_token: 'old-account-token', user: USER })

    await expect(restoring).resolves.toBe(false)
    expect(store.accessToken).toBeNull()
    expect(store.user).toBeNull()
    expect(vi.mocked(completeRegistration)).toHaveBeenCalledTimes(1)
    resolveCompletion({ access_token: 'completed-token', user: NEW_USER })
    await completing
    expect(store.accessToken).toBe('completed-token')
    expect(store.user).toEqual(NEW_USER)
  })
})

describe('auth store: password reset session cleanup', () => {
  it('removes the stale refresh cookie and clears local auth after a successful reset', async () => {
    vi.mocked(resetPassword).mockResolvedValue(undefined)
    vi.mocked(logout).mockResolvedValue(undefined)
    const store = useAuthStore()
    store.accessToken = 'old-access-token'
    store.user = USER

    await store.resetPassword({ token: 'reset-token', password: 'new secure password' })

    expect(vi.mocked(resetPassword)).toHaveBeenCalledTimes(1)
    expect(vi.mocked(logout)).toHaveBeenCalledTimes(1)
    expect(store.accessToken).toBeNull()
    expect(store.user).toBeNull()
    expect(store.sessionResolved).toBe(true)
  })
})
