import { describe, it, expect } from 'vitest'

import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createMemoryHistory, createRouter } from 'vue-router'
import App from '../App.vue'
import { useAuthStore } from '@/stores/auth'

function makeRouter() {
  return createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', component: { template: '<div />' } },
      { path: '/login', component: { template: '<div />' } },
      { path: '/register', component: { template: '<div />' } },
    ],
  })
}

describe('App', () => {
  it('renders the app shell with the TCGLense brand', async () => {
    const pinia = createPinia()
    const router = makeRouter()
    router.push('/')
    await router.isReady()

    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const wrapper = mount(App, {
      global: {
        plugins: [pinia, router, [VueQueryPlugin, { queryClient }]],
      },
    })

    expect(wrapper.text()).toContain('TCGLense')
  })

  it('shows the build-time app version in the footer', async () => {
    const pinia = createPinia()
    const router = makeRouter()
    router.push('/')
    await router.isReady()

    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const wrapper = mount(App, {
      global: {
        plugins: [pinia, router, [VueQueryPlugin, { queryClient }]],
      },
    })

    // Injected from package.json via the vite.config.ts `define` (issue #250).
    expect(wrapper.text()).toMatch(/v\d+\.\d+\.\d+/)
  })

  it('places the signed-in currency shortcut between theme and account controls', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    const auth = useAuthStore()
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
    const router = makeRouter()
    router.push('/')
    await router.isReady()
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const wrapper = mount(App, {
      global: { plugins: [pinia, router, [VueQueryPlugin, { queryClient }]] },
    })

    const shellText = wrapper.text()
    const theme = shellText.indexOf('Toggle theme')
    const currency = shellText.indexOf('Display currency: USD')
    const account = shellText.indexOf('Account menu')
    expect(theme).toBeGreaterThanOrEqual(0)
    expect(currency).toBeGreaterThan(theme)
    expect(account).toBeGreaterThan(currency)
  })
})
