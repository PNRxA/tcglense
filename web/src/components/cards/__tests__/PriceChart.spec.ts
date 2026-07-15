import { describe, it, expect } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createPinia } from 'pinia'
import PriceChart from '../PriceChart.vue'

interface Pt {
  date: string
  usd: string | null
  usd_foil: string | null
}

// Mount with a fetcher that resolves to a fixed series. PriceChartInner (unovis) is stubbed
// so the empty/non-empty branch is what we assert on, without pulling the chart body into
// jsdom.
async function mountChart(data: Pt[]) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const wrapper = mount(PriceChart, {
    props: {
      queryKey: ['price-chart-test'],
      fetcher: async () => ({ data }),
      emptyText: 'NOTHING PLOTTABLE',
      singleSeries: true,
    },
    global: {
      plugins: [createPinia(), [VueQueryPlugin, { queryClient }]],
      stubs: { PriceChartInner: true },
    },
  })
  await flushPromises()
  return wrapper
}

describe('PriceChart empty state', () => {
  // The add-date clamp means a young collection's series is all-null until it has a priced
  // day; row count alone would keep the chart body up and render a blank frame (issue #283
  // review finding), so the empty state must key off there being no plottable value.
  it('shows emptyText when every point is null', async () => {
    const wrapper = await mountChart([
      { date: '2024-01-01', usd: null, usd_foil: null },
      { date: '2024-01-02', usd: null, usd_foil: null },
    ])
    expect(wrapper.text()).toContain('NOTHING PLOTTABLE')
    expect(wrapper.findComponent({ name: 'PriceChartInner' }).exists()).toBe(false)
  })

  it('shows emptyText when there are no rows at all', async () => {
    const wrapper = await mountChart([])
    expect(wrapper.text()).toContain('NOTHING PLOTTABLE')
  })

  it('renders the chart (not the empty state) once a single day is priced', async () => {
    const wrapper = await mountChart([
      { date: '2024-01-01', usd: null, usd_foil: null },
      { date: '2024-01-02', usd: '12.34', usd_foil: null },
    ])
    expect(wrapper.text()).not.toContain('NOTHING PLOTTABLE')
    expect(wrapper.findComponent({ name: 'PriceChartInner' }).exists()).toBe(true)
  })
})
