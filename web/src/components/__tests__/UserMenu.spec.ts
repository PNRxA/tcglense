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
      { path: '/settings', component: { template: '<div />' } },
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
  it('shows a neutral placeholder while the session is unresolved', async () => {
    // Default store: session not yet resolved and no token — render neither the Sign-in
    // link nor the account menu, so we don't flash "Sign in" at an about-to-resolve user.
    const { wrapper } = await mountMenu()
    expect(wrapper.find('a[href^="/login"]').exists()).toBe(false)
    expect(wrapper.find('[data-slot="skeleton"]').exists()).toBe(true)
  })

  it('shows a login link once resolved signed out', async () => {
    const { wrapper, store } = await mountMenu()
    store.sessionResolved = true
    await wrapper.vm.$nextTick()
    // The link carries a ?redirect= back to the current route (here, "/").
    const link = wrapper.find('a[href^="/login"]')
    expect(link.exists()).toBe(true)
    expect(wrapper.text()).toContain('Sign in')
  })

  it('shows the account trigger, not a login link, when signed in', async () => {
    const { wrapper, store } = await mountMenu()
    store.accessToken = 'token'
    store.user = {
      id: 1,
      email: 'ash@pallet.town',
      created_at: '2026-01-01T00:00:00Z',
      username: 'Ash',
      discriminator: 7,
      handle: 'Ash-0007',
    }
    await wrapper.vm.$nextTick()

    expect(wrapper.find('a[href^="/login"]').exists()).toBe(false)
    // The menu labels itself with the username now (display names were removed).
    expect(wrapper.text()).toContain('Ash')
  })
})
