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
  set_code: 'lea',
  set_name: 'Limited Edition Alpha',
  collector_number: '161',
})
const island = entry('island', {
  name: 'Island',
  type_line: 'Basic Land — Island',
  color_identity: ['U'],
  set_code: 'dmu',
  set_name: 'Dominaria United',
  collector_number: '0001',
})
const sol = entry('sol', {
  name: 'Sol Ring',
  type_line: 'Artifact',
  oracle_text: '{T}: Add {C}{C}.',
  color_identity: [],
  set_code: 'c21',
  set_name: 'Commander 2021',
  collector_number: '333',
  rarity: 'uncommon',
})
const mdfc = entry('mdfc', {
  name: 'Malakir Rebirth // Malakir Mire',
  type_line: 'Instant // Land',
  color_identity: ['B'],
  set_code: 'znr',
  set_name: 'Zendikar Rising',
  collector_number: '18a',
  rarity: 'mythic',
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

  it('matches set codes, set names, and rarity like the printing picker', () => {
    expect(filterDeckEntries(all, 'lea', [])).toEqual([bolt])
    expect(filterDeckEntries(all, 'zendikar', [])).toEqual([mdfc])
    expect(filterDeckEntries(all, 'mythic', [])).toEqual([mdfc])
  })

  it('treats a bare number as an exact collector-number lookup, never a substring', () => {
    expect(filterDeckEntries(all, '161', [])).toEqual([bolt])
    expect(filterDeckEntries(all, '16', [])).toEqual([])
    // Leading zeros ignored on both sides; sol's "333" must not swallow it either.
    expect(filterDeckEntries(all, '1', [])).toEqual([island])
  })

  it('matches letter-suffixed collector numbers and standalone set-name years', () => {
    expect(filterDeckEntries(all, '#18a', [])).toEqual([mdfc])
    expect(filterDeckEntries(all, '2021', [])).toEqual([sol])
  })

  it('matches a standalone number in rules text without substring noise', () => {
    const burn = entry('burn', {
      name: 'Crater Blast',
      type_line: 'Sorcery',
      oracle_text: 'Crater Blast deals 30 damage to each creature.',
      color_identity: ['R'],
      set_code: 'tsr',
      set_name: 'Test Reforged',
      collector_number: '77',
    })
    const pool = [...all, burn]
    // "deals 3 damage" matches; "deals 30 damage" must not (word boundary, not substring).
    expect(filterDeckEntries(pool, '3', [])).toEqual([bolt])
    expect(filterDeckEntries(pool, '30', [])).toEqual([burn])
  })

  it('keeps a #-prefixed number a pure collector-number lookup', () => {
    expect(filterDeckEntries(all, '#161', [])).toEqual([bolt])
    // Never falls through to rules text: "#3" must not surface "deals 3 damage".
    expect(filterDeckEntries(all, '#3', [])).toEqual([])
  })
})
