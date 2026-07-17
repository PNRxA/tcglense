import { describe, it, expect } from 'vitest'

import { mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createPinia } from 'pinia'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { Card } from '@/lib/api'
import CardPrints from '../CardPrints.vue'

function makeCard(id: string, setCode: string): Card {
  return {
    id,
    name: 'Dummy Reprinted Relic',
    set_code: setCode,
    set_name: setCode.toUpperCase(),
    collector_number: '1',
    rarity: 'rare',
    lang: 'en',
    released_at: '2024-01-01',
    mana_cost: '{2}',
    cmc: 2,
    type_line: 'Artifact',
    oracle_text: null,
    power: null,
    toughness: null,
    loyalty: null,
    color_identity: [],
    colors: [],
    layout: 'normal',
    prices: { usd: '1.00', usd_foil: null, eur: null, tix: null },
    has_image: false,
    drop_name: null,
    drop_slug: null,
    secret_lair_bonus: false,
    secret_lair_spend_incentive: false,
    faces: [],
  }
}

async function mountPrints(id: string, prints: Card[]) {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/cards/:game/cards/:id', component: { template: '<div />' } }],
  })
  router.push(`/cards/mtg/cards/${id}`)
  await router.isReady()
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  // Seed the cache so the prints are available synchronously (no network in tests).
  queryClient.setQueryData(['card-prints', 'mtg', id], { data: prints })
  // CardGrid (rendered for each printing) reads the persisted card-size preference
  // from a Pinia store, so the mounted tree needs an active Pinia.
  return mount(CardPrints, {
    props: { game: 'mtg', id },
    global: { plugins: [router, createPinia(), [VueQueryPlugin, { queryClient }]] },
  })
}

describe('CardPrints', () => {
  it('lists the other printings with a count, revealed on expand (collapsed by default)', async () => {
    const wrapper = await mountPrints('dummy-dmb-0080', [
      makeCard('dummy-dmu-0013', 'dmu'),
      makeCard('dummy-dmb-0080', 'dmb'),
    ])
    // The count shows on the (collapsed) header; the grid stays hidden until expanded (#332).
    expect(wrapper.text()).toContain('Other printings (2)')
    expect(wrapper.get('button[aria-expanded]').attributes('aria-expanded')).toBe('false')
    expect(wrapper.find('a[href="/cards/mtg/cards/dummy-dmu-0013"]').exists()).toBe(false)

    await wrapper.get('button[aria-expanded]').trigger('click')
    // One tile link per printing, each pointing at that printing's detail page.
    expect(wrapper.find('a[href="/cards/mtg/cards/dummy-dmu-0013"]').exists()).toBe(true)
    expect(wrapper.find('a[href="/cards/mtg/cards/dummy-dmb-0080"]').exists()).toBe(true)
  })

  it('filters the other printings by set/number/rarity when expanded (#472)', async () => {
    const wrapper = await mountPrints('dummy-aaa-0001', [
      makeCard('dummy-aaa-0001', 'aaa'),
      makeCard('dummy-bbb-0002', 'bbb'),
    ])
    await wrapper.get('button[aria-expanded]').trigger('click')
    // Both printings are visible before filtering.
    expect(wrapper.find('a[href="/cards/mtg/cards/dummy-aaa-0001"]').exists()).toBe(true)
    expect(wrapper.find('a[href="/cards/mtg/cards/dummy-bbb-0002"]').exists()).toBe(true)

    // Narrowing to the "bbb" set drops the "aaa" printing.
    await wrapper
      .get('input[aria-label="Filter printings by set, number, or rarity"]')
      .setValue('bbb')
    expect(wrapper.find('a[href="/cards/mtg/cards/dummy-aaa-0001"]').exists()).toBe(false)
    expect(wrapper.find('a[href="/cards/mtg/cards/dummy-bbb-0002"]').exists()).toBe(true)

    // A filter matching nothing shows the empty note instead of the grid.
    await wrapper
      .get('input[aria-label="Filter printings by set, number, or rarity"]')
      .setValue('zzz')
    expect(wrapper.text()).toContain('No printings match')
  })

  it('renders nothing when the card has no other printings', async () => {
    const wrapper = await mountPrints('dummy-dmb-0001', [])
    expect(wrapper.find('section').exists()).toBe(false)
    expect(wrapper.text()).not.toContain('Other printings')
  })
})
