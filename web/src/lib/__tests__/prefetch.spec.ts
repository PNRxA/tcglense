import { describe, it, expect, vi, afterEach } from 'vitest'

import { flushPromises } from '@vue/test-utils'
import type { Router } from 'vue-router'
import { prefetchRouteChunks, scheduleIdleWarm } from '../prefetch'

// A minimal stand-in for the parts of Router these helpers touch (resolve → matched
// records). Component tests exercise the real memory router; here we drive the branches
// directly.
function fakeRouter(matched: unknown[]): Router {
  return { resolve: () => ({ matched }) } as unknown as Router
}

afterEach(() => {
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

describe('prefetchRouteChunks', () => {
  it('invokes each lazy route component factory once', () => {
    const loader = vi.fn<() => Promise<object>>(() => Promise.resolve({}))
    prefetchRouteChunks(fakeRouter([{ components: { default: loader } }]), '/x')
    expect(loader).toHaveBeenCalledTimes(1)
  })

  it('skips eager component objects', () => {
    const eager = { template: '<div />' }
    expect(() =>
      prefetchRouteChunks(fakeRouter([{ components: { default: eager } }]), '/x'),
    ).not.toThrow()
  })

  it('is a no-op when the location cannot be resolved', () => {
    const router = {
      resolve: () => {
        throw new Error('no such route')
      },
    } as unknown as Router
    expect(() => prefetchRouteChunks(router, '/nope')).not.toThrow()
  })

  it('swallows a rejected chunk import', async () => {
    const loader = vi.fn<() => Promise<never>>(() => Promise.reject(new Error('chunk load failed')))
    expect(() =>
      prefetchRouteChunks(fakeRouter([{ components: { default: loader } }]), '/x'),
    ).not.toThrow()
    await flushPromises()
  })
})

describe('scheduleIdleWarm', () => {
  it('warms every location and extra loader via requestIdleCallback', () => {
    const ric = vi.fn<(cb: () => void) => void>((cb) => cb())
    vi.stubGlobal('requestIdleCallback', ric)
    const loader = vi.fn<() => Promise<object>>(() => Promise.resolve({}))
    const extra = vi.fn<() => Promise<object>>(() => Promise.resolve({}))
    scheduleIdleWarm(fakeRouter([{ components: { default: loader } }]), ['/a', '/b'], [extra])
    expect(ric).toHaveBeenCalledTimes(1)
    expect(loader).toHaveBeenCalledTimes(2)
    expect(extra).toHaveBeenCalledTimes(1)
  })

  it('falls back to a setTimeout when requestIdleCallback is unavailable', () => {
    vi.stubGlobal('requestIdleCallback', undefined)
    vi.useFakeTimers()
    const extra = vi.fn<() => Promise<object>>(() => Promise.resolve({}))
    scheduleIdleWarm(fakeRouter([]), [], [extra])
    expect(extra).not.toHaveBeenCalled()
    vi.advanceTimersByTime(2000)
    expect(extra).toHaveBeenCalledTimes(1)
  })
})
