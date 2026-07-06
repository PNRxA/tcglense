import { describe, it, expect } from 'vitest'

import { mount } from '@vue/test-utils'
import { createPinia } from 'pinia'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createMemoryHistory, createRouter } from 'vue-router'
import App from '../App.vue'

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
})
