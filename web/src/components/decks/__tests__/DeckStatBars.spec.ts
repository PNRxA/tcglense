import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import DeckStatBars from '@/components/decks/DeckStatBars.vue'
import type { DeckStatItem } from '@/lib/api'

// The distribution itself is the server's; what's under test is the `selectable` mode
// (issue #671) and — just as importantly — that turning it OFF leaves the chart every other
// caller has always rendered. A bar that quietly became a button would put eight new focus
// stops into the analytics panel.

const items: DeckStatItem[] = [
  { key: 'ramp', label: 'Ramp', count: 10, color: null },
  { key: 'removal', label: 'Removal', count: 4, color: null },
  { key: 'board_wipe', label: 'Board wipes', count: 0, color: null },
]

function mountBars(props: Record<string, unknown> = {}) {
  return mount(DeckStatBars, { props: { title: 'Copies by role', items, ...props } })
}

describe('DeckStatBars', () => {
  it('renders no buttons at all unless asked for them', () => {
    for (const layout of ['rows', 'columns'] as const) {
      const wrapper = mountBars({ layout })
      expect(wrapper.findAll('button')).toHaveLength(0)
      expect(wrapper.find('[aria-pressed]').exists()).toBe(false)
    }
  })

  it('renders the same markup selectable or not, once `selectable` is off', () => {
    // The prop is defaulted, so passing it explicitly false must be indistinguishable from
    // never having heard of it — that is what keeps the analytics panel's chart untouched.
    expect(mountBars({ selectable: false }).html()).toBe(mountBars().html())
  })

  it('makes each row a toggle when selectable, zero-count buckets included', () => {
    const wrapper = mountBars({ selectable: true })
    const buttons = wrapper.findAll('button')

    expect(buttons).toHaveLength(3)
    expect(buttons.map((button) => button.attributes('type'))).toEqual([
      'button',
      'button',
      'button',
    ])
    expect(buttons.map((button) => button.attributes('aria-pressed'))).toEqual([
      'false',
      'false',
      'false',
    ])
  })

  it('selects on click and clears when the selected bucket is clicked again', async () => {
    const wrapper = mountBars({ selectable: true })

    await wrapper.findAll('button')[1]!.trigger('click')
    expect(wrapper.emitted('update:selected')).toEqual([['removal']])

    await wrapper.setProps({ selected: 'removal' })
    expect(wrapper.findAll('button')[1]!.attributes('aria-pressed')).toBe('true')
    expect(wrapper.findAll('button')[0]!.attributes('aria-pressed')).toBe('false')

    // The same bar undoes the filter it set.
    await wrapper.findAll('button')[1]!.trigger('click')
    expect(wrapper.emitted('update:selected')).toEqual([['removal'], [null]])

    // …and a different bar moves the selection rather than adding to it.
    await wrapper.findAll('button')[0]!.trigger('click')
    expect(wrapper.emitted('update:selected')).toEqual([['removal'], [null], ['ramp']])
  })

  it('ignores `selectable` in the compact strip, which has no room for a hit target', () => {
    const wrapper = mountBars({ layout: 'columns', selectable: true })
    expect(wrapper.findAll('button')).toHaveLength(0)
  })

  it('spells the unit out in every bar label, so a count is never read as cards', () => {
    const labels = mountBars({ selectable: true })
      .findAll('[role="img"]')
      .map((bar) => bar.attributes('aria-label'))
    expect(labels).toEqual(['Ramp: 10 copies', 'Removal: 4 copies', 'Board wipes: 0 copies'])
  })
})
