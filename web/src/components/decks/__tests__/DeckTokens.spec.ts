import { beforeEach, describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import DeckTokens from '@/components/decks/DeckTokens.vue'
import type { Card, DeckTokens as DeckTokensPayload } from '@/lib/api'

// The token list itself is the server's, so what's under test here is that the panel keeps
// the response's two carefully-separated silences: it never turns "we haven't checked this
// card yet" into "this deck makes no tokens", and it never invents a number of tokens to
// bring. Plus the plain rendering contract: a token without a printing in the catalog still
// has to say what it is.

const query = vi.hoisted(() => ({
  pending: false,
  fetching: false,
  error: false,
  data: null as DeckTokensPayload | null,
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
    isError: computed(() => query.error),
  })
  return {
    useDeckTokensQuery: () => {
      query.authedCalls += 1
      return result()
    },
    usePublicDeckTokensQuery: () => {
      query.publicCalls += 1
      return result()
    },
    usePreconTokensQuery: () => {
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

vi.mock('@/composables/useCurrency', () => ({
  useCurrency: () => ({ formatUsd: () => '' }),
}))

function tokenCard(id: string, name: string): Card {
  return {
    id,
    name,
    set_code: 'tdmb',
    set_name: 'Tokens',
    collector_number: '1',
    rarity: 'common',
    lang: 'en',
    released_at: '2024-01-15',
    mana_cost: null,
    cmc: 0,
    type_line: `Token Creature — ${name}`,
    oracle_text: null,
    power: '1',
    toughness: '1',
    loyalty: null,
    color_identity: [],
    colors: [],
    layout: 'token',
    prices: { usd: null, usd_foil: null, eur: null, tix: null },
    has_image: false,
    drop_name: null,
    drop_slug: null,
    secret_lair_bonus: false,
    secret_lair_spend_incentive: false,
    faces: [],
    legalities: null,
  }
}

function makePayload(over: Partial<DeckTokensPayload> = {}): DeckTokensPayload {
  return {
    tokens: [
      {
        key: 'oracle-goblin',
        name: 'Goblin',
        type_line: 'Token Creature — Goblin',
        card: tokenCard('tok-goblin', 'Goblin'),
        sources: [
          { card_id: 'krenko', name: 'Krenko, Mob Boss', quantity: 1 },
          { card_id: 'rabblemaster', name: 'Goblin Rabblemaster', quantity: 2 },
        ],
        source_count: 2,
      },
    ],
    unchecked_count: 0,
    ...over,
  }
}

beforeEach(() => {
  // The panel reads the shared card-size preference, so its tiles match the deck's.
  setActivePinia(createPinia())
  query.pending = false
  query.fetching = false
  query.error = false
  query.data = makePayload()
  query.authedCalls = 0
  query.publicCalls = 0
  query.preconCalls = 0
})

function mountPanel(props: Record<string, unknown> = {}) {
  return mount(DeckTokens, {
    props: { game: 'mtg', deckId: 7, ...props },
    global: { stubs: { RouterLink: { template: '<a><slot /></a>' } } },
  })
}

describe('DeckTokens', () => {
  it('lists each token once with how many of the deck’s cards make it', () => {
    const wrapper = mountPanel()

    expect(wrapper.text()).toContain('Tokens to bring')
    expect(wrapper.text()).toContain('Goblin')
    expect(wrapper.text()).toContain('×2')
  })

  it('never states a number of tokens to bring', () => {
    // The catalog can't tell "create a Treasure" from "create X Treasures", so the only
    // counts on the panel are counts of *cards* — anything else would be invented.
    const text = mountPanel().text()
    expect(text).not.toMatch(/bring \d/i)
    expect(text).not.toMatch(/\d+ copies/i)
  })

  it('names the cards that make a token behind the disclosure', async () => {
    const wrapper = mountPanel()
    expect(wrapper.text()).not.toContain('Krenko, Mob Boss')

    await wrapper.get('button[aria-expanded]').trigger('click')

    expect(wrapper.text()).toContain('Krenko, Mob Boss')
    // A card the deck runs two of says so — that's the arithmetic the panel leaves to the
    // player rather than doing wrongly for them.
    expect(wrapper.text()).toContain('Goblin Rabblemaster')
    expect(wrapper.text()).toContain('×2')
  })

  it('renders a token whose printing is not in the catalog from its stored name', () => {
    query.data = makePayload({
      tokens: [
        {
          key: 'name:dummy warden emblem|emblem — dummy warden',
          name: 'Dummy Warden Emblem',
          type_line: 'Emblem — Dummy Warden',
          card: null,
          sources: [{ card_id: 'planeswalker', name: 'Dummy Warden', quantity: 1 }],
          source_count: 1,
        },
      ],
    })

    const wrapper = mountPanel()
    expect(wrapper.text()).toContain('Dummy Warden Emblem')
    expect(wrapper.text()).toContain('No image')
  })

  it('says a deck makes no tokens only when every card was checked', () => {
    query.data = makePayload({ tokens: [], unchecked_count: 0 })
    expect(mountPanel().text()).toContain("None of this deck's cards make tokens")
  })

  it('reports unchecked cards instead of claiming the deck makes nothing', () => {
    query.data = makePayload({ tokens: [], unchecked_count: 12 })
    const text = mountPanel().text()

    expect(text).not.toContain("None of this deck's cards make tokens")
    expect(text).toContain('12')
    expect(text).toContain('still syncing')
  })

  it('warns that a non-empty list may be short while cards are unchecked', () => {
    query.data = makePayload({ unchecked_count: 3 })
    expect(mountPanel().text()).toContain('may be short')
  })

  it('picks its query by the addressing mode it was mounted with', () => {
    mountPanel()
    expect(query.authedCalls).toBe(1)
    expect(query.publicCalls).toBe(0)

    mountPanel({ handle: 'alice-0001' })
    expect(query.publicCalls).toBe(1)

    mountPanel({ deckId: undefined, preconSlug: 'turtle-power-tmc' })
    expect(query.preconCalls).toBe(1)
  })
})
