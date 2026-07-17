import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import ChartSeriesLegend from '../ChartSeriesLegend.vue'

const items = [
  { key: 'usd', label: 'Cards', color: 'var(--chart-1)', visible: true },
  { key: 'usdFoil', label: 'Sealed products', color: 'var(--chart-2)', visible: false },
]

describe('ChartSeriesLegend', () => {
  it('renders one button per series with its label', () => {
    const wrapper = mount(ChartSeriesLegend, { props: { items } })
    const buttons = wrapper.findAll('button')
    expect(buttons).toHaveLength(2)
    expect(buttons[0]!.text()).toBe('Cards')
    expect(buttons[1]!.text()).toBe('Sealed products')
  })

  it('reflects visibility via aria-pressed and a struck-through hidden label', () => {
    const wrapper = mount(ChartSeriesLegend, { props: { items } })
    const shown = wrapper.findAll('button')[0]!
    const hidden = wrapper.findAll('button')[1]!
    expect(shown.attributes('aria-pressed')).toBe('true')
    expect(shown.attributes('aria-label')).toBe('Hide Cards')
    expect(hidden.attributes('aria-pressed')).toBe('false')
    expect(hidden.attributes('aria-label')).toBe('Show Sealed products')
    // The hidden series' label is struck through to read as "off".
    expect(hidden.find('.line-through').exists()).toBe(true)
    expect(shown.find('.line-through').exists()).toBe(false)
  })

  it('emits the series key on click', async () => {
    const wrapper = mount(ChartSeriesLegend, { props: { items } })
    await wrapper.findAll('button')[1]!.trigger('click')
    expect(wrapper.emitted('toggle')).toEqual([['usdFoil']])
  })
})
