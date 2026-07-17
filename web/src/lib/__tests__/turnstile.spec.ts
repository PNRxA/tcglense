import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

beforeEach(() => {
  vi.resetModules()
  document.head.querySelectorAll('[data-tcglense-turnstile]').forEach((el) => el.remove())
  delete window.turnstile
  delete window.__turnstileOnload
})

afterEach(() => {
  document.head.querySelectorAll('[data-tcglense-turnstile]').forEach((el) => el.remove())
  delete window.turnstile
  delete window.__turnstileOnload
})

describe('loadTurnstile', () => {
  it('drops a failed loader so the next call injects a fresh script', async () => {
    const { loadTurnstile } = await import('../turnstile')

    const first = loadTurnstile()
    const firstScript = document.head.querySelector<HTMLScriptElement>('[data-tcglense-turnstile]')
    expect(firstScript).not.toBeNull()
    queueMicrotask(() => firstScript?.dispatchEvent(new Event('error')))
    await expect(first).rejects.toThrow('failed to load Turnstile')
    expect(firstScript?.isConnected).toBe(false)

    const second = loadTurnstile()
    const secondScript = document.head.querySelector<HTMLScriptElement>('[data-tcglense-turnstile]')
    expect(secondScript).not.toBeNull()
    expect(secondScript).not.toBe(firstScript)

    queueMicrotask(() => secondScript?.dispatchEvent(new Event('error')))
    await expect(second).rejects.toThrow('failed to load Turnstile')
  })
})
