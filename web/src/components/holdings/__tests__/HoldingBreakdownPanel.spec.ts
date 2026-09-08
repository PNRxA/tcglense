import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import { createRouter, createMemoryHistory } from 'vue-router'
import { createPinia } from 'pinia'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import type { Card, HoldingBreakdown } from '@/lib/api'
import HoldingBreakdownPanel from '../HoldingBreakdownPanel.vue'

const breakdown: HoldingBreakdown = {
  summary: { unique_cards: 3, total_cards: 6, total_value_usd: '40.00', bulk_value_usd: '0.00' },
  rarity: [
    { key: 'common', cards: 1, copies: 4, value_usd: '10.00' },
    { key: 'rare', cards: 1, copies: 1, value_usd: '30.00' },
    { key: 'unknown', cards: 1, copies: 1, value_usd: null },
  ],
  color: [{ key: 'multicolor', cards: 3, copies: 6, value_usd: '40.00' }],
  card_type: [{ key: 'creature', cards: 3, copies: 6, value_usd: '40.00' }],
  finish: [
    { key: 'regular', cards: 3, copies: 5, value_usd: '35.00' },
    { key: 'foil', cards: 1, copies: 1, value_usd: '5.00' },
  ],
  top: [
    {
      card: { id: 'card-1', name: 'Dear card', set_code: 'tst', collector_number: '1' } as Card,
      quantity: 1,
      foil_quantity: 0,
      value_usd: '30.00',
    },
    {
      card: { id: 'card-2', name: 'Cheap card', set_code: 'tst', collector_number: '2' } as Card,
      quantity: 3,
      foil_quantity: 1,
      value_usd: '10.00',
    },
  ],
  unpriced_cards: 1,
}

const empty: HoldingBreakdown = {
  summary: { unique_cards: 0, total_cards: 0, total_value_usd: null, bulk_value_usd: null },
  rarity: [],
  color: [],
  card_type: [],
  finish: [],
  top: [],
  unpriced_cards: 0,
}

function mountPanel(props: Partial<InstanceType<typeof HoldingBreakdownPanel>['$props']> = {}) {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:pathMatch(.*)*', component: { template: '<div />' } }],
  })
  return mount(HoldingBreakdownPanel, {
    props: {
      game: 'mtg',
      breakdown,
      pending: false,
      error: false,
      countNoun: 'owned' as const,
      expanded: true,
      ...props,
    },
    global: {
      plugins: [router, createPinia(), [VueQueryPlugin, { queryClient: new QueryClient() }]],
      stubs: { CardImage: true },
    },
  })
}

describe('HoldingBreakdownPanel', () => {
  it('draws the rarity facet by default with value and copies per bucket', () => {
    const wrapper = mountPanel()
    const text = wrapper.text()
    expect(text).toContain('Where the value is')
    expect(text).toContain('Top holdings by value')
    expect(text).toContain('Rare')
    expect(text).toContain('$30.00')
    expect(text).toContain('4 owned')
    // The unpriced bucket is drawn (its copies are real) and worded as such, and the
    // unpriced count rides the footnote so a small total can't be mistaken for a full one.
    expect(text).toContain('Unknown rarity')
    expect(text).toContain('Unpriced')
    expect(text).toContain("1 card has no price and isn't counted")
    // Bars are money-scaled: the $30 rare bar is three quarters of the facet's $40.
    const bars = wrapper.findAll('[role="img"]')
    expect(bars.map((bar) => (bar.element as HTMLElement).style.width)).toEqual([
      '25%',
      '75%',
      '0%',
    ])
    expect(bars[1]!.attributes('aria-label')).toBe('Rare: $30.00 · 1 owned')
  })

  it('switches facets from the segmented control', async () => {
    const wrapper = mountPanel()
    const finish = wrapper.findAll('button').find((b) => b.text() === 'Finish')!
    await finish.trigger('click')
    expect(finish.attributes('aria-pressed')).toBe('true')
    expect(wrapper.text()).toContain('Foil')
    expect(wrapper.text()).toContain('$5.00')
    expect(wrapper.text()).not.toContain('Unknown rarity')
  })

  it('lists the top holdings by held value with their counts', () => {
    const wrapper = mountPanel({ countNoun: 'wanted' })
    expect(wrapper.text()).toContain('Where the cost is')
    expect(wrapper.text()).toContain('Most expensive wanted cards')
    expect(wrapper.text()).not.toContain('Top holdings by value')
    const rows = wrapper.findAll('a')
    expect(rows.map((row) => row.text())).toEqual([
      expect.stringContaining('Dear card'),
      expect.stringContaining('Cheap card'),
    ])
    expect(rows[1]!.text()).toContain('$10.00')
    expect(rows[1]!.find('[aria-label="4 wanted"]').exists()).toBe(true)
    expect(rows[1]!.find('[aria-label="1 foil"]').exists()).toBe(true)
    expect(rows[0]!.find('[aria-label="1 foil"]').exists()).toBe(false)
    expect(rows[0]!.attributes('href')).toBe('/cards/mtg/cards/card-1')
  })

  it('rests collapsed with only the disclosure header, and opens on click', async () => {
    const wrapper = mountPanel({ expanded: false })
    expect(wrapper.text()).toContain('Where the value is')
    expect(wrapper.text()).toContain('most valuable holdings')
    expect(wrapper.text()).not.toContain('Dear card')
    expect(wrapper.findAll('[role="img"]')).toHaveLength(0)
    const disclosure = wrapper.find('button[aria-expanded]')
    expect(disclosure.attributes('aria-expanded')).toBe('false')
    await disclosure.trigger('click')
    expect(wrapper.emitted('update:expanded')?.[0]).toEqual([true])
    expect(wrapper.text()).toContain('Dear card')
  })

  it('words an empty breakdown, and keeps the pending and error states, once open', () => {
    expect(mountPanel({ breakdown: empty }).text()).toContain('Nothing to break down yet.')
    expect(
      mountPanel({ breakdown: undefined, pending: true }).find('[aria-hidden="true"]').exists(),
    ).toBe(true)
    expect(mountPanel({ breakdown: undefined, error: true }).text()).toContain(
      "Couldn't load the breakdown.",
    )
  })
})
