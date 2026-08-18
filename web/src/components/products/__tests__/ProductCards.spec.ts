import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import type { ProductCardSection } from '@/lib/api'
import ProductCards from '../ProductCards.vue'
import ProductCardsSection from '../ProductCardsSection.vue'

// Drive the parent off a controlled sections manifest + search state, stubbing the manifest
// query and the URL-backed search composable so no QueryClient / router / Pinia is needed —
// the unit under test is which section blocks render (issue #224) and how the search box gates
// them (issue #222). The per-section pagination lives in the stubbed ProductCardsSection child.
const state = vi.hoisted(() => ({
  manifest: [] as ProductCardSection[],
  query: '',
  sort: 'default',
  error: undefined as unknown,
  // True while vue-query is holding the previous (filtered) manifest up through a refetch.
  stale: false,
}))

vi.mock('@/composables/useProducts', () => ({
  PRODUCT_CARDS_PAGE_SIZE: 60,
  useProductCardSectionsQuery: () => ({
    data: {
      get value() {
        return { data: state.manifest }
      },
    },
    error: {
      get value() {
        return state.error
      },
    },
    // The heading treats a stale (kept-previous) manifest as filtered, so the mount needs this.
    isPlaceholderData: {
      get value() {
        return state.stale
      },
    },
  }),
  useProductCardsQuery: () => ({ data: { value: { data: [], total: 0 } } }),
}))
vi.mock('@/composables/useProductCardsSearch', async () => {
  const { computed, ref } = await import('vue')
  return {
    useProductCardsSearch: () => ({
      searchInput: ref(state.query),
      query: computed(() => state.query),
      sort: ref(state.sort),
    }),
  }
})
vi.mock('@/composables/useCardSearch', () => ({
  // A truthy error means "malformed search"; the exact message is the real fn's concern.
  searchErrorMessage: (error: unknown) => (error ? 'Malformed search.' : null),
}))

function section(key: string, total = 1, boosterFamily: string | null = null): ProductCardSection {
  return { key, total, booster_family: boosterFamily, component: null, inherited: false }
}

/// A section whose every card arrived through a listed sub-product (its page shows them).
function inherited(key: string, total = 1): ProductCardSection {
  return { ...section(key, total), inherited: true }
}

/// An unlisted component's section — the cards packed in a named box piece.
function componentSection(key: string, name: string, total = 1): ProductCardSection {
  return { ...section(key, total), component: name }
}

// Mount over a manifest (+ optional search state) and return the section blocks the parent
// rendered — each block's section key, heading, and the search threaded into it, in order.
function mountCards(
  manifest: ProductCardSection[],
  productType: string,
  opts: { query?: string; sort?: string; error?: unknown; stale?: boolean } = {},
) {
  state.manifest = manifest
  state.query = opts.query ?? ''
  state.sort = opts.sort ?? 'default'
  state.error = opts.error
  state.stale = opts.stale ?? false
  const wrapper = mount(ProductCards, {
    props: { game: 'mtg', id: '100', productType },
    global: {
      stubs: {
        ProductCardsSection: true,
        CardSearchBox: true,
        SearchSyntaxHint: true,
        AdvancedSearchPanel: true,
        CardSizeMenu: true,
        CardSortMenu: true,
      },
    },
  })
  return {
    wrapper,
    sections: wrapper.findAllComponents(ProductCardsSection).map((c) => ({
      key: c.props('sectionKey') as string,
      component: c.props('component') as string | undefined,
      title: c.props('title') as string,
      count: c.props('count') as number,
      search: c.props('search') as string,
      sort: c.props('sort') as string,
    })),
  }
}

beforeEach(() => {
  state.manifest = []
  state.query = ''
  state.sort = 'default'
  state.error = undefined
  state.stale = false
})

describe('ProductCards sections', () => {
  it('renders one block per manifest section, in order, with the right headings', () => {
    const { sections } = mountCards(
      [section('contains'), section('exclusive'), section('booster'), section('variable')],
      'collector_pack',
    )
    expect(sections.map((s) => s.key)).toEqual(['contains', 'exclusive', 'booster', 'variable'])
    expect(sections.map((s) => s.title)).toEqual([
      'Guaranteed cards',
      'Exclusive to Collector Booster',
      // "Shared", not "Collector Booster pull pool": the exclusives split out above are part of
      // that pool too, so this block is its remainder — the page must not state two sizes for
      // one named pool.
      'Shared Collector Booster pool',
      'May be included',
    ])
  })

  it('titles the exclusives block by the backend-provided contained booster family', () => {
    // A bundle's own product_type carries no family, but the backend names the collector
    // booster it wraps — so the section reads "Exclusive to Collector Booster" (issue #290).
    // The shared pool below it spans every family, so it stays generic on a bundle.
    const { sections } = mountCards(
      [section('exclusive', 1, 'collector_pack'), section('booster')],
      'bundle',
    )
    expect(sections.map((s) => s.title)).toEqual([
      'Exclusive to Collector Booster',
      'Shared booster pool',
    ])
  })

  it('falls back to the product’s own booster family when the backend hands none down', () => {
    const { sections } = mountCards([section('exclusive'), section('booster')], 'play_pack')
    expect(sections.map((s) => s.title)).toEqual([
      'Exclusive to Play Booster',
      'Shared Play Booster pool',
    ])
  })

  it('falls back to a generic exclusives label with no booster family at all', () => {
    const { sections } = mountCards([section('exclusive')], 'bundle')
    // Never "this booster" — the viewed product is a bundle, which is not one.
    expect(sections.map((s) => s.title)).toEqual(["Exclusive to this product's boosters"])
  })

  it('names an unrecognised section key as a possible-cards bucket, never a containment claim', () => {
    // Mirrors the server, which files an unknown membership into the weakest bucket — the raw
    // wire slug must never surface as a heading.
    const { sections } = mountCards([section('someday')], 'bundle')
    expect(sections.map((s) => s.title)).toEqual(['Possible cards'])
  })

  it('sums the section counts into the heading total', () => {
    const { wrapper, sections } = mountCards(
      [section('contains', 2), section('booster', 3)],
      'bundle',
    )
    expect(sections.map((s) => s.key)).toEqual(['contains', 'booster'])
    // The grand total across sections (from the manifest).
    expect(wrapper.find('h2').text()).toContain('5')
  })

  it('threads each manifest count into its block (labels the collapsed header, #291)', () => {
    const { sections } = mountCards([section('contains', 2), section('booster', 3)], 'bundle')
    expect(sections.map((s) => s.count)).toEqual([2, 3])
  })

  it('renders nothing when the product has no card sections', () => {
    const { wrapper, sections } = mountCards([], 'bundle')
    expect(sections).toHaveLength(0)
    expect(wrapper.find('section').exists()).toBe(false)
  })

  it('threads the committed search into each block', () => {
    const { sections } = mountCards([section('booster')], 'bundle', { query: 't:goblin' })
    expect(sections.map((s) => s.search)).toEqual(['t:goblin'])
  })

  it('threads the committed sort into every block (so the sections re-order together)', () => {
    const { sections } = mountCards([section('contains'), section('booster')], 'bundle', {
      sort: 'price:desc',
    })
    expect(sections.map((s) => s.sort)).toEqual(['price:desc', 'price:desc'])
  })

  it('keeps the search box up and shows a no-match note when a search matches nothing', () => {
    const { wrapper, sections } = mountCards([], 'bundle', { query: 'zzznope' })
    // The section (and thus the search box) stays mounted so the filter can be cleared…
    expect(wrapper.find('section').exists()).toBe(true)
    // …but no blocks render, replaced by the note.
    expect(sections).toHaveLength(0)
    expect(wrapper.text()).toContain('No cards match')
  })

  it('surfaces a malformed-search error instead of the blocks', () => {
    const { wrapper, sections } = mountCards([], 'bundle', { query: 'bad:', error: new Error('x') })
    expect(sections).toHaveLength(0)
    expect(wrapper.text()).toContain('Malformed search.')
    expect(wrapper.text()).not.toContain('No cards match')
  })

  it('hides an inherited booster/exclusive section — its pool lives on the linked child’s page', () => {
    // A booster box / bundle whose whole pool arrived through a listed sub-product ("What's
    // in the box" links it): rendering the pool here doubled it up, so only the sections
    // with something of their own survive. The guaranteed card is direct and stays.
    const { wrapper, sections } = mountCards(
      [section('contains', 2), inherited('exclusive', 80), inherited('booster', 520)],
      'bundle',
    )
    expect(sections.map((s) => s.key)).toEqual(['contains'])
    // The heading total must agree with what's on screen, not with the hidden pool.
    expect(wrapper.find('h2').text()).toContain('2')
  })

  it('keeps an inherited guarantee — hiding it would lose information, not deduplicate it', () => {
    const { sections } = mountCards([inherited('contains', 3)], 'bundle')
    expect(sections.map((s) => s.key)).toEqual(['contains'])
  })

  it('keeps calling the booster block "shared" when the exclusive section is hidden', () => {
    // A hidden (inherited) exclusive section still means the server split those cards OUT
    // of the booster block — so the surviving block is the pool's shared remainder and must
    // not reclaim wholeness just because its sibling isn't rendered.
    const { sections } = mountCards(
      [
        { ...inherited('exclusive', 80), booster_family: 'collector_pack' },
        section('booster', 520),
      ],
      'collector_pack',
    )
    expect(sections.map((s) => s.key)).toEqual(['booster'])
    expect(sections[0]!.title).toBe('Shared Collector Booster pool')
  })

  it('falls back to scrolling the cards area when the clicked component has no visible block', () => {
    // A committed search can filter a component's sections out of the manifest while the
    // "What's in the box" row (built from the unfiltered manifest) still offers the click —
    // it must land somewhere, not die.
    const scrolled = vi.fn<() => void>()
    Element.prototype.scrollIntoView = scrolled
    const { wrapper } = mountCards([section('contains', 1)], 'bundle', { query: 't:goblin' })
    ;(wrapper.vm as unknown as { openComponent: (name: string) => void }).openComponent('Land Pack')
    expect(scrolled).toHaveBeenCalled()
  })

  it('renders an unlisted component’s sections, titled after the box piece, in order', () => {
    const { sections } = mountCards(
      [
        section('contains', 1),
        componentSection('contains', 'Land Pack', 5),
        componentSection('variable', 'Land Pack', 1),
        section('variable', 2),
      ],
      'bundle',
    )
    expect(sections.map((s) => [s.key, s.component ?? null])).toEqual([
      ['contains', null],
      ['contains', 'Land Pack'],
      ['variable', 'Land Pack'],
      ['variable', null],
    ])
    expect(sections.map((s) => s.title)).toEqual([
      'Guaranteed cards',
      'In the Land Pack',
      'May be in the Land Pack',
      'May be included',
    ])
  })

  it('words a component pool as pullable, never as containment', () => {
    const { sections } = mountCards([componentSection('booster', 'Welcome Pack', 40)], 'bundle')
    expect(sections.map((s) => s.title)).toEqual(['Pullable from the Welcome Pack'])
  })

  it('never lets a component section borrow the exclusive family label', () => {
    // Only the *plain* exclusive section names a booster family; a component exclusive key
    // (the server never emits one today) must not trip the family lookup.
    const { sections } = mountCards(
      [componentSection('contains', 'Land Pack'), section('exclusive', 1, 'collector_pack')],
      'bundle',
    )
    expect(sections.map((s) => s.title)).toEqual([
      'In the Land Pack',
      'Exclusive to Collector Booster',
    ])
  })
})

// The heading is the string that used to lie: it read "Cards in this product (600)" over a
// booster's pull pool. It now takes its noun from the certainties the manifest holds.
describe('ProductCards heading', () => {
  const headingOf = (manifest: ProductCardSection[], productType = 'bundle') =>
    mountCards(manifest, productType).wrapper.find('h2').text().replace(/\s+/g, ' ').trim()

  it('keeps the containment wording when every card really is guaranteed', () => {
    expect(headingOf([section('contains', 99)], 'commander_deck')).toBe(
      'Cards in this product (99)',
    )
  })

  it('calls a booster’s pool a pool, and puts the unit inside the number', () => {
    // The reported bug: a pack holding ~15 cards announced its ~600-card pull pool as its
    // contents. Both the noun and the parenthetical have to change.
    const heading = headingOf([section('booster', 600)], 'play_pack')
    expect(heading).toBe('What you can pull (600-card pool)')
    expect(heading).not.toContain('in this product')
  })

  it('counts the family-exclusive slice into the same pool, never as extra cards', () => {
    expect(headingOf([section('exclusive', 80), section('booster', 520)], 'collector_pack')).toBe(
      'What you can pull (600-card pool)',
    )
  })

  it('hedges anything randomized with one voice', () => {
    expect(headingOf([section('variable', 12)])).toBe('What you might get (12)')
  })

  it('keeps the pool legible when a randomized insert joins it', () => {
    // A collector box is routinely pool + a randomized insert with nothing guaranteed — the
    // shape the dummy catalog itself ships. Without the split line it reads as one number.
    const { wrapper } = mountCards(
      [section('booster', 600), section('variable', 2)],
      'collector_display',
    )
    expect(wrapper.find('h2').text().replace(/\s+/g, ' ').trim()).toBe('What you might get (602)')
    expect(wrapper.find('h2 + p').text()).toBe(
      '600 in the pull pool · 2 sometimes included — a copy opens some of the pool, not all of it.',
    )
  })

  it('drops the pool-size claim while a search narrows the manifest', () => {
    // Filtered, the number is what matched — asserting "(3-card pool)" over a 600-card pool
    // would be a brand-new lie.
    const filtered = mountCards([section('booster', 3)], 'play_pack', { query: 't:goblin' })
    expect(filtered.wrapper.find('h2').text().replace(/\s+/g, ' ').trim()).toBe(
      'What you can pull (3)',
    )
  })

  it('holds the pool-size claim back while a cleared search refetches', () => {
    // `searching` flips the instant the URL clears, but keepPreviousData still holds the
    // *filtered* counts — so for one refetch the heading would assert a 3-card pool over a
    // 600-card one. The placeholder flag has to suppress the unit too.
    const stale = mountCards([section('booster', 3)], 'play_pack', { stale: true })
    expect(stale.wrapper.find('h2').text().replace(/\s+/g, ' ').trim()).toBe(
      'What you can pull (3)',
    )
  })

  it('names both certainties on a mixed product, and spells the split out below', () => {
    const { wrapper } = mountCards([section('contains', 2), section('booster', 600)], 'bundle')
    expect(wrapper.find('h2').text().replace(/\s+/g, ' ').trim()).toBe(
      "What's guaranteed, what's random (602)",
    )
    expect(wrapper.find('h2 + p').text()).toBe(
      '2 guaranteed · 600 in the pull pool — a copy opens some of the pool, not all of it.',
    )
  })

  it('gives a single-certainty product no reconciling line', () => {
    expect(
      mountCards([section('booster', 5)], 'play_pack')
        .wrapper.find('h2 + p')
        .exists(),
    ).toBe(false)
  })
})
