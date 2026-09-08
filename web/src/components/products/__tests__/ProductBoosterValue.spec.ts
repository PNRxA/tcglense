import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createPinia } from 'pinia'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { Card, PackCardOdds, PackEv, Product, ProductEv, SlotEv } from '@/lib/api'
import ProductBoosterValue from '../ProductBoosterValue.vue'

// Drive the panel off controlled EV-query state, stubbing the composable so no API (and no
// live QueryClient fetch) is needed. The unit under test is the wiring: when the panel is
// there at all, what the headline says, and that the sheets/contributors it renders are the
// server's. The wording rules themselves live in lib/productCounts.ts and are tested there.
const state = vi.hoisted(() => ({
  ev: null as ProductEv | null,
  pending: false,
  failed: false,
}))

vi.mock('@/composables/useProducts', async () => {
  const { computed } = await import('vue')
  return {
    useProductEvQuery: () => ({
      data: computed(() => (state.pending ? undefined : { data: state.ev })),
      isPending: computed(() => state.pending),
      isError: computed(() => state.failed),
    }),
  }
})

const card = (id: string, name: string): Card =>
  ({ id, name, has_image: false, prices: { usd: null } }) as unknown as Card

const odds = (overrides: Partial<PackCardOdds> = {}): PackCardOdds => ({
  card: card('sf-1', 'Sheoldred, the Apocalypse'),
  foil: false,
  sheet: 'rareMythic',
  expected_per_pack: 0.04,
  one_in: 24,
  price_usd: '80.00',
  contribution_usd: '3.33',
  ...overrides,
})

const slot = (overrides: Partial<SlotEv> = {}): SlotEv => ({
  sheet: 'rareMythic',
  foil: false,
  picks: 1,
  ev_usd: '2.10',
  card_count: 120,
  priced_share: 1,
  top: [odds()],
  ...overrides,
})

const pack = (overrides: Partial<PackEv> = {}): PackEv => ({
  set_code: 'blb',
  booster_code: 'play',
  name: 'Play Booster',
  quantity: 1,
  cards_per_pack: 14,
  ev_usd: '5.00',
  priced_share: 1,
  slots: [slot()],
  top: [odds()],
  ...overrides,
})

const productEv = (overrides: Partial<ProductEv> = {}): ProductEv => ({
  ev_usd: '5.00',
  packs: [pack()],
  top: [odds()],
  caveats: ['Expected value is an average over many packs at current market prices.'],
  ...overrides,
})

const product = (usd: string | null): Product =>
  ({ id: '900002', name: 'Play Booster', prices: { usd, usd_foil: null } }) as unknown as Product

async function mountPanel(
  opts: { ev?: ProductEv | null; pending?: boolean; failed?: boolean; product?: Product } = {},
) {
  state.ev = opts.ev ?? null
  state.pending = opts.pending ?? false
  state.failed = opts.failed ?? false
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/sealed/:game/:id', component: { template: '<div />' } },
      { path: '/cards/:game/cards/:id', component: { template: '<div />' } },
    ],
  })
  await router.push('/sealed/mtg/900002')
  await router.isReady()
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return mount(ProductBoosterValue, {
    props: { game: 'mtg', id: '900002', product: opts.product },
    global: { plugins: [createPinia(), router, [VueQueryPlugin, { queryClient }]] },
  })
}

beforeEach(() => {
  state.ev = null
  state.pending = false
  state.failed = false
})

describe('ProductBoosterValue', () => {
  it('renders nothing for a product with no booster sheets', async () => {
    // `data: null` is the API's "there is nothing to compute here" — a precon deck, a
    // product MTGJSON doesn't describe.
    const wrapper = await mountPanel({ ev: null })
    expect(wrapper.find('section').exists()).toBe(false)
    expect(wrapper.text()).toBe('')
  })

  it('renders nothing when the read failed, rather than an error mid-page', async () => {
    const wrapper = await mountPanel({ failed: true, ev: productEv() })
    expect(wrapper.find('section').exists()).toBe(false)
  })

  it('holds the heading and a skeleton while the read is in flight', async () => {
    const wrapper = await mountPanel({ pending: true })
    expect(wrapper.get('h2').text()).toBe('Expected value')
    expect(wrapper.text()).not.toContain('per pack')
  })

  it('heads the panel exactly "Expected value" and quotes a single booster per pack', async () => {
    const wrapper = await mountPanel({ ev: productEv() })
    // The screenshot script waits on this exact heading — nothing else may join it.
    expect(wrapper.get('h2').text()).toBe('Expected value')
    expect(wrapper.text()).toContain('$5.00')
    expect(wrapper.text()).toContain('per pack, on average')
    expect(wrapper.text()).toContain("An average over many openings at today's prices")
  })

  it('quotes a box per copy and says how many packs a copy opens', async () => {
    const wrapper = await mountPanel({
      ev: productEv({ ev_usd: '180.00', packs: [pack({ quantity: 36 })] }),
    })
    expect(wrapper.text()).toContain('per copy, on average')
    expect(wrapper.text()).toContain('One copy opens 36 packs.')
    // The per-pack figure stays labelled per pack beside the per-copy headline.
    expect(wrapper.text()).toContain('36×')
  })

  it('compares the expectation with the current price only when there is one', async () => {
    const priced = await mountPanel({ ev: productEv(), product: product('10.00') })
    expect(priced.text()).toContain('Expected value is 50% of the current price')

    const unpriced = await mountPanel({ ev: productEv(), product: product(null) })
    expect(unpriced.text()).not.toContain('of the current price')
  })

  it('states the pack’s card count per pack, never as its contents', async () => {
    const wrapper = await mountPanel({ ev: productEv() })
    expect(wrapper.text()).toContain('14 cards per pack')
    expect(wrapper.text().toLowerCase()).not.toContain('cards in this product')
  })

  it('names the coverage only when some picks are unpriced', async () => {
    const whole = await mountPanel({ ev: productEv() })
    expect(whole.text()).not.toContain('of picks priced')

    const partial = await mountPanel({
      ev: productEv({ packs: [pack({ priced_share: 0.83 })] }),
    })
    expect(partial.text()).toContain('83% of picks priced')
  })

  it('breaks the pack into its sheets, flagging foil ones and linking the best pull', async () => {
    const wrapper = await mountPanel({
      ev: productEv({
        packs: [
          pack({
            slots: [
              slot(),
              slot({
                sheet: 'foil',
                foil: true,
                picks: 0.25,
                ev_usd: '0.90',
                top: [odds({ card: card('sf-2', 'Lightning Bolt'), foil: true, sheet: 'foil' })],
              }),
            ],
          }),
        ],
      }),
    })
    const rows = wrapper.findAll('tbody tr')
    expect(rows).toHaveLength(2)
    expect(rows[0]!.text()).toContain('rareMythic')
    expect(rows[0]!.text()).toContain('Sheoldred, the Apocalypse')
    // A fractional expectation keeps its decimals — a foil slot isn't one card a pack.
    expect(rows[1]!.text()).toContain('0.25')
    expect(rows[1]!.text()).toContain('Foil')
    // The best pull links to the card's own page (the shared detail-modal anchor idiom).
    expect(rows[1]!.get('a').attributes('href')).toBe('/cards/mtg/cards/sf-2')
  })

  it('lists the biggest contributors with their odds, capped at eight', async () => {
    const many = Array.from({ length: 12 }, (_, index) =>
      odds({ card: card(`sf-${index}`, `Card ${index}`), one_in: 24 + index }),
    )
    const wrapper = await mountPanel({ ev: productEv({ top: many }) })
    // The contributors list is the panel's first <ul> (the caveats footnote is the last).
    const items = wrapper.findAll('ul')[0]!.findAll('li')
    // The API sends up to 12; the panel is a summary.
    expect(items).toHaveLength(8)
    expect(items[0]!.text()).toContain('Card 0')
    expect(items[0]!.text()).toContain('1 in 24 packs')
    expect(items[0]!.text()).toContain('$80.00')
    expect(items[0]!.text()).toContain('$3.33')
    expect(wrapper.text()).not.toContain('Card 8')
  })

  it('says a contributor has no price rather than printing $0', async () => {
    const wrapper = await mountPanel({
      ev: productEv({ top: [odds({ price_usd: null, contribution_usd: '0.00' })] }),
    })
    expect(wrapper.text()).toContain('No price')
  })

  it('shows the server’s caveats verbatim', async () => {
    const wrapper = await mountPanel({
      ev: productEv({ caveats: ['Colour balancing of common slots is not simulated.'] }),
    })
    expect(wrapper.text()).toContain('Colour balancing of common slots is not simulated.')
  })
})
