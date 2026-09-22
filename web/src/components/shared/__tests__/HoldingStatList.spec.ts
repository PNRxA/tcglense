import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import HoldingStatList from '../HoldingStatList.vue'
import type { StatChange } from '@/lib/valueChange'

function gain(overrides: Partial<StatChange> = {}): StatChange {
  return { text: '+$3.50', pctText: '+2.8%', direction: 'up', asOf: '2026-09-22', ...overrides }
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
    const lines = wrapper.findAll('p')
    expect(lines).toHaveLength(1)
    const line = lines[0]!
    expect(line.text()).toContain('+$3.50')
    expect(line.text()).toContain('+2.8%')
    expect(line.text()).toContain('1D')
    expect(line.classes()).toContain('text-success')
    // The full sentence rides the tooltip / accessible name, with the capture it measures to.
    expect(line.attributes('title')).toMatch(
      /since the previous day's captured prices \(as of .*22\)/,
    )
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
    const [loss, flat] = wrapper.findAll('p')
    expect(loss!.classes()).toContain('text-destructive')
    expect(flat!.classes()).toContain('text-muted-foreground')
    // No date known → no "(as of …)" suffix.
    expect(flat!.attributes('title')).not.toContain('as of')
  })

  it('omits the percentage chip when there is none', () => {
    const wrapper = mount(HoldingStatList, {
      props: { items: [{ label: 'Total value', value: '$2.00', change: gain({ pctText: null }) }] },
    })
    expect(wrapper.text()).not.toContain('%')
    expect(wrapper.text()).toContain('+$3.50')
  })
})
