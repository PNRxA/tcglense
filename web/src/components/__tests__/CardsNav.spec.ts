import { describe, it, expect } from 'vitest'

import { mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createMemoryHistory, createRouter } from 'vue-router'
import CardsNav from '../CardsNav.vue'

function makeRouter() {
  return createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', component: { template: '<div />' } },
      { path: '/cards', component: { template: '<div />' } },
      { path: '/cards/:game', component: { template: '<div />' } },
    ],
  })
}

async function mountNav() {
  const router = makeRouter()
  router.push('/')
  await router.isReady()
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return mount(CardsNav, {
    global: { plugins: [router, [VueQueryPlugin, { queryClient }]] },
  })
}

describe('CardsNav', () => {
  it('links the Cards parent to the games landing page', async () => {
    const wrapper = await mountNav()
    const link = wrapper.find('a[href="/cards"]')
    expect(link.exists()).toBe(true)
    expect(link.text()).toContain('Cards')
  })

  it('renders a game-menu trigger button', async () => {
    const wrapper = await mountNav()
    expect(wrapper.find('button').exists()).toBe(true)
  })
})
