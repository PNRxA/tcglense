import { describe, it, expect, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import GhostToggle from '../GhostToggle.vue'

function mountToggle(props: { showGhosts: boolean; list?: 'collection' | 'wishlist' }) {
  const pinia = createPinia()
  setActivePinia(pinia)
  return mount(GhostToggle, { props, global: { plugins: [pinia] } })
}

const settingsTrigger = '[title="Display settings"]'

describe('GhostToggle', () => {
  beforeEach(() => localStorage.clear())

  it('emits toggle with the negated state when the button is clicked', async () => {
    const wrapper = mountToggle({ showGhosts: false })
    await wrapper.get('button[aria-pressed]').trigger('click')
    expect(wrapper.emitted('toggle')?.[0]).toEqual([true])
  })

  it('emits toggle(false) to turn ghosts off', async () => {
    const wrapper = mountToggle({ showGhosts: true })
    await wrapper.get('button[aria-pressed]').trigger('click')
    expect(wrapper.emitted('toggle')?.[0]).toEqual([false])
  })

  it('reflects the on/off state via aria-pressed', () => {
    expect(
      mountToggle({ showGhosts: true }).get('button[aria-pressed]').attributes('aria-pressed'),
    ).toBe('true')
    expect(
      mountToggle({ showGhosts: false }).get('button[aria-pressed]').attributes('aria-pressed'),
    ).toBe('false')
  })

  it('on the collection, reveals the settings caret only while ghosts are shown', () => {
    expect(mountToggle({ showGhosts: false }).find(settingsTrigger).exists()).toBe(false)
    expect(mountToggle({ showGhosts: true }).find(settingsTrigger).exists()).toBe(true)
  })

  it('on the wish list, keeps the settings caret reachable even with ghosts off', () => {
    // The wish list's "Show owned" markers act in every browse mode, so its control must stay
    // reachable regardless of ghost mode (issue #213 — no undiscoverable dead-end).
    expect(
      mountToggle({ showGhosts: false, list: 'wishlist' }).find(settingsTrigger).exists(),
    ).toBe(true)
    expect(mountToggle({ showGhosts: true, list: 'wishlist' }).find(settingsTrigger).exists()).toBe(
      true,
    )
  })
})
