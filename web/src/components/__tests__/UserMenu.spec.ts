import { describe, it, expect } from 'vitest'

import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { createMemoryHistory, createRouter } from 'vue-router'
import UserMenu from '../UserMenu.vue'
import { useAuthStore } from '@/stores/auth'

function makeRouter() {
  return createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', component: { template: '<div />' } },
      { path: '/login', component: { template: '<div />' } },
      { path: '/profile', component: { template: '<div />' } },
      { path: '/dashboard', component: { template: '<div />' } },
    ],
  })
}

async function mountMenu() {
  const pinia = createPinia()
  setActivePinia(pinia)
  const router = makeRouter()
  router.push('/')
  await router.isReady()
  const wrapper = mount(UserMenu, { global: { plugins: [pinia, router] } })
  return { wrapper, store: useAuthStore() }
}

describe('UserMenu', () => {
  it('shows a login link when signed out', async () => {
    const { wrapper } = await mountMenu()
    const link = wrapper.find('a[href="/login"]')
    expect(link.exists()).toBe(true)
    expect(wrapper.text()).toContain('Sign in')
  })

  it('shows the account trigger, not a login link, when signed in', async () => {
    const { wrapper, store } = await mountMenu()
    store.accessToken = 'token'
    store.user = {
      id: 1,
      email: 'ash@pallet.town',
      display_name: 'Ash',
      created_at: '2026-01-01T00:00:00Z',
    }
    await wrapper.vm.$nextTick()

    expect(wrapper.find('a[href="/login"]').exists()).toBe(false)
    expect(wrapper.text()).toContain('Ash')
  })
})
