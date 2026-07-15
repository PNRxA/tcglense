import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import {
  ApiError,
  getCard,
  getConfig,
  getCurrencyRates,
  login,
  logout,
  me,
  register,
  setCurrency,
} from '../api'
import { onMaintenanceDetected } from '@/lib/maintenance'

// Build a minimal `fetch` Response stand-in (only what `request()` reads).
function fakeResponse(status: number, body: unknown) {
  const text = typeof body === 'string' ? body : JSON.stringify(body)
  return {
    ok: status >= 200 && status < 300,
    status,
    text: () => Promise.resolve(text),
  } as Response
}

const fetchMock = vi.fn<typeof fetch>()

beforeEach(() => {
  vi.stubGlobal('fetch', fetchMock)
  fetchMock.mockReset()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

function lastCall() {
  const { calls } = fetchMock.mock
  const call = calls[calls.length - 1]
  if (!call) throw new Error('fetch was not called')
  return { url: call[0] as string, init: (call[1] ?? {}) as RequestInit }
}

describe('api client: credentials + headers', () => {
  it('always sends the refresh cookie (credentials: include) and JSON content-type', async () => {
    fetchMock.mockResolvedValueOnce(
      fakeResponse(200, { access_token: 't', user: { id: 1, email: 'a@b.com' } }),
    )
    await login({ email: 'a@b.com', password: 'password123' })

    const { init } = lastCall()
    expect(init.credentials).toBe('include')
    expect((init.headers as Record<string, string>)['Content-Type']).toBe('application/json')
    // No bearer header on an unauthenticated call.
    expect((init.headers as Record<string, string>).Authorization).toBeUndefined()
  })

  it('sends the bearer token only when one is supplied', async () => {
    fetchMock.mockResolvedValueOnce(fakeResponse(200, { user: { id: 1, email: 'a@b.com' } }))
    await me('access-token-123')

    const { init } = lastCall()
    expect((init.headers as Record<string, string>).Authorization).toBe('Bearer access-token-123')
    expect(init.credentials).toBe('include')
  })

  it('persists currency with an authenticated PUT', async () => {
    fetchMock.mockResolvedValueOnce(fakeResponse(200, { id: 1, currency: 'AUD' }))
    await setCurrency('access-token-123', 'AUD')

    const { url, init } = lastCall()
    expect(url).toBe('/api/auth/currency')
    expect(init.method).toBe('PUT')
    expect(init.body).toBe(JSON.stringify({ currency: 'AUD' }))
    expect((init.headers as Record<string, string>).Authorization).toBe('Bearer access-token-123')
  })

  it('loads the public currency-rate snapshot without a bearer', async () => {
    fetchMock.mockResolvedValueOnce(
      fakeResponse(200, { base: 'USD', as_of: '2026-07-14', rates: { USD: 1, AUD: 1.52 } }),
    )
    await getCurrencyRates()

    const { url, init } = lastCall()
    expect(url).toBe('/api/currencies')
    expect(init.method).toBe('GET')
    expect((init.headers as Record<string, string>).Authorization).toBeUndefined()
  })

  it('always bypasses the browser cache for runtime config', async () => {
    fetchMock.mockResolvedValueOnce(
      fakeResponse(200, {
        maintenance_mode: false,
        turnstile_site_key: null,
        signups_enabled: true,
        signups_disabled_message: null,
      }),
    )
    await getConfig()

    const { url, init } = lastCall()
    expect(url).toBe('/api/config')
    expect(init.cache).toBe('no-store')
  })

  it('percent-encodes path segments so they cannot break out of the URL', async () => {
    fetchMock.mockResolvedValueOnce(fakeResponse(200, { id: 'x' }))
    // A traversal-looking id must be encoded, not interpolated raw.
    await getCard('mtg', '../../etc/passwd')

    const { url } = lastCall()
    expect(url).toContain('%2F')
    expect(url).not.toContain('/etc/passwd')
  })
})

describe('api client: error mapping', () => {
  it('maps a non-2xx JSON body to ApiError carrying the server message + status', async () => {
    fetchMock.mockResolvedValueOnce(fakeResponse(401, { error: 'invalid email or password' }))

    await expect(login({ email: 'a@b.com', password: 'nope' })).rejects.toMatchObject({
      name: 'ApiError',
      message: 'invalid email or password',
      status: 401,
    })
  })

  it('falls back to a status-based message when the error body is not JSON', async () => {
    fetchMock.mockResolvedValueOnce(fakeResponse(502, '<html>bad gateway</html>'))

    const err = await register({ email: 'a@b.com' }).catch((e) => e)
    expect(err).toBeInstanceOf(ApiError)
    expect(err.status).toBe(502)
    // The non-JSON proxy page must not surface as the user-facing message.
    expect(err.message).not.toContain('<html>')
  })

  it('resolves an empty 204 body without throwing (logout)', async () => {
    fetchMock.mockResolvedValueOnce(fakeResponse(204, ''))
    await expect(logout()).resolves.toBeUndefined()
  })

  it('signals only machine-readable maintenance errors to the app shell', async () => {
    const detected = vi.fn<() => void>()
    const unsubscribe = onMaintenanceDetected(detected)

    fetchMock.mockResolvedValueOnce(
      fakeResponse(503, {
        error: 'service is under maintenance',
        code: 'maintenance',
      }),
    )
    await expect(getCard('mtg', 'card-id')).rejects.toMatchObject({
      status: 503,
      code: 'maintenance',
    })
    expect(detected).toHaveBeenCalledOnce()

    fetchMock.mockResolvedValueOnce(fakeResponse(503, { error: 'import queue is full' }))
    await expect(getCard('mtg', 'card-id')).rejects.toMatchObject({ status: 503 })
    expect(detected).toHaveBeenCalledOnce()
    unsubscribe()
  })
})
