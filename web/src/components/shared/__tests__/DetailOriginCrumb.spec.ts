import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import type { Card, Product } from '@/lib/api'
import DetailOriginCrumb from '../DetailOriginCrumb.vue'

const product: Product = {
  id: 'p1',
  name: 'Zendikar Rising Collector Booster Box',
  set_code: 'znr',
  set_name: 'Zendikar Rising',
  product_type: 'booster_box',
  url: null,
  has_image: false,
  prices: { usd: '200.00', usd_foil: null },
  msrp: null,
  released_at: null,
}

const card: Card = {
  id: 'c1',
  name: 'Lightning Bolt',
  set_code: 'lea',
  set_name: 'Limited Edition Alpha',
  collector_number: '161',
  rarity: 'common',
  lang: 'en',
  released_at: '1993-08-05',
  mana_cost: '{R}',
  cmc: 1,
  type_line: 'Instant',
  oracle_text: null,
  power: null,
  toughness: null,
  loyalty: null,
  color_identity: ['R'],
  colors: ['R'],
  layout: 'normal',
  prices: { usd: null, usd_foil: null, eur: null, tix: null },
  has_image: false,
  drop_name: null,
  drop_slug: null,
  secret_lair_bonus: false,
  secret_lair_spend_incentive: false,
  faces: [],
}

function mountCrumb(
  props: { game: string; kind: 'card' | 'product'; id: string },
  seed?: (qc: QueryClient) => void,
) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  seed?.(queryClient)
  return mount(DetailOriginCrumb, {
    props,
    global: { plugins: [[VueQueryPlugin, { queryClient }]] },
  })
}

describe('DetailOriginCrumb', () => {
  it('names a product origin from the warm detail cache', () => {
    const wrapper = mountCrumb({ game: 'mtg', kind: 'product', id: 'p1' }, (qc) =>
      qc.setQueryData(['product', 'mtg', 'p1'], product),
    )
    expect(wrapper.text()).toContain('Back to Zendikar Rising Collector Booster Box')
  })

  it('names a card origin from the warm detail cache', () => {
    const wrapper = mountCrumb({ game: 'mtg', kind: 'card', id: 'c1' }, (qc) =>
      qc.setQueryData(['card', 'mtg', 'c1'], card),
    )
    expect(wrapper.text()).toContain('Back to Lightning Bolt')
  })

  it('scavenges a product name from a warm list cache when there is no detail entry', () => {
    // The reverse trip's origin is often only in the grid you clicked it from, never fetched on
    // its own — findProductInCache reaches those list/relation caches.
    const wrapper = mountCrumb({ game: 'mtg', kind: 'product', id: 'p1' }, (qc) =>
      qc.setQueryData(['products', 'mtg', ''], { data: [product] }),
    )
    expect(wrapper.text()).toContain('Back to Zendikar Rising Collector Booster Box')
  })

  it('falls back to the generic noun on a cold cache', () => {
    expect(mountCrumb({ game: 'mtg', kind: 'product', id: 'p1' }).text()).toContain(
      'Back to sealed product',
    )
    expect(mountCrumb({ game: 'mtg', kind: 'card', id: 'c1' }).text()).toContain('Back to card')
  })

  it('falls back to the generic noun with no query client at all', () => {
    // The modal always mounts under a query client in the app, but the crumb must stay renderable
    // without one (the guarded useQueryClient) rather than throwing.
    const wrapper = mount(DetailOriginCrumb, {
      props: { game: 'mtg', kind: 'product', id: 'p1' },
    })
    expect(wrapper.text()).toContain('Back to sealed product')
  })

  it('emits navigate on click', async () => {
    const wrapper = mountCrumb({ game: 'mtg', kind: 'product', id: 'p1' })
    await wrapper.get('button').trigger('click')
    expect(wrapper.emitted('navigate')).toHaveLength(1)
  })
})
