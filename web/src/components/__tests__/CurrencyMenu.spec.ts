import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createPinia, setActivePinia } from 'pinia'
import CurrencyMenu from '../CurrencyMenu.vue'
import { useAuthStore } from '@/stores/auth'

vi.mock('@/lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api')>()
  return {
    ...actual,
    getCurrencyRates: vi.fn<typeof actual.getCurrencyRates>().mockResolvedValue({
      base: 'USD',
      as_of: '2026-07-15',
      rates: { USD: 1, AUD: 1.52, CAD: 1.37, EUR: 0.86, GBP: 0.75, JPY: 158.4, NZD: 1.66 },
    }),
  }
})

function mountMenu(signedIn: boolean) {
  const pinia = createPinia()
  setActivePinia(pinia)
  const auth = useAuthStore()
  if (signedIn) {
    auth.accessToken = 'access-token'
    auth.user = {
      id: 1,
      email: 'currency@example.com',
      created_at: '2026-07-15T00:00:00Z',
      username: null,
      discriminator: null,
      handle: null,
      currency: 'USD',
    }
  }
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const wrapper = mount(CurrencyMenu, {
    attachTo: document.body,
    global: { plugins: [pinia, [VueQueryPlugin, { queryClient }]] },
  })
  return wrapper
}

describe('CurrencyMenu', () => {
  beforeEach(() => localStorage.clear())

  it('is available signed out and signed in, and names the active currency accessibly', () => {
    const signedOut = mountMenu(false)
    expect(signedOut.get('button').text()).toContain('Display currency: USD')
    signedOut.unmount()

    const signedIn = mountMenu(true)
    expect(signedIn.get('button').text()).toContain('Display currency: USD')
    signedIn.unmount()
  })

  it('offers every supported currency from the header', async () => {
    const wrapper = mountMenu(true)
    await wrapper.get('button').trigger('click')
    await flushPromises()

    const menuText = document.body.textContent ?? ''
    expect(menuText).toContain('Australian Dollar')
    expect(menuText).toContain('Canadian Dollar')
    expect(menuText).toContain('Euro')
    expect(menuText).toContain('British Pound')
    expect(menuText).toContain('Japanese Yen')
    expect(menuText).toContain('New Zealand Dollar')
    wrapper.unmount()
  })

  it('persists a signed-out selection locally and applies it immediately', async () => {
    const wrapper = mountMenu(false)
    await wrapper.get('button').trigger('click')
    await flushPromises()

    const audOption = Array.from(
      document.body.querySelectorAll<HTMLElement>('[role="menuitemradio"]'),
    ).find((item) => item.textContent?.includes('Australian Dollar'))
    expect(audOption).toBeDefined()
    audOption!.click()
    await flushPromises()

    expect(wrapper.get('button').text()).toContain('Display currency: AUD')
    expect(localStorage.getItem('tcglense_currency')).toBe('AUD')
    wrapper.unmount()
  })
})
