import { beforeEach, describe, it, expect, vi } from 'vitest'

import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { nextTick } from 'vue'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { PublicConfig } from '@/lib/api'

const apiMocks = vi.hoisted(() => ({
  getConfig: vi.fn<() => Promise<PublicConfig>>(),
}))

vi.mock('@/lib/api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/lib/api')>()),
  getConfig: apiMocks.getConfig,
}))

import App from '../App.vue'
import { announceMaintenance } from '@/lib/maintenance'
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
  beforeEach(() => {
    apiMocks.getConfig.mockReset().mockResolvedValue({
      maintenance_mode: false,
      turnstile_site_key: null,
      signups_enabled: true,
      signups_disabled_message: null,
    })
  })

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
    wrapper.unmount()
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
    wrapper.unmount()
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
    wrapper.unmount()
  })

  it('replaces a cached app shell when runtime config reports maintenance', async () => {
    apiMocks.getConfig.mockResolvedValueOnce({
      maintenance_mode: true,
      turnstile_site_key: null,
      signups_enabled: true,
      signups_disabled_message: null,
    })
    const router = makeRouter()
    await router.push('/')
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const wrapper = mount(App, {
      global: {
        plugins: [createPinia(), router, [VueQueryPlugin, { queryClient }]],
      },
    })
    await flushPromises()

    expect(wrapper.text()).toContain('Maintenance in progress')
    expect(wrapper.text()).toContain('Your collection and account data are safe.')
    expect(wrapper.text()).not.toMatch(/v\d+\.\d+\.\d+/)
    wrapper.unmount()
  })

  it('switches an already-open app after an API maintenance response', async () => {
    const router = makeRouter()
    await router.push('/')
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const wrapper = mount(App, {
      global: {
        plugins: [createPinia(), router, [VueQueryPlugin, { queryClient }]],
      },
    })
    await flushPromises()
    expect(wrapper.text()).not.toContain('Maintenance in progress')

    announceMaintenance()
    await nextTick()

    expect(wrapper.text()).toContain('Maintenance in progress')
    wrapper.unmount()
  })
})
