import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createPinia } from 'pinia'
import SettingsView from '../SettingsView.vue'

describe('SettingsView', () => {
  it('labels the canonical bulk threshold unambiguously as USD', () => {
    const pinia = createPinia()
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const wrapper = mount(SettingsView, {
      global: { plugins: [pinia, [VueQueryPlugin, { queryClient }]] },
    })

    expect(wrapper.text()).toContain('Bulk threshold (USD)')
    expect(wrapper.text()).toContain('always measured in US dollars')
    expect(
      wrapper.get('input[aria-label="Bulk threshold in US dollars"]').attributes('aria-label'),
    ).toBe('Bulk threshold in US dollars')
  })
})
