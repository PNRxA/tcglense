import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import HoldingStatList from '../HoldingStatList.vue'
import type { StatChange } from '@/lib/valueChange'
import type { MoverWindow } from '@/lib/api'

function gain(overrides: Partial<StatChange> = {}): StatChange {
  return {
    text: '+$3.50',
    pctText: '+2.8%',
    direction: 'up',
    window: 'day',
    asOf: '2026-09-22',
    ...overrides,
  }
}

describe('HoldingStatList', () => {
  it('drops items without a value and renders nothing when none remain', () => {
    const wrapper = mount(HoldingStatList, {
      props: {
        items: [
          { label: 'Total value', value: null },
          { label: 'Bulk value', value: undefined },
        ],
      },
    })
    expect(wrapper.find('dl').exists()).toBe(false)
  })

  it('renders each stat as a label/value pair with no change line by default', () => {
    const wrapper = mount(HoldingStatList, {
      props: {
        items: [
          { label: 'Unique cards', value: '12' },
          { label: 'Total value', value: '$128.50' },
        ],
      },
    })
    const terms = wrapper.findAll('dt').map((dt) => dt.text())
    expect(terms).toEqual(['Unique cards', 'Total value'])
    expect(wrapper.findAll('dd').map((dd) => dd.text())).toEqual(['12', '$128.50'])
    expect(wrapper.find('.stat-change').exists()).toBe(false)
    expect(wrapper.text()).not.toContain('1D')
  })

  it('renders a change line under a stat that carries one, coloured by direction', () => {
    const wrapper = mount(HoldingStatList, {
      props: {
        items: [
          { label: 'Total value', value: '$128.50', change: gain() },
          { label: 'Bulk value', value: '$4.00', change: null },
        ],
      },
    })
    const lines = wrapper.findAll('.stat-change')
    expect(lines).toHaveLength(1)
    const line = lines[0]!
    expect(line.text()).toContain('+$3.50')
    expect(line.text()).toContain('+2.8%')
    expect(line.text()).toContain('1D')
    expect(line.classes()).toContain('text-success')
    // The line sits inside the value's <dd> (a <dl> wrapper may only hold <dt>/<dd>), and no
    // <p> is emitted.
    expect(wrapper.find('dd .stat-change').exists()).toBe(true)
    expect(wrapper.find('p').exists()).toBe(false)
    // The full sentence — with the capture it measures to — is the line's accessible text
    // (a visually-hidden span; every visible fragment is aria-hidden) and the mouse tooltip.
    const sentence = /since the previous day's captured prices \(as of .*22\)/
    expect(line.attributes('title')).toMatch(sentence)
    expect(line.find('.sr-only').text()).toMatch(sentence)
    expect(line.find('.sr-only').text()).toContain('+2.8%')
    expect(line.attributes('aria-label')).toBeUndefined()
    const visible = line.findAll('span:not(.sr-only)')
    expect(visible.length).toBeGreaterThan(0)
    for (const span of visible) expect(span.attributes('aria-hidden')).toBe('true')
  })

  it('uses the destructive token for a loss and the muted one for a flat day', () => {
    const wrapper = mount(HoldingStatList, {
      props: {
        items: [
          {
            label: 'Cards',
            value: '$1.00',
            change: gain({ text: '−$0.50', pctText: '−1.0%', direction: 'down' }),
          },
          {
            label: 'Sealed',
            value: '$2.00',
            change: gain({ text: '$0.00', pctText: '0.0%', direction: 'flat', asOf: null }),
          },
        ],
      },
    })
    const [loss, flat] = wrapper.findAll('.stat-change')
    expect(loss!.classes()).toContain('text-destructive')
    expect(flat!.classes()).toContain('text-muted-foreground')
    // No date known → no "(as of …)" suffix.
    expect(flat!.attributes('title')).not.toContain('as of')
  })

  it('tags the line with the window it covers and words the sentence for it', () => {
    const wrapper = mount(HoldingStatList, {
      props: {
        items: [
          { label: 'Total value', value: '$128.50', change: gain({ window: 'week' }) },
          { label: 'Cards', value: '$1.00', change: gain({ window: 'all_time' }) },
        ],
      },
    })
    const [week, all] = wrapper.findAll('.stat-change')
    expect(week!.text()).toContain('7D')
    expect(week!.text()).not.toContain('1D')
    expect(week!.find('.sr-only').text()).toMatch(/over the last 7 days \(as of .*22\)/)
    expect(all!.text()).toContain('All')
    expect(all!.find('.sr-only').text()).toContain('since the earliest captured prices')
  })

  it('renders the window tag as plain text when no window is bound', () => {
    const wrapper = mount(HoldingStatList, {
      props: { items: [{ label: 'Total value', value: '$128.50', change: gain() }] },
    })
    expect(wrapper.find('.stat-change button').exists()).toBe(false)
    expect(wrapper.find('.stat-change').text()).toContain('1D')
  })

  it('makes the window tag a picker when a window is bound', () => {
    const wrapper = mount(HoldingStatList, {
      props: {
        items: [
          { label: 'Total value', value: '$128.50', change: gain({ window: 'week' }) },
          { label: 'Cards', value: '$1.00', change: gain({ window: 'week' }) },
        ],
        changeWindow: 'week' as MoverWindow,
      },
    })
    const triggers = wrapper.findAll('.stat-change button')
    expect(triggers).toHaveLength(2)
    const trigger = triggers[0]!
    expect(trigger.text()).toContain('7D')
    expect(trigger.attributes('aria-label')).toBe('Change window: 7D')
    expect(trigger.attributes('aria-haspopup')).toBe('menu')
  })

  it('omits the percentage chip when there is none', () => {
    const wrapper = mount(HoldingStatList, {
      props: { items: [{ label: 'Total value', value: '$2.00', change: gain({ pctText: null }) }] },
    })
    expect(wrapper.text()).not.toContain('%')
    expect(wrapper.text()).toContain('+$3.50')
  })
})
