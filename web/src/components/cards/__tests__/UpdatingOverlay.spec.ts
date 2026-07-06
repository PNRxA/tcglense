import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import UpdatingOverlay from '../UpdatingOverlay.vue'

// The grid overlay (issue #264): it keeps the (stale) slotted content mounted so nothing reflows,
// but while `loading` it dims that content, floats a spinner over it, and marks itself aria-busy —
// the content-level counterpart to the pager's own button spinner and the count line's UpdatingCue.
function mountOverlay(loading?: boolean) {
  return mount(UpdatingOverlay, {
    props: loading === undefined ? {} : { loading },
    slots: { default: '<div class="grid-content">cards</div>' },
  })
}

describe('UpdatingOverlay', () => {
  it('always renders its slotted content (the stale page stays put across a page change)', () => {
    expect(mountOverlay(false).find('.grid-content').exists()).toBe(true)
    expect(mountOverlay(true).find('.grid-content').exists()).toBe(true)
  })

  it('is passive when not loading: no spinner, no dim, interactive, not busy', () => {
    const wrapper = mountOverlay(false)
    expect(wrapper.find('.animate-spin').exists()).toBe(false)
    expect(wrapper.find('.opacity-50').exists()).toBe(false)
    expect(wrapper.find('.grid-content').element.parentElement!.hasAttribute('inert')).toBe(false)
    expect(wrapper.attributes('aria-busy')).toBeUndefined()
  })

  it('dims the content, makes it inert, floats a spinner and marks itself busy while loading', () => {
    const wrapper = mountOverlay(true)
    // A spinner overlays the grid.
    expect(wrapper.find('.animate-spin').exists()).toBe(true)
    // The stale content is dimmed and made fully non-interactive (mouse + keyboard), so neither a
    // click nor a keypress can hit a card that is about to be replaced by the incoming page.
    const content = wrapper.find('.grid-content').element.parentElement!
    expect(content.className).toContain('opacity-50')
    expect(content.className).toContain('pointer-events-none')
    expect(content.hasAttribute('inert')).toBe(true)
    // Announced to assistive tech.
    expect(wrapper.attributes('aria-busy')).toBe('true')
  })

  it('reacts to the loading prop toggling on and back off', async () => {
    const wrapper = mountOverlay(false)
    expect(wrapper.find('.animate-spin').exists()).toBe(false)
    await wrapper.setProps({ loading: true })
    expect(wrapper.find('.animate-spin').exists()).toBe(true)
    await wrapper.setProps({ loading: false })
    expect(wrapper.find('.animate-spin').exists()).toBe(false)
  })
})
