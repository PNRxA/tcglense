import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { listCards } from '../api'
import { ApiError, request } from '../api/client'

// Build a minimal `fetch` Response stand-in (only what `request()` reads).
function fakeResponse(status: number, body: unknown) {
  const text = typeof body === 'string' ? body : JSON.stringify(body)
  return {
    ok: status >= 200 && status < 300,
    status,
    text: () => Promise.resolve(text),
  } as Response
}

// A `fetch` that never resolves on its own — it only rejects (with the same
// `AbortError` a real fetch throws) once its signal aborts, so tests can drive the
// timeout / caller-cancellation paths deterministically.
function abortableFetch(init?: RequestInit): Promise<Response> {
  return new Promise((_resolve, reject) => {
    const signal = init?.signal
    const reject_ = () => reject(new DOMException('The operation was aborted.', 'AbortError'))
    if (signal) {
      if (signal.aborted) reject_()
      else signal.addEventListener('abort', reject_)
    }
  })
}

const fetchMock = vi.fn<typeof fetch>()

beforeEach(() => {
  vi.stubGlobal('fetch', fetchMock)
  fetchMock.mockReset()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

function lastInit(): RequestInit {
  const { calls } = fetchMock.mock
  const call = calls[calls.length - 1]
  if (!call) throw new Error('fetch was not called')
  return (call[1] ?? {}) as RequestInit
}

describe('api client: signal + timeout', () => {
  it('passes a composed signal (not the caller signal) to fetch on a GET', async () => {
    fetchMock.mockResolvedValueOnce(fakeResponse(200, { data: [] }))
    const caller = new AbortController()
    await listCards('mtg', undefined, caller.signal)

    const signal = lastInit().signal
    expect(signal).toBeInstanceOf(AbortSignal)
    // The client wraps the caller's signal in its own controller (for the timeout).
    expect(signal).not.toBe(caller.signal)
  })

  it('re-throws the original AbortError on caller abort (no ApiError conversion)', async () => {
    fetchMock.mockImplementationOnce((_url, init) => abortableFetch(init))
    const caller = new AbortController()
    const p = listCards('mtg', undefined, caller.signal)
    caller.abort()

    const err = await p.catch((e) => e)
    expect(err).toBeInstanceOf(DOMException)
    expect(err.name).toBe('AbortError')
    expect(err).not.toBeInstanceOf(ApiError)
  })

  it('surfaces a GET timeout as a non-retryable ApiError 408', async () => {
    vi.useFakeTimers()
    try {
      fetchMock.mockImplementationOnce((_url, init) => abortableFetch(init))
      const p = listCards('mtg').catch((e) => e)
      await vi.advanceTimersByTimeAsync(60_000)
      const err = await p
      expect(err).toBeInstanceOf(ApiError)
      expect(err).toMatchObject({ message: 'Request timed out', status: 408 })
    } finally {
      vi.useRealTimers()
    }
  })

  it('gives a non-GET request no timeout signal by default', async () => {
    fetchMock.mockResolvedValueOnce(fakeResponse(204, ''))
    await request('/api/thing', { method: 'POST', body: { a: 1 } })

    // No caller signal + non-GET => no unilateral timeout, so nothing is passed.
    expect(lastInit().signal).toBeUndefined()
  })

  it('honors a caller signal on a non-GET without adding a timeout', async () => {
    fetchMock.mockResolvedValueOnce(fakeResponse(204, ''))
    const caller = new AbortController()
    await request('/api/thing', { method: 'POST', signal: caller.signal })

    // Passed through unwrapped (no timeout controller in front of it).
    expect(lastInit().signal).toBe(caller.signal)
  })

  it('supports an opt-in deadline for a non-GET request', async () => {
    vi.useFakeTimers()
    try {
      fetchMock.mockImplementationOnce((_url, init) => abortableFetch(init))
      const p = request('/api/auth/refresh', {
        method: 'POST',
        timeoutMs: 15_000,
      }).catch((e) => e)

      expect(lastInit().signal).toBeInstanceOf(AbortSignal)
      await vi.advanceTimersByTimeAsync(15_000)

      await expect(p).resolves.toMatchObject({
        name: 'ApiError',
        message: 'Request timed out',
        status: 408,
      })
    } finally {
      vi.useRealTimers()
    }
  })
})
