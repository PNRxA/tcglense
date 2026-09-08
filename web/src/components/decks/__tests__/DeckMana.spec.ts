import { beforeEach, describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import DeckMana from '@/components/decks/DeckMana.vue'
import type { DeckManaBase, DeckManaColor } from '@/lib/api'

// The numbers are the server's, so what's under test here is that the panel is *honest*:
// the collapsed rows say sources against the number needed with the verdict beside them, the
// evidence (which spells set the requirement, which cards were counted as sources) is one
// click away rather than hidden, a hybrid pip is shown but never counted, and "not checked
// yet" is worded as syncing rather than read as "produces nothing".

const query = vi.hoisted(() => ({
  pending: false,
  fetching: false,
  loadingError: false,
  refetchError: false,
  data: null as DeckManaBase | null,
  authedCalls: 0,
  publicCalls: 0,
  preconCalls: 0,
}))

vi.mock('@/composables/useDeckAnalysis', async () => {
  const { computed } = await import('vue')
  const result = () => ({
    data: computed(() => (query.pending ? undefined : (query.data ?? undefined))),
    isPending: computed(() => query.pending),
    isFetching: computed(() => query.fetching || query.pending),
    isLoadingError: computed(() => query.loadingError),
    isRefetchError: computed(() => query.refetchError),
  })
  return {
    useDeckManaQuery: () => {
      query.authedCalls += 1
      return result()
    },
    usePublicDeckManaQuery: () => {
      query.publicCalls += 1
      return result()
    },
    usePreconManaQuery: () => {
      query.preconCalls += 1
      return result()
    },
  }
})

vi.mock('@/composables/useDetailModalLink', () => ({
  useDetailModalLink: () => ({
    hrefFor: (_kind: string, game: string, id: string) => `/cards/${game}/cards/${id}`,
    onActivate: () => {},
    warm: () => {},
  }),
}))

function color(over: Partial<DeckManaColor> = {}): DeckManaColor {
  return {
    color: 'B',
    label: 'Black',
    pips: 4,
    hybrid_pips: 0,
    demand_count: 2,
    demand: [
      {
        card_id: 'necro',
        name: 'Necropotence',
        quantity: 1,
        mana_cost: '{B}{B}{B}',
        pips: 3,
        turn: 3,
        cost_key: 'CCC',
        gold: false,
        sources_needed: 36,
        x_cost: false,
        clamped: false,
      },
      {
        card_id: 'zur',
        name: 'Zur the Enchanter',
        quantity: 1,
        mana_cost: '{1}{W}{U}{B}',
        pips: 1,
        turn: 4,
        cost_key: '3C',
        gold: true,
        sources_needed: 17,
        x_cost: false,
        clamped: false,
      },
    ],
    sources: 21,
    land_sources: 20,
    nonland_sources: 1,
    source_count: 2,
    source_cards: [
      { card_id: 'swamp', name: 'Swamp', quantity: 20, land: true },
      { card_id: 'signet', name: 'Arcane Signet', quantity: 1, land: false },
    ],
    sources_needed: 36,
    shortfall: 15,
    status: 'short',
    verdict: 'Short 15 black sources: 21 of 36 needed for Necropotence.',
    ...over,
  }
}

function makeBase(over: Partial<DeckManaBase> = {}): DeckManaBase {
  return {
    deck_size: 29,
    table_size: 99,
    library_size: 28,
    land_count: 25,
    colors: [
      color({
        color: 'U',
        label: 'Blue',
        pips: 2,
        demand_count: 1,
        demand: [
          {
            card_id: 'rhystic',
            name: 'Rhystic Study',
            quantity: 1,
            mana_cost: '{2}{U}',
            pips: 1,
            turn: 3,
            cost_key: '2C',
            gold: false,
            sources_needed: 18,
            x_cost: false,
            clamped: false,
          },
        ],
        sources: 20,
        land_sources: 20,
        nonland_sources: 0,
        source_count: 1,
        source_cards: [{ card_id: 'island', name: 'Island', quantity: 20, land: true }],
        sources_needed: 18,
        shortfall: 0,
        status: 'enough',
        verdict: 'Enough blue sources: 20 of the 18 needed.',
      }),
      color(),
    ],
    unchecked_count: 0,
    caveats: [
      "Thresholds are Frank Karsten's 2022 numbers for a 99-card deck.",
      'Hybrid, Phyrexian and {2/C} pips can be paid another way.',
    ],
    source: 'Frank Karsten, "How Many Sources Do You Need…" (2022)',
    ...over,
  }
}

beforeEach(() => {
  query.pending = false
  query.fetching = false
  query.loadingError = false
  query.refetchError = false
  query.data = makeBase()
  query.authedCalls = 0
  query.publicCalls = 0
  query.preconCalls = 0
})

function mountPanel(props: Record<string, unknown> = {}) {
  return mount(DeckMana, {
    props: { game: 'mtg', deckId: 7, ...props },
    global: { stubs: { ManaSymbols: true } },
  })
}

/** Mount and open the disclosure — what a case asserting on the evidence needs. */
async function mountExpanded(props: Record<string, unknown> = {}) {
  const wrapper = mountPanel(props)
  await wrapper.get('button[aria-expanded]').trigger('click')
  return wrapper
}

describe('DeckMana', () => {
  it('rests on one chip per colour: sources against the number needed, and the verdict', () => {
    const wrapper = mountPanel()

    const chips = wrapper.findAll('ul > li')
    expect(chips).toHaveLength(2)
    // The colour's name is the pip, spelled out for a screen reader only.
    expect(chips[0]!.text()).toContain('Blue')
    expect(chips[0]!.find('.sr-only').text()).toBe('Blue:')
    expect(chips[0]!.text()).toContain('20 / 18')
    expect(chips[0]!.text()).toContain('Enough')
    expect(chips[1]!.text()).toContain('21 / 36')
    // The shortfall rides the badge: it's the number to fix.
    expect(chips[1]!.text()).toContain('Short 15')
    // The pip count and the verdict sentence are a hover away, not on the row.
    expect(chips[1]!.text()).not.toContain('pips')
    expect(chips[1]!.attributes('title')).toBe(
      'Short 15 black sources: 21 of 36 needed for Necropotence. · 4 black pips · 21 sources in the library',
    )
    // The model, so 21 is never read against a 60-card deck.
    expect(wrapper.text()).toContain('Judged as a 99-card deck')
    expect(wrapper.text()).toContain('25 lands in a 28-card library')
    // …but not the evidence.
    expect(wrapper.text()).not.toContain('Necropotence')
    expect(wrapper.text()).not.toContain('Swamp')
    expect(wrapper.text()).not.toContain('What this assumes')
    expect(wrapper.findAll('a')).toHaveLength(0)
  })

  it('tones the verdict chip by status through the design tokens', () => {
    const wrapper = mountPanel()
    const chips = wrapper.findAll('ul > li span.rounded-md')
    expect(chips[0]!.classes()).toContain('text-success')
    expect(chips[1]!.classes()).toContain('text-warning')

    query.data = makeBase({
      colors: [
        color({
          status: 'no_demand',
          sources_needed: null,
          shortfall: 0,
          pips: 0,
          demand: [],
          demand_count: 0,
        }),
        color({
          color: 'R',
          label: 'Red',
          status: 'undecided',
          sources_needed: null,
          shortfall: 0,
        }),
      ],
    })
    const muted = mountPanel().findAll('ul > li span.rounded-md')
    expect(muted[0]!.text()).toBe('No demand')
    expect(muted[0]!.classes()).toContain('text-muted-foreground')
    expect(muted[1]!.text()).toBe('Depends on X')
    expect(muted[1]!.classes()).toContain('text-muted-foreground')
  })

  it('opens the evidence on demand: the spells that set the number and the counted sources', async () => {
    const wrapper = await mountExpanded()
    const toggle = wrapper.get('button[aria-expanded]')
    expect(toggle.attributes('aria-expanded')).toBe('true')
    expect(wrapper.find(`#${toggle.attributes('aria-controls')}`).exists()).toBe(true)

    const black = wrapper.findAll('section').find((s) => s.text().startsWith('Black'))!
    expect(black.text()).toContain('Short 15 black sources: 21 of 36 needed for Necropotence.')
    // Hungriest first, each with the number it needs and the row it was judged as.
    const demand = black.findAll('a').slice(0, 2)
    expect(demand[0]!.text()).toContain('Necropotence')
    expect(demand[0]!.text()).toContain('→ 36')
    expect(demand[0]!.attributes('title')).toBe('CCC on turn 3: 36 black sources needed')
    // No region landmarks: the sections stay unnamed, as the sibling panels' are.
    expect(wrapper.findAll('section[aria-label]')).toHaveLength(0)
    expect(demand[0]!.attributes('href')).toBe('/cards/mtg/cards/necro')
    expect(demand[1]!.text()).toContain('Zur the Enchanter')
    // The counted sources, with the land/nonland split stated.
    expect(black.text()).toMatch(/Sources · 20 lands\s+\+ 1 nonland/)
    expect(black.text()).toContain('Swamp')
    expect(black.text()).toContain('×20')
    expect(black.text()).toContain('Arcane Signet')
    // And the model behind every number, with its citation.
    expect(wrapper.text()).toContain('What this assumes')
    expect(wrapper.text()).toContain("Frank Karsten's 2022 numbers")
    expect(wrapper.text()).toContain('Source: Frank Karsten')

    await toggle.trigger('click')
    expect(toggle.attributes('aria-expanded')).toBe('false')
    expect(wrapper.text()).not.toContain('Necropotence')
  })

  it('shows a hybrid pip without counting it', async () => {
    query.data = makeBase({
      colors: [
        color({
          color: 'W',
          label: 'White',
          pips: 0,
          hybrid_pips: 8,
          demand_count: 0,
          demand: [],
          sources: 10,
          land_sources: 10,
          nonland_sources: 0,
          source_count: 1,
          source_cards: [{ card_id: 'plains', name: 'Plains', quantity: 10, land: true }],
          sources_needed: null,
          shortfall: 0,
          status: 'no_demand',
          verdict: 'Only hybrid pips ask for white — 8 pips it could pay, none it must.',
        }),
      ],
    })
    const wrapper = await mountExpanded()
    // No requirement, so the sources stand alone rather than against a made-up number.
    expect(wrapper.get('ul > li').text()).toContain('10')
    expect(wrapper.get('ul > li').text()).not.toContain('/')
    // …and the reason is on the badge, not hidden behind a bare "No demand".
    expect(wrapper.get('ul > li').text()).toContain('Hybrid only')
    expect(wrapper.get('ul > li').attributes('title')).toContain('8 hybrid pips')
    // The verdict already says it, so the note that would repeat it stays out…
    expect(wrapper.text()).toContain('Only hybrid pips ask for white')
    expect(wrapper.text()).not.toContain('Plus 8 hybrid pips')

    // …and appears only beside a hard requirement the hybrid pips don't add to.
    query.data = makeBase({
      colors: [color({ color: 'G', label: 'Green', hybrid_pips: 3 })],
    })
    const mixed = await mountExpanded()
    expect(mixed.text()).toContain('Plus 3 hybrid pips this colour could pay — not counted')
  })

  it("never presents a clamped row or an X cost as the cost's own turn", async () => {
    query.data = makeBase({
      colors: [
        color({
          demand: [
            {
              card_id: 'ex',
              name: 'Exsanguinate',
              quantity: 1,
              mana_cost: '{X}{B}{B}',
              pips: 2,
              turn: 2,
              cost_key: 'CC',
              gold: false,
              sources_needed: 30,
              x_cost: true,
              clamped: false,
            },
            {
              card_id: 'surge',
              name: 'Primal Surge',
              quantity: 1,
              mana_cost: '{8}{B}{B}',
              pips: 2,
              turn: 10,
              cost_key: '5CC',
              gold: false,
              sources_needed: 20,
              x_cost: false,
              clamped: true,
            },
          ],
        }),
      ],
    })
    const wrapper = await mountExpanded()
    const chips = wrapper.findAll('a')
    expect(chips[0]!.attributes('title')).toBe(
      'X spell: 30 black sources needed with X at zero — listed, never the card that sets the number',
    )
    expect(chips[0]!.text()).toContain('→ 30 at X=0')
    expect(chips[1]!.attributes('title')).toBe(
      "Mana value 10, judged as Karsten's 5CC row: 20 black sources needed",
    )
    expect(chips[1]!.text()).toContain('→ 20*')
  })

  it('words an unchecked card as syncing, never as producing nothing', () => {
    query.data = makeBase({ unchecked_count: 3 })
    const wrapper = mountPanel()
    expect(wrapper.text()).toContain(
      "3 cards haven't been checked for what they produce yet — card data is still syncing",
    )
  })

  it('says when a colour has no sources at all', async () => {
    query.data = makeBase({
      colors: [
        color({
          sources: 0,
          land_sources: 0,
          nonland_sources: 0,
          source_count: 0,
          source_cards: [],
          shortfall: 36,
          verdict: 'Short 36 black sources: 0 of 36 needed for Necropotence.',
        }),
      ],
    })
    const wrapper = await mountExpanded()
    expect(wrapper.text()).toContain('Nothing in the library produces black.')
  })

  it('caps the lists and says how many more were counted', async () => {
    query.data = makeBase({
      colors: [color({ demand_count: 12, source_count: 60 })],
    })
    const wrapper = await mountExpanded()
    expect(wrapper.text()).toContain('…and 10 more')
    expect(wrapper.text()).toContain('…and 58 more')
  })

  it('renders nothing for a deck with no coloured costs or sources', () => {
    query.data = makeBase({ colors: [] })
    const wrapper = mountPanel()
    expect(wrapper.html()).not.toContain('Mana base')
  })

  it('shows its shape while loading and a plain failure afterwards', () => {
    query.pending = true
    const loading = mountPanel()
    expect(loading.text()).toContain('Counting sources…')
    expect(loading.get('button[aria-expanded]').attributes('disabled')).toBeDefined()

    query.pending = false
    query.loadingError = true
    query.data = null
    const failed = mountPanel()
    expect(failed.text()).toContain("The mana base couldn't be worked out")
    expect(failed.find('button[aria-expanded]').exists()).toBe(false)
  })

  it('keeps the last answer through a failed refetch and says so', () => {
    query.refetchError = true
    const wrapper = mountPanel()
    expect(wrapper.text()).toContain("Couldn't refresh")
    expect(wrapper.text()).toContain('21 / 36')
  })

  it('selects the hook for its surface once at mount', () => {
    mountPanel()
    expect(query.authedCalls).toBe(1)
    expect(query.publicCalls).toBe(0)
    expect(query.preconCalls).toBe(0)

    mountPanel({ handle: 'alice-0001' })
    expect(query.publicCalls).toBe(1)

    mountPanel({ deckId: undefined, preconSlug: 'a-precon' })
    expect(query.preconCalls).toBe(1)
    expect(query.authedCalls).toBe(1)
  })
})
