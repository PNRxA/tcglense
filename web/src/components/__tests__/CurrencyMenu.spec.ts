import { describe, expect, it } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createPinia, setActivePinia } from 'pinia'
import CurrencyMenu from '../CurrencyMenu.vue'
import { useAuthStore } from '@/stores/auth'

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
  it('is account-only and names the active currency accessibly', () => {
    const signedOut = mountMenu(false)
    expect(signedOut.find('button').exists()).toBe(false)
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
})
