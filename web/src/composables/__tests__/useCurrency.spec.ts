import { beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createPinia, setActivePinia } from 'pinia'

vi.mock('@/lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api')>()
  return {
    ...actual,
    getCurrencyRates: vi.fn<typeof actual.getCurrencyRates>(),
    setCurrency: vi.fn<typeof actual.setCurrency>(),
  }
})

import { ApiError, getCurrencyRates, setCurrency, type User } from '@/lib/api'
import { useCurrency, useSetCurrencyMutation } from '@/composables/useCurrency'
import { useAuthStore } from '@/stores/auth'

const RATES = {
  base: 'USD',
  as_of: '2026-07-15',
  rates: { USD: 1, AUD: 1.52, CAD: 1.37, EUR: 0.86, GBP: 0.75, JPY: 158.4, NZD: 1.66 },
}

function user(currency: string): User {
  return {
    id: 1,
    email: 'currency@example.com',
    created_at: '2026-07-15T00:00:00Z',
    username: null,
    discriminator: null,
    handle: null,
    currency,
  }
}

const Harness = defineComponent({
  setup() {
    const money = useCurrency()
    const mutation = useSetCurrencyMutation()
    const chooseAud = () => mutation.mutateAsync({ currency: 'AUD' })
    const chooseGuestNzd = () => money.setGuestCurrency('NZD')
    return { money, chooseAud, chooseGuestNzd }
  },
  template: `
    <div data-currency>{{ money.displayCurrency.value }}</div>
    <div data-value>{{ money.formatUsd('12.5') }}</div>
    <button type="button" @click="chooseAud">AUD</button>
    <button type="button" data-guest @click="chooseGuestNzd">guest NZD</button>
  `,
})

function mountHarness(currency: string) {
  const pinia = createPinia()
  setActivePinia(pinia)
  const auth = useAuthStore()
  auth.accessToken = 'access-token'
  auth.user = user(currency)
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const wrapper = mount(Harness, {
    global: { plugins: [pinia, [VueQueryPlugin, { queryClient }]] },
  })
  return { wrapper, auth, queryClient }
}

function mountGuestHarness() {
  const pinia = createPinia()
  setActivePinia(pinia)
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const wrapper = mount(Harness, {
    global: { plugins: [pinia, [VueQueryPlugin, { queryClient }]] },
  })
  return { wrapper, queryClient }
}

beforeEach(() => {
  localStorage.clear()
  vi.mocked(getCurrencyRates).mockReset()
  vi.mocked(setCurrency).mockReset()
})

describe('useCurrency', () => {
  it('restores and persists the signed-out device preference', async () => {
    localStorage.setItem('tcglense_currency', 'CAD')
    vi.mocked(getCurrencyRates).mockResolvedValue(RATES)
    const { wrapper } = mountGuestHarness()
    await flushPromises()

    expect(wrapper.get('[data-currency]').text()).toBe('CAD')
    await wrapper.get('[data-guest]').trigger('click')
    await flushPromises()

    expect(wrapper.get('[data-currency]').text()).toBe('NZD')
    expect(localStorage.getItem('tcglense_currency')).toBe('NZD')
  })

  it('prefers the signed-in account currency over the saved guest choice', () => {
    localStorage.setItem('tcglense_currency', 'CAD')
    const { wrapper } = mountHarness('USD')

    expect(wrapper.get('[data-currency]').text()).toBe('USD')
    expect(wrapper.get('[data-value]').text()).toBe('$12.50')
  })

  it('labels the fallback explicitly as USD while a preferred rate is loading', async () => {
    let resolveRates!: (value: typeof RATES) => void
    vi.mocked(getCurrencyRates).mockReturnValue(
      new Promise((resolve) => {
        resolveRates = resolve
      }),
    )
    const { wrapper } = mountHarness('AUD')

    expect(wrapper.get('[data-currency]').text()).toBe('USD')
    expect(wrapper.get('[data-value]').text()).toBe('USD 12.50')

    resolveRates(RATES)
    await flushPromises()
    expect(wrapper.get('[data-currency]').text()).toBe('AUD')
    expect(wrapper.get('[data-value]').text()).toContain('19')
    expect(wrapper.get('[data-value]').text()).not.toContain('USD')
  })

  it('keeps the explicit USD fallback after a cold rate failure', async () => {
    vi.mocked(getCurrencyRates).mockRejectedValue(new ApiError('temporarily unavailable', 502))
    const { wrapper } = mountHarness('NZD')
    await flushPromises()

    expect(wrapper.get('[data-currency]').text()).toBe('USD')
    expect(wrapper.get('[data-value]').text()).toBe('USD 12.50')
  })

  it('repaints mounted values after the preference mutation replaces the cached user', async () => {
    vi.mocked(getCurrencyRates).mockResolvedValue(RATES)
    vi.mocked(setCurrency).mockResolvedValue(user('AUD'))
    const { wrapper, auth } = mountHarness('USD')
    expect(wrapper.get('[data-value]').text()).toBe('$12.50')

    await wrapper.get('button').trigger('click')
    await flushPromises()

    expect(setCurrency).toHaveBeenCalledWith('access-token', 'AUD')
    expect(auth.user?.currency).toBe('AUD')
    expect(wrapper.get('[data-currency]').text()).toBe('AUD')
    expect(wrapper.get('[data-value]').text()).toContain('19')
  })
})
