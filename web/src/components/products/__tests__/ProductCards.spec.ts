import { describe, it, expect, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import type { ProductCardSection } from '@/lib/api'
import ProductCards from '../ProductCards.vue'
import ProductCardsSection from '../ProductCardsSection.vue'

// Drive the parent off a controlled sections manifest, stubbing the manifest query so no
// QueryClient / Pinia is needed — the unit under test is which section blocks render, in what
// order, with what heading (issue #224). The per-section pagination lives in the stubbed
// ProductCardsSection child, so it never runs its own query here.
const state = vi.hoisted(() => ({ manifest: [] as ProductCardSection[] }))

vi.mock('@/composables/useProducts', () => ({
  PRODUCT_CARDS_PAGE_SIZE: 60,
  useProductCardSectionsQuery: () => ({ data: { value: { data: state.manifest } } }),
  useProductCardsQuery: () => ({ data: { value: { data: [], total: 0 } } }),
}))

function section(key: string, total = 1): ProductCardSection {
  return { key, total }
}

// Mount over a manifest and return the section blocks the parent rendered — each block's
// section key, heading, and (manifest) card count, in render order.
function blocks(manifest: ProductCardSection[], productType: string) {
  state.manifest = manifest
  const wrapper = mount(ProductCards, {
    props: { game: 'mtg', id: '100', productType },
    global: { stubs: { ProductCardsSection: true } },
  })
  return {
    wrapper,
    sections: wrapper.findAllComponents(ProductCardsSection).map((c) => ({
      key: c.props('sectionKey') as string,
      title: c.props('title') as string,
    })),
  }
}

describe('ProductCards sections', () => {
  it('renders one block per manifest section, in order, with the right headings', () => {
    const { sections } = blocks(
      [section('contains'), section('exclusive'), section('booster'), section('variable')],
      'collector_pack',
    )
    expect(sections.map((s) => s.key)).toEqual(['contains', 'exclusive', 'booster', 'variable'])
    expect(sections.map((s) => s.title)).toEqual([
      'In the box',
      'Collector Booster exclusives',
      'Can be pulled from boosters',
      'May be included',
    ])
  })

  it('labels the exclusives block by the product’s own booster family', () => {
    const { sections } = blocks([section('exclusive'), section('booster')], 'play_pack')
    expect(sections.map((s) => s.title)).toEqual([
      'Play Booster exclusives',
      'Can be pulled from boosters',
    ])
  })

  it('falls back to a generic exclusives label with no booster family', () => {
    const { sections } = blocks([section('exclusive')], 'bundle')
    expect(sections.map((s) => s.title)).toEqual(['Booster exclusives'])
  })

  it('sums the section counts into the heading total', () => {
    const { wrapper, sections } = blocks([section('contains', 2), section('booster', 3)], 'bundle')
    expect(sections.map((s) => s.key)).toEqual(['contains', 'booster'])
    // "Cards in this product (5)" — the grand total across sections (from the manifest).
    expect(wrapper.find('h2').text()).toContain('5')
  })

  it('renders nothing when the product has no card sections', () => {
    const { wrapper, sections } = blocks([], 'bundle')
    expect(sections).toHaveLength(0)
    expect(wrapper.find('section').exists()).toBe(false)
  })
})
