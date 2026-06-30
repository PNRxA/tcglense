import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

// Mock the API client but keep the real `ApiError` class so `instanceof` checks in
// the store behave exactly as in production.
vi.mock('@/lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api')>()
  return {
    ...actual,
    login: vi.fn<typeof actual.login>(),
    register: vi.fn<typeof actual.register>(),
    logout: vi.fn<typeof actual.logout>(),
    me: vi.fn<typeof actual.me>(),
    refresh: vi.fn<typeof actual.refresh>(),
  }
})

import { ApiError, login, logout, me, refresh } from '@/lib/api'
import type { User } from '@/lib/api'
import { useAuthStore } from '@/stores/auth'

const USER: User = {
  id: 1,
  email: 'ash@pallet.town',
  display_name: 'Ash',
  created_at: '2026-01-01T00:00:00Z',
}

beforeEach(() => {
  setActivePinia(createPinia())
  vi.mocked(login).mockReset()
  vi.mocked(logout).mockReset()
  vi.mocked(me).mockReset()
  vi.mocked(refresh).mockReset()
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
    let resolveRefresh!: (value: { access_token: string }) => void
    vi.mocked(refresh).mockReturnValue(
      new Promise((resolve) => {
        resolveRefresh = resolve
      }),
    )

    const store = useAuthStore()
    const first = store.refresh()
    const second = store.refresh()

    // Both callers share one in-flight request — the rotating cookie is submitted
    // exactly once (a parallel submit would look like reuse and burn the session).
    expect(vi.mocked(refresh)).toHaveBeenCalledTimes(1)

    resolveRefresh({ access_token: 'rotated' })
    expect(await first).toBe(true)
    expect(await second).toBe(true)
    expect(store.accessToken).toBe('rotated')
  })

  it('clears session state when a refresh fails', async () => {
    vi.mocked(refresh).mockRejectedValue(new ApiError('invalid refresh token', 401))

    const store = useAuthStore()
    store.accessToken = 'stale'
    store.user = USER

    expect(await store.refresh()).toBe(false)
    expect(store.accessToken).toBeNull()
    expect(store.user).toBeNull()
  })
})

describe('auth store: authFetch refresh-and-retry', () => {
  it('refreshes once on a 401 and retries the call', async () => {
    vi.mocked(refresh).mockResolvedValue({ access_token: 'fresh' })

    const store = useAuthStore()
    store.accessToken = 'stale'

    const call = vi
      .fn<(token: string) => Promise<string>>()
      .mockRejectedValueOnce(new ApiError('expired', 401))
      .mockResolvedValueOnce('ok')

    await expect(store.authFetch(call)).resolves.toBe('ok')
    expect(call).toHaveBeenCalledTimes(2)
    expect(vi.mocked(refresh)).toHaveBeenCalledTimes(1)
  })

  it('logs out when the retry still returns 401', async () => {
    vi.mocked(refresh).mockResolvedValue({ access_token: 'fresh' })
    vi.mocked(logout).mockResolvedValue(undefined)

    const store = useAuthStore()
    store.accessToken = 'stale'
    store.user = USER

    const call = vi
      .fn<(token: string) => Promise<string>>()
      .mockRejectedValue(new ApiError('expired', 401))

    await expect(store.authFetch(call)).rejects.toMatchObject({ status: 401 })
    // Persistent 401 after a fresh token means the session is dead -> log out.
    expect(vi.mocked(logout)).toHaveBeenCalledTimes(1)
    expect(store.accessToken).toBeNull()
    expect(store.user).toBeNull()
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
})
