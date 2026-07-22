import { describe, expect, it } from 'vitest'
import { makeCard } from '@/test/fixtures'
import type { Card, CardFace, DeckCardEntry } from '../api'
import { filterDeckEntries } from '../deckFilter'

function entry(id: string, card: Partial<Card>): DeckCardEntry {
  return { section_id: 1, quantity: 1, foil_quantity: 0, card: makeCard(id, card) }
}

function face(over: Partial<CardFace>): CardFace {
  return {
    name: null,
    mana_cost: null,
    type_line: null,
    oracle_text: null,
    power: null,
    toughness: null,
    loyalty: null,
    ...over,
  }
}

const bolt = entry('bolt', {
  name: 'Lightning Bolt',
  type_line: 'Instant',
  oracle_text: 'Lightning Bolt deals 3 damage to any target.',
  color_identity: ['R'],
})
const island = entry('island', {
  name: 'Island',
  type_line: 'Basic Land — Island',
  color_identity: ['U'],
})
const sol = entry('sol', {
  name: 'Sol Ring',
  type_line: 'Artifact',
  oracle_text: '{T}: Add {C}{C}.',
  color_identity: [],
})
const mdfc = entry('mdfc', {
  name: 'Malakir Rebirth // Malakir Mire',
  type_line: 'Instant // Land',
  color_identity: ['B'],
  faces: [
    face({ name: 'Malakir Rebirth', type_line: 'Instant', oracle_text: 'Choose target creature.' }),
    face({ name: 'Malakir Mire', type_line: 'Land', oracle_text: 'Malakir Mire enters tapped.' }),
  ],
})
const all = [bolt, island, sol, mdfc]

describe('filterDeckEntries', () => {
  it('returns the list unchanged for a blank query and no colours', () => {
    expect(filterDeckEntries(all, '   ', [])).toBe(all)
  })

  it('matches the card name case-insensitively', () => {
    expect(filterDeckEntries(all, 'LIGHTNING', [])).toEqual([bolt])
  })

  it('matches the type line and rules text', () => {
    expect(filterDeckEntries(all, 'artifact', [])).toEqual([sol])
    expect(filterDeckEntries(all, 'damage', [])).toEqual([bolt])
  })

  it('ANDs whitespace-separated tokens', () => {
    expect(filterDeckEntries(all, 'basic island', [])).toEqual([island])
    expect(filterDeckEntries(all, 'basic bolt', [])).toEqual([])
  })

  it('matches the faces of a multi-faced card', () => {
    expect(filterDeckEntries(all, 'tapped', [])).toEqual([mdfc])
  })

  it('ORs selected colour pips over colour identity', () => {
    expect(filterDeckEntries(all, '', ['R'])).toEqual([bolt])
    expect(filterDeckEntries(all, '', ['R', 'U'])).toEqual([bolt, island])
  })

  it('matches colourless cards only via the colourless pip', () => {
    expect(filterDeckEntries(all, '', ['C'])).toEqual([sol])
    expect(filterDeckEntries(all, '', ['B', 'C'])).toEqual([sol, mdfc])
  })

  it('ANDs the text query with the colour selection', () => {
    expect(filterDeckEntries(all, 'instant', ['R'])).toEqual([bolt])
    expect(filterDeckEntries(all, 'instant', ['U'])).toEqual([])
  })
})
