import { createPinia } from 'pinia'
import { mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { describe, expect, it, vi } from 'vitest'

vi.mock('@/composables/useTurnstile', () => ({
  useTurnstile: () => ({
    execute: vi.fn<() => Promise<string | null>>().mockResolvedValue(null),
  }),
}))

import LoginView from '../LoginView.vue'

describe('login accessibility', () => {
  it('places the password input immediately after email in the form tab order', async () => {
    const page = { template: '<div />' }
    const router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/login', component: page },
        { path: '/forgot-password', component: page },
        { path: '/register', component: page },
      ],
    })
    await router.push('/login')
    const wrapper = mount(LoginView, {
      global: { plugins: [createPinia(), router] },
    })

    const tabStops = wrapper.get('form').findAll('input, a[href], button')
    expect(
      tabStops.slice(0, 3).map((element) => element.attributes('id') ?? element.text()),
    ).toEqual(['email', 'password', 'Forgot password?'])
    wrapper.unmount()
  })
})
