import { describe, it, expect, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import type { ProductCardEntry } from '@/lib/api'
import ProductCards from '../ProductCards.vue'

// Drive the component off a controlled page of entries, stubbing the vue-query read and the
// owned-count lookup so no QueryClient / Pinia is needed — the unit under test is the
// membership + exclusivity section split, not the data fetching.
const state = vi.hoisted(() => ({ entries: [] as ProductCardEntry[], total: 0 }))

vi.mock('@/composables/useProducts', () => ({
  PRODUCT_CARDS_PAGE_SIZE: 60,
  useProductCardsQuery: () => ({ data: { value: { data: state.entries, total: state.total } } }),
}))
vi.mock('@/composables/useCollection', () => ({
  useOwnedCounts: () => ({ ownership: {} }),
}))

function entry(membership: string, exclusive = false, id = membership): ProductCardEntry {
  return { card: { id } as ProductCardEntry['card'], membership, foil: false, exclusive }
}

function headings(entries: ProductCardEntry[], productType: string): string[] {
  state.entries = entries
  state.total = entries.length
  const wrapper = mount(ProductCards, {
    props: { game: 'mtg', id: '100', productType },
    global: { stubs: { CardGrid: true, CardPagination: true } },
  })
  return wrapper.findAll('h3').map((h) => h.text())
}

describe('ProductCards sections', () => {
  it('splits the booster pool, exclusives ahead of the shared pool, family-labelled', () => {
    const entries = [
      entry('contains', false, 'g'),
      entry('booster', true, 'x'),
      entry('booster', false, 's'),
      entry('variable', false, 'v'),
    ]
    expect(headings(entries, 'collector_pack')).toEqual([
      'In the box',
      'Collector Booster exclusives',
      'Can be pulled from boosters',
      'May be included',
    ])
  })

  it('labels the exclusives section by the product’s own booster family', () => {
    const entries = [entry('booster', true), entry('booster', false)]
    expect(headings(entries, 'play_pack')).toEqual([
      'Play Booster exclusives',
      'Can be pulled from boosters',
    ])
  })

  it('shows no exclusives section when nothing is flagged exclusive', () => {
    const entries = [entry('booster', false), entry('contains', false, 'g')]
    expect(headings(entries, 'bundle')).toEqual(['In the box', 'Can be pulled from boosters'])
  })
})
