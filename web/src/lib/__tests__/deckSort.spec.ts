import { describe, expect, it } from 'vitest'
import { DECK_DEFAULT_SORT, DECK_SORT_OPTIONS, sortDecks } from '../deckSort'
import type { Deck } from '@/lib/api'

let nextId = 1
function deck(overrides: Partial<Deck>): Deck {
  return {
    id: nextId++,
    game: 'mtg',
    name: `Deck ${nextId}`,
    description: null,
    format: null,
    folder_id: null,
    is_public: false,
    card_count: 0,
    color_identity: null,
    commanders: [],
    value_usd: null,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...overrides,
  }
}

describe('sortDecks', () => {
  it('passes the painted order through untouched on the default sort', () => {
    const decks = [deck({ name: 'Zoo' }), deck({ name: 'Aristocrats' })]
    expect(sortDecks(decks, DECK_DEFAULT_SORT).map((d) => d.name)).toEqual(['Zoo', 'Aristocrats'])
  })

  it('returns a new array rather than mutating the pinned order', () => {
    const decks = [deck({ name: 'B' }), deck({ name: 'A' })]
    const sorted = sortDecks(decks, 'name')
    expect(sorted).not.toBe(decks)
    expect(decks.map((d) => d.name)).toEqual(['B', 'A'])
  })

  it('orders by name with the id as a stable tie-break', () => {
    const first = deck({ name: 'Same' })
    const second = deck({ name: 'Same' })
    const other = deck({ name: 'Aggro' })
    expect(sortDecks([second, other, first], 'name')).toEqual([other, first, second])
  })

  it('orders by price high to low, parking unpriced decks last', () => {
    const cheap = deck({ name: 'Cheap', value_usd: '12.34' })
    const dear = deck({ name: 'Dear', value_usd: '150.00' })
    const unpriced = deck({ name: 'Unpriced', value_usd: null })
    // "$0.00" is a priced deck — it must still beat an unpriced one, the same
    // null-vs-zero distinction the API keeps.
    const worthless = deck({ name: 'Worthless', value_usd: '0.00' })
    expect(sortDecks([unpriced, cheap, worthless, dear], 'price').map((d) => d.name)).toEqual([
      'Dear',
      'Cheap',
      'Worthless',
      'Unpriced',
    ])
  })

  it('breaks price ties by name so a refetch cannot reshuffle equals', () => {
    const b = deck({ name: 'Beta', value_usd: '5.00' })
    const a = deck({ name: 'Alpha', value_usd: '5.00' })
    expect(sortDecks([b, a], 'price').map((d) => d.name)).toEqual(['Alpha', 'Beta'])
  })

  it('offers exactly the sorts the menu renders, led by the default', () => {
    expect(DECK_SORT_OPTIONS.map((o) => o.value)).toEqual(['updated', 'name', 'price'])
    expect(DECK_SORT_OPTIONS[0]?.value).toBe(DECK_DEFAULT_SORT)
  })
})
