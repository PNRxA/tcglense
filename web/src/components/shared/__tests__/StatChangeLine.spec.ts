import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import StatChangeLine from '../StatChangeLine.vue'
import type { StatChange } from '@/lib/valueChange'

function gain(overrides: Partial<StatChange> = {}): StatChange {
  return {
    text: '+$3.50',
    pctText: '+2.8%',
    direction: 'up',
    window: 'week',
    asOf: '2026-09-22',
    ...overrides,
  }
}

describe('StatChangeLine', () => {
  it('renders the tag as text when the window cannot be picked', () => {
    const wrapper = mount(StatChangeLine, { props: { change: gain() } })
    expect(wrapper.find('button').exists()).toBe(false)
    expect(wrapper.text()).toContain('7D')
  })

  it('renders the tag as a menu trigger when pickable, with the sentence still accessible', () => {
    const wrapper = mount(StatChangeLine, { props: { change: gain(), pickable: true } })
    const trigger = wrapper.find('button')
    expect(trigger.exists()).toBe(true)
    expect(trigger.text()).toContain('7D')
    expect(trigger.attributes('aria-label')).toBe('Change window: 7D')
    expect(trigger.attributes('aria-haspopup')).toBe('menu')
    expect(trigger.attributes('aria-expanded')).toBe('false')
    expect(wrapper.find('.sr-only').text()).toMatch(/over the last 7 days/)
  })

  it('emits the picked window once per real change', () => {
    const wrapper = mount(StatChangeLine, { props: { change: gain(), pickable: true } })
    // Drive the radio group's contract directly: the menu is rendered in a portal on open, so
    // the pick handler is exercised through the component's exposed logic.
    const vm = wrapper.vm as unknown as { onPick: (value: string | undefined) => void }
    vm.onPick('all_time')
    vm.onPick('week') // the current window — no event
    vm.onPick('bogus') // not a window token — no event
    expect(wrapper.emitted('pick')).toEqual([['all_time']])
  })
})
