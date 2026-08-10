import { beforeEach, describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import DeckBracket from '@/components/decks/DeckBracket.vue'
import type { DeckBracketEstimate } from '@/lib/api'
import { BRACKET_BAR } from '@/lib/bracket'

// The estimate itself is the server's, so what's under test here is that the panel is
// *honest*: it renders nothing for a deck the ladder doesn't describe, it never hides a
// category the response reported, it lists the cards a count was made of (so the number can
// be checked), and it always shows the caveats that make a floor a floor.
//
// It opens **collapsed** — two rows: the rung, and one chip per category — so these mostly
// come in pairs: what the condensed view must already say, and what expanding adds.

const query = vi.hoisted(() => ({
  pending: false,
  fetching: false,
  estimate: null as DeckBracketEstimate | null,
  publicCalls: 0,
  authedCalls: 0,
  enabled: null as { value: boolean } | null,
}))

vi.mock('@/composables/useDeckAnalysis', async () => {
  const { computed } = await import('vue')
  const result = () => ({
    data: computed(() => (query.pending ? undefined : { data: query.estimate })),
    isPending: computed(() => query.pending),
    isFetching: computed(() => query.fetching || query.pending),
  })
  return {
    useDeckBracketQuery: (_g: unknown, _d: unknown, enabled?: { value: boolean }) => {
      query.authedCalls += 1
      query.enabled = enabled ?? null
      return result()
    },
    usePublicDeckBracketQuery: (_h: unknown, _d: unknown, enabled?: { value: boolean }) => {
      query.publicCalls += 1
      query.enabled = enabled ?? null
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

const LADDER = [
  { bracket: 1, label: 'Exhibition', description: 'Ultra-casual.' },
  { bracket: 2, label: 'Core', description: 'Precon level.' },
  { bracket: 3, label: 'Upgraded', description: 'Above precon.' },
  { bracket: 4, label: 'Optimized', description: 'High power.' },
  { bracket: 5, label: 'cEDH', description: 'Tournament Commander.' },
]

function makeEstimate(over: Partial<DeckBracketEstimate> = {}): DeckBracketEstimate {
  return {
    format_key: 'commander',
    format_label: 'Commander',
    bracket: 3,
    label: 'Upgraded',
    description: 'Above precon.',
    ladder: LADDER,
    reasons: ['2 Game Changers — Rhystic Study and Smothering Tithe.'],
    caveats: ["Two-card infinite combos aren't detected.", 'Bracket 5 (cEDH) describes intent.'],
    categories: [
      {
        signal: 'game_changer',
        label: 'Game Changers',
        description: 'Bracket 3 allows up to three.',
        count: 2,
        decisive: true,
        cards: [
          { card_id: 'rs', name: 'Rhystic Study', quantity: 1 },
          { card_id: 'st', name: 'Smothering Tithe', quantity: 2 },
        ],
      },
      {
        signal: 'mass_land_denial',
        label: 'Mass land denial',
        description: "Brackets 1 to 3 don't allow it.",
        count: 0,
        decisive: false,
        cards: [],
      },
      {
        signal: 'extra_turn',
        label: 'Extra turns',
        description: 'Not chained.',
        count: 0,
        decisive: false,
        cards: [],
      },
      {
        signal: 'tutor',
        label: 'Tutors',
        description: 'Sparse in brackets 1 and 2.',
        count: 0,
        decisive: false,
        cards: [],
      },
    ],
    exhibition_possible: false,
    ...over,
  }
}

beforeEach(() => {
  query.pending = false
  query.fetching = false
  query.estimate = makeEstimate()
  query.publicCalls = 0
  query.authedCalls = 0
  query.enabled = null
})

function mountPanel(props: Record<string, unknown> = {}) {
  return mount(DeckBracket, { props: { game: 'mtg', deckId: 7, ...props } })
}

/** Mount and open the disclosure — what a case asserting on the evidence needs. */
async function mountExpanded(props: Record<string, unknown> = {}) {
  const wrapper = mountPanel(props)
  await wrapper.get('button[aria-expanded]').trigger('click')
  return wrapper
}

describe('DeckBracket', () => {
  it('rests on two rows: the rung and its position on the ladder', () => {
    const wrapper = mountPanel()

    expect(wrapper.text()).toContain('Upgraded')
    // The ladder's position without the ladder — the strip itself is a detail.
    expect(wrapper.text()).toContain('Estimated bracket · 3 of 5')
    expect(wrapper.find('ol').exists()).toBe(false)
    // The bracket's own description is explanation, not a number to scan.
    expect(wrapper.text()).not.toContain('Above precon.')
  })

  it('draws the whole ladder once expanded, with only the estimated rung filled', async () => {
    const wrapper = await mountExpanded()

    const rungs = wrapper.findAll('ol > li')
    expect(rungs).toHaveLength(5)
    expect(rungs.map((rung) => rung.text())).toEqual([
      '1 · Exhibition',
      '2 · Core',
      '3 · Upgraded',
      '4 · Optimized',
      '5 · cEDH',
    ])
    const filled = rungs.filter((rung) => rung.find('div').classes().includes(BRACKET_BAR[3]!))
    expect(filled).toHaveLength(1)
    expect(filled[0]!.text()).toBe('3 · Upgraded')
    // …and the sentence that says the number is a floor, not a verdict.
    expect(wrapper.text()).toContain("don't rule out")
  })

  it('summarizes every category while collapsed, zeroes included', () => {
    const wrapper = mountPanel()

    // The condensed view still names all four counts — an absent category would otherwise
    // have to be inferred from silence, and "0 mass land denial" is half the point.
    const chips = wrapper.findAll('ul > li')
    expect(chips.map((chip) => chip.findAll('span').map((part) => part.text()))).toEqual([
      ['Game Changers', '2'],
      ['Mass land denial', '0'],
      ['Extra turns', '0'],
      ['Tutors', '0'],
    ])
    // …but not the evidence behind them.
    expect(wrapper.text()).not.toContain('Rhystic Study')
    expect(wrapper.text()).not.toContain("What this can't see")
    expect(wrapper.findAll('a')).toHaveLength(0)
  })

  it('opens the details on demand and closes them again', async () => {
    const wrapper = mountPanel()
    const toggle = wrapper.get('button[aria-expanded]')

    expect(toggle.attributes('aria-expanded')).toBe('false')
    // The body is unmounted while collapsed, so the button must not advertise a region that
    // isn't in the accessibility tree — an aria-controls IDREF that resolves to nothing is
    // dropped, taking the relationship with it.
    expect(toggle.attributes('aria-controls')).toBeUndefined()

    await toggle.trigger('click')
    expect(toggle.attributes('aria-expanded')).toBe('true')
    // The reasons and the evidence are what expanding buys.
    expect(wrapper.text()).toContain('2 Game Changers')
    expect(wrapper.text()).toContain('Rhystic Study')
    // …and now the region it names exists.
    expect(wrapper.find(`#${toggle.attributes('aria-controls')}`).exists()).toBe(true)

    await toggle.trigger('click')
    expect(toggle.attributes('aria-expanded')).toBe('false')
    expect(wrapper.text()).not.toContain('Rhystic Study')
  })

  it('lists every category in the details, including the ones the deck holds none of', async () => {
    const wrapper = await mountExpanded()
    const sections = wrapper.findAll('section')

    // Four categories plus the caveats section.
    expect(sections.length).toBe(5)
    const text = wrapper.text()
    for (const label of ['Game Changers', 'Mass land denial', 'Extra turns', 'Tutors']) {
      expect(text).toContain(label)
    }
  })

  it('links each counted card so the number can be checked', async () => {
    const wrapper = await mountExpanded()
    const links = wrapper.findAll('a')

    expect(links).toHaveLength(2)
    expect(links[0]!.attributes('href')).toBe('/cards/mtg/cards/rs')
    expect(links[0]!.text()).toBe('Rhystic Study')
    // Copies are shown only where there's more than one — a "×1" on every chip is noise.
    expect(links[1]!.text()).toBe('Smothering Tithe ×2')
  })

  it('says how many cards a capped list left out', async () => {
    const estimate = makeEstimate()
    estimate.categories[0]!.count = 9
    query.estimate = estimate

    expect((await mountExpanded()).text()).toContain('…and 7 more')
  })

  it('shows what the estimate could not see once expanded', async () => {
    const wrapper = await mountExpanded()

    expect(wrapper.text()).toContain("What this can't see")
    expect(wrapper.text()).toContain("Two-card infinite combos aren't detected.")
  })

  it('renders nothing for a deck the ladder does not describe', () => {
    query.estimate = null

    expect(mountPanel().text()).toBe('')
  })

  it('shows a one-line cue while the estimate is in flight, not a panel-sized skeleton', () => {
    query.pending = true
    const wrapper = mountPanel()

    expect(wrapper.text()).toContain('Estimating bracket…')
    // A cue, not the panel's own shape: no ladder, no chips, nothing to reflow when the
    // answer turns out to be "this isn't a Commander deck".
    expect(wrapper.find('ol').exists()).toBe(false)
    expect(wrapper.find('button[aria-expanded]').exists()).toBe(false)
  })

  it('selects the public read when a handle is given, and the authed one otherwise', () => {
    mountPanel()
    expect(query.authedCalls).toBe(1)
    expect(query.publicCalls).toBe(0)

    mountPanel({ handle: 'alice-0001' })
    expect(query.publicCalls).toBe(1)
    expect(query.authedCalls).toBe(1)
  })

  it("doesn't spend an Analytics request on a deck the ladder can't describe", () => {
    // The read shares a 30/min per-user bucket with the deck's stats, legality and goldfish,
    // and every format but Commander is a guaranteed null — so the client, which normalises
    // formats through the same mirrored table the server does, simply doesn't ask.
    mountPanel({ format: 'Modern' })
    expect(query.enabled!.value).toBe(false)

    mountPanel({ format: 'EDH' })
    expect(query.enabled!.value).toBe(true)

    mountPanel({ format: null })
    expect(query.enabled!.value).toBe(false)

    // An unstated format is not a claim that it isn't Commander, so it still asks.
    mountPanel()
    expect(query.enabled!.value).toBe(true)
  })
})
