import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia } from 'pinia'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import CardPriceSummary from '../CardPriceSummary.vue'
import { makeCard } from '@/test/fixtures'
import type { Card } from '@/lib/api'

// The price tiles on the card page. A guest with no stored currency preference sees USD, so the
// labels below are the USD spellings. The currency store reads the FX rates through vue-query,
// hence the query client (the fetch is never awaited here — USD needs no rate).
function mountSummary(prices: Card['prices']) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return mount(CardPriceSummary, {
    props: { card: makeCard('c1', { prices }) },
    global: { plugins: [createPinia(), [VueQueryPlugin, { queryClient }]] },
  })
}

describe('CardPriceSummary', () => {
  it('shows the etched-foil price as its own tile, with the holdings note (issue #676)', () => {
    const wrapper = mountSummary({
      usd: '1.50',
      usd_foil: '9.99',
      usd_etched: '14.50',
      eur: null,
      tix: null,
    })
    const labels = wrapper.findAll('dt').map((dt) => dt.text())
    expect(labels).toEqual(['USD', 'USD foil', 'USD etched'])
    expect(wrapper.text()).toContain('$14.50')
    // An etched copy is held and valued as foil until holding lots (#594) — the tile must say
    // so, or the number reads as what a held copy is worth.
    expect(wrapper.find('[data-testid="etched-note"]').text()).toContain('valued as foil')
  })

  it('omits the etched tile and its note for a card with no etched price', () => {
    const wrapper = mountSummary({
      usd: '1.50',
      usd_foil: '9.99',
      usd_etched: null,
      eur: '1.20',
      tix: null,
    })
    const labels = wrapper.findAll('dt').map((dt) => dt.text())
    expect(labels).toEqual(['USD', 'USD foil', 'Cardmarket EUR'])
    expect(wrapper.find('[data-testid="etched-note"]').exists()).toBe(false)
  })

  it('can show an etched-only card (no regular or foil quote)', () => {
    const wrapper = mountSummary({
      usd: null,
      usd_foil: null,
      usd_etched: '14.50',
      eur: null,
      tix: null,
    })
    expect(wrapper.findAll('dt').map((dt) => dt.text())).toEqual(['USD etched'])
    expect(wrapper.find('[data-testid="etched-note"]').exists()).toBe(true)
  })

  it('renders nothing when the card carries no price at all', () => {
    const wrapper = mountSummary({
      usd: null,
      usd_foil: null,
      usd_etched: null,
      eur: null,
      tix: null,
    })
    expect(wrapper.text()).toBe('')
  })
})
