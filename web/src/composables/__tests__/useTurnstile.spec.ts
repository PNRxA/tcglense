import { defineComponent, ref } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { TurnstileApi, TurnstileRenderOptions } from '@/lib/turnstile'

const mocks = vi.hoisted(() => ({
  loadTurnstile: vi.fn<() => Promise<TurnstileApi>>(),
  turnstileSiteKey: vi.fn<() => Promise<string | undefined>>(),
}))

vi.mock('@/lib/turnstile', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/lib/turnstile')>()),
  loadTurnstile: mocks.loadTurnstile,
  turnstileSiteKey: mocks.turnstileSiteKey,
}))

import { useTurnstile } from '../useTurnstile'

function mountComposable() {
  let execute!: () => Promise<string | null>
  const wrapper = mount(
    defineComponent({
      setup() {
        const container = ref<HTMLElement>(document.createElement('div'))
        execute = useTurnstile(container).execute
        return () => null
      },
    }),
  )
  return { execute: () => execute(), wrapper }
}

beforeEach(() => {
  mocks.loadTurnstile.mockReset()
  mocks.turnstileSiteKey.mockReset()
  delete window.turnstile
})

describe('useTurnstile', () => {
  it('queues concurrent calls so each receives a fresh single-use token', async () => {
    let callback: ((token: string) => void) | undefined
    const api: TurnstileApi = {
      render: vi.fn<TurnstileApi['render']>((_el: HTMLElement, options: TurnstileRenderOptions) => {
        callback = options.callback
        return 'widget-1'
      }),
      execute: vi.fn<TurnstileApi['execute']>(),
      reset: vi.fn<TurnstileApi['reset']>(),
      remove: vi.fn<TurnstileApi['remove']>(),
      getResponse: vi.fn<TurnstileApi['getResponse']>(),
    }
    window.turnstile = api
    mocks.turnstileSiteKey.mockResolvedValue('site-key')
    mocks.loadTurnstile.mockResolvedValue(api)
    const { execute, wrapper } = mountComposable()

    const first = execute()
    const second = execute()
    expect(second).not.toBe(first)

    await flushPromises()
    expect(api.execute).toHaveBeenCalledTimes(1)
    expect(api.render).toHaveBeenCalledWith(
      expect.any(HTMLElement),
      expect.objectContaining({ action: 'auth', sitekey: 'site-key' }),
    )
    callback?.('captcha-token-1')
    await expect(first).resolves.toBe('captcha-token-1')

    await flushPromises()
    expect(api.execute).toHaveBeenCalledTimes(2)
    callback?.('captcha-token-2')
    await expect(second).resolves.toBe('captcha-token-2')
    expect(api.reset).toHaveBeenCalledTimes(2)
    wrapper.unmount()
  })

  it('bounds config/script preparation as part of the whole execution', async () => {
    vi.useFakeTimers()
    try {
      mocks.turnstileSiteKey.mockReturnValue(new Promise(() => {}))
      const { execute, wrapper } = mountComposable()

      const result = execute()
      await vi.advanceTimersByTimeAsync(20_000)
      await expect(result).resolves.toBeNull()
      wrapper.unmount()
    } finally {
      vi.useRealTimers()
    }
  })
})
