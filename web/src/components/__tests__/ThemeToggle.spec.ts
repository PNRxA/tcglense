import { beforeEach, describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import ThemeToggle from '../ThemeToggle.vue'

describe('ThemeToggle', () => {
  beforeEach(() => {
    localStorage.clear()
    document.documentElement.classList.remove('dark')
  })

  it('renders an accessible theme toggle trigger', () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    const wrapper = mount(ThemeToggle, { global: { plugins: [pinia] } })

    expect(wrapper.find('button').exists()).toBe(true)
    expect(wrapper.text()).toContain('Toggle theme')
  })
})
