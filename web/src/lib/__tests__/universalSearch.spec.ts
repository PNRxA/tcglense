import { describe, expect, it } from 'vitest'
import type { Card, Deck, KeywordEntry, PreconDeck, Product, SearchResults } from '@/lib/api'
import {
  SEARCH_GROUP_LIMIT,
  buildSearchGroups,
  cardSearchLocation,
  filterDecks,
  matchesEveryWord,
  preconSearchLocation,
  rankByPrefix,
  searchAllOption,
  sealedSearchLocation,
} from '../universalSearch'

function card(overrides: Partial<Card> = {}): Card {
  return {
    id: 'c1',
    name: 'Lightning Bolt',
    set_code: 'lea',
    set_name: 'Limited Edition Alpha',
    collector_number: '161',
    rarity: 'common',
    lang: 'en',
    released_at: null,
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
    prices: { usd: null, usd_foil: null, usd_etched: null, eur: null, tix: null },
    has_image: true,
    drop_name: null,
    drop_slug: null,
    secret_lair_bonus: false,
    secret_lair_spend_incentive: false,
    faces: [],
    legalities: null,
    ...overrides,
  } as Card
}

function product(overrides: Partial<Product> = {}): Product {
  return {
    id: '100',
    name: 'Bloomburrow Play Booster Box',
    set_code: 'blb',
    set_name: 'Bloomburrow',
    product_type: 'play_display',
    url: null,
    has_image: true,
    prices: { usd: null, usd_foil: null },
    msrp: null,
    released_at: null,
    ...overrides,
  }
}

function precon(overrides: Partial<PreconDeck> = {}): PreconDeck {
  return {
    slug: 'squirreled-away-blc',
    game: 'mtg',
    name: 'Squirreled Away',
    set_code: 'blc',
    set_name: 'Bloomburrow Commander',
    deck_type: 'Commander Deck',
    released_at: '2024-08-02',
    color_identity: ['B', 'G'],
    card_count: 100,
    sideboard_count: 0,
    price_usd: null,
    face_card: { card_id: 'face-1', name: 'Hazel of the Rootbloom', has_image: true },
    ...overrides,
  }
}

function keyword(overrides: Partial<KeywordEntry> = {}): KeywordEntry {
  return {
    name: 'Flash',
    slug: 'flash',
    kind: 'ability',
    text: 'You may cast this spell any time you could cast an instant.',
    parameterized: false,
    match_mode: 'anywhere',
    ...overrides,
  }
}

function deck(id: number, name: string, overrides: Partial<Deck> = {}): Deck {
  return {
    id,
    game: 'mtg',
    name,
    description: null,
    format: 'Commander',
    folder_id: null,
    is_public: false,
    card_count: 100,
    color_identity: ['R'],
    commanders: [],
    value_usd: null,
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-01T00:00:00Z',
    ...overrides,
  }
}

/** The trailing row of a group — where a "see all" link sits. */
function last<T>(items: T[] | undefined): T | undefined {
  return items?.[items.length - 1]
}

function results(overrides: Partial<SearchResults> = {}): SearchResults {
  return {
    cards: { data: [], has_more: false },
    products: { data: [], has_more: false },
    precons: { data: [], has_more: false },
    keywords: { data: [], has_more: false },
    ...overrides,
  }
}

describe('matchesEveryWord (the API name rule, mirrored for decks)', () => {
  it('needs every word, in any order, case-insensitively', () => {
    expect(matchesEveryWord('Krenko Goblin Tribal', 'goblin krenko')).toBe(true)
    expect(matchesEveryWord('Krenko Goblin Tribal', 'KRENKO')).toBe(true)
    expect(matchesEveryWord('Krenko Goblin Tribal', 'krenko elves')).toBe(false)
  })

  it('matches substrings, not whole words, and never a blank term', () => {
    expect(matchesEveryWord('Atraxa Superfriends', 'trax')).toBe(true)
    expect(matchesEveryWord('Atraxa Superfriends', '   ')).toBe(false)
    expect(matchesEveryWord('Atraxa Superfriends', '')).toBe(false)
  })
})

describe('rankByPrefix', () => {
  it('leads with names that start with the term, then keeps name order', () => {
    const names = ['Bolt of Lightning', 'Lightning Bolt', 'Alpha Bolt', 'bolt-on']
    expect(rankByPrefix(names, (n) => n, 'bolt')).toEqual([
      'Bolt of Lightning',
      'bolt-on',
      'Alpha Bolt',
      'Lightning Bolt',
    ])
  })

  it('does not mutate its input', () => {
    const names = ['b', 'a']
    rankByPrefix(names, (n) => n, 'a')
    expect(names).toEqual(['b', 'a'])
  })
})

describe('filterDecks', () => {
  it('filters, ranks, and cuts at the group limit', () => {
    const decks = [
      deck(1, 'Elf Ball'),
      deck(2, 'Goblin Elf'),
      deck(3, 'Shelf Life'),
      deck(4, 'Elfen Lied'),
      // "Elves" does not contain "elf" — a substring rule, not a stem.
      deck(5, 'Elves of Deep Shadow'),
      deck(6, 'Nothing here'),
      deck(7, 'Half-Elf Rangers'),
      deck(8, 'Self-Mill'),
    ]
    const { data, hasMore } = filterDecks(decks, 'elf')
    expect(data).toHaveLength(SEARCH_GROUP_LIMIT)
    // Prefix matches first (name order), then the rest.
    expect(data.map((d) => d.name)).toEqual([
      'Elf Ball',
      'Elfen Lied',
      'Goblin Elf',
      'Half-Elf Rangers',
    ])
    expect(hasMore).toBe(true)
  })

  it('reports no remainder when everything fits', () => {
    const { data, hasMore } = filterDecks([deck(1, 'Elf Ball')], 'elf')
    expect(data).toHaveLength(1)
    expect(hasMore).toBe(false)
  })
})

describe('link builders', () => {
  it('hand off to the URL-backed search of each listing', () => {
    expect(cardSearchLocation('mtg', 'sol ring')).toEqual({
      path: '/cards/mtg/cards',
      query: { q: 'sol ring' },
    })
    expect(sealedSearchLocation('mtg', 'bundle')).toEqual({
      path: '/sealed/mtg/products',
      query: { q: 'bundle' },
    })
    expect(preconSearchLocation('mtg', 'commander')).toEqual({
      path: '/decks/mtg/precons/all',
      query: { q: 'commander' },
    })
  })

  it('encodes the game segment', () => {
    expect(cardSearchLocation('a/b', 'x')).toMatchObject({ path: '/cards/a%2Fb/cards' })
  })
})

describe('buildSearchGroups', () => {
  const full = results({
    cards: { data: [card()], has_more: true },
    products: { data: [product()], has_more: true },
    precons: { data: [precon()], has_more: true },
    keywords: { data: [keyword()], has_more: true },
  })

  it('lays the groups out in display order with a row per hit', () => {
    const groups = buildSearchGroups({ game: 'mtg', term: 'bolt', results: full })
    expect(groups.map((g) => g.id)).toEqual(['card', 'product', 'precon', 'keyword'])
    expect(groups.map((g) => g.label)).toEqual([
      'Cards',
      'Sealed products',
      'Preconstructed decks',
      'Keywords',
    ])
  })

  it('links every hit to its own page, with the thumbnail its tile draws', () => {
    const [cards, products, precons, keywords] = buildSearchGroups({
      game: 'mtg',
      term: 'x',
      results: full,
    })

    expect(cards?.options[0]).toMatchObject({
      key: 'card:c1',
      kind: 'card',
      label: 'Lightning Bolt',
      sublabel: 'Instant',
      to: '/cards/mtg/cards/c1',
      thumbnail: { kind: 'card', id: 'c1', hasImage: true },
    })
    expect(products?.options[0]).toMatchObject({
      key: 'product:100',
      label: 'Bloomburrow Play Booster Box',
      sublabel: 'Bloomburrow · Play Booster Box',
      to: '/sealed/mtg/100',
      thumbnail: { kind: 'product', id: '100' },
    })
    expect(precons?.options[0]).toMatchObject({
      key: 'precon:squirreled-away-blc',
      sublabel: 'Commander Deck · Bloomburrow Commander',
      to: '/decks/mtg/precons/squirreled-away-blc',
      thumbnail: { kind: 'card', id: 'face-1', name: 'Hazel of the Rootbloom' },
    })
    expect(keywords?.options[0]).toMatchObject({
      key: 'keyword:flash',
      sublabel: 'Keyword ability',
      to: '/keywords/mtg/flash',
    })
    expect(keywords?.options[0]?.thumbnail).toBeUndefined()
  })

  it('appends a "see all" row to a cut group — except cards, whose link is the closing row', () => {
    const groups = buildSearchGroups({ game: 'mtg', term: 'bolt', results: full })
    const byId = Object.fromEntries(groups.map((g) => [g.id, g]))

    expect(byId.card?.options.map((o) => o.kind)).toEqual(['card'])
    expect(last(byId.product?.options)).toMatchObject({
      kind: 'more',
      key: 'more:product',
      label: 'All sealed products matching “bolt”',
      to: { path: '/sealed/mtg/products', query: { q: 'bolt' } },
    })
    expect(last(byId.precon?.options)).toMatchObject({
      kind: 'more',
      to: { path: '/decks/mtg/precons/all', query: { q: 'bolt' } },
    })
    // The glossary index filters locally, so its "more" is the index itself.
    expect(last(byId.keyword?.options)).toMatchObject({ kind: 'more', to: '/keywords/mtg' })
  })

  it('offers no "see all" row when a group was not cut', () => {
    const groups = buildSearchGroups({
      game: 'mtg',
      term: 'bolt',
      results: results({ products: { data: [product()], has_more: false } }),
    })
    expect(groups).toHaveLength(1)
    expect(groups[0]?.options.map((o) => o.kind)).toEqual(['product'])
  })

  it('drops empty groups, and answers nothing before the first result', () => {
    expect(buildSearchGroups({ game: 'mtg', term: 'zzz', results: results() })).toEqual([])
    expect(buildSearchGroups({ game: 'mtg', term: 'zzz' })).toEqual([])
  })

  it("adds the signed-in user's matching decks second, filtered by the same name rule", () => {
    const decks = [
      deck(1, 'Bolt Storm', {
        commanders: [{ card_id: 'cmd-1', name: 'Krenko' }],
        card_count: 100,
      }),
      deck(2, 'Elves', { format: null, card_count: 1 }),
    ]
    const groups = buildSearchGroups({ game: 'mtg', term: 'bolt', results: full, decks })
    expect(groups.map((g) => g.id)).toEqual(['card', 'deck', 'product', 'precon', 'keyword'])
    const mine = groups[1]
    expect(mine?.label).toBe('Your decks')
    expect(mine?.options).toHaveLength(1)
    expect(mine?.options[0]).toMatchObject({
      key: 'deck:1',
      kind: 'deck',
      label: 'Bolt Storm',
      sublabel: 'Commander · 100 cards',
      to: '/decks/mtg/1',
      thumbnail: { kind: 'card', id: 'cmd-1', name: 'Krenko' },
    })

    // A format-less, one-card deck words its sublabel without a dangling separator.
    const [elves] = buildSearchGroups({ game: 'mtg', term: 'elv', decks }).flatMap((g) => g.options)
    expect(elves?.sublabel).toBe('1 card')
    expect(elves?.thumbnail).toBeUndefined()
  })

  it('cuts the deck group at the limit with a "see all" row', () => {
    const decks = Array.from({ length: SEARCH_GROUP_LIMIT + 1 }, (_, i) => deck(i, `Bolt ${i}`))
    const [mine] = buildSearchGroups({ game: 'mtg', term: 'bolt', decks })
    expect(mine?.options).toHaveLength(SEARCH_GROUP_LIMIT + 1)
    expect(last(mine?.options)).toMatchObject({
      kind: 'more',
      label: 'All your decks',
      to: '/decks/mtg',
    })
  })

  it('leaves the deck group out entirely when signed out', () => {
    const groups = buildSearchGroups({ game: 'mtg', term: 'bolt', results: full })
    expect(groups.some((g) => g.id === 'deck')).toBe(false)
  })
})

describe('searchAllOption', () => {
  it('is the full card search for the typed text', () => {
    expect(searchAllOption('mtg', 'sol ring')).toMatchObject({
      key: 'search:cards',
      kind: 'search',
      label: 'Search all cards for “sol ring”',
      to: { path: '/cards/mtg/cards', query: { q: 'sol ring' } },
    })
  })
})
