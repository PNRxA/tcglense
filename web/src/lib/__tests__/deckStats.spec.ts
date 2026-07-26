import { describe, expect, it } from 'vitest'
import type { Card, DeckCardEntry } from '../api'
import { calculateDeckStats, defaultDrawSectionIds, drawProbability } from '../deckStats'

function entry(id: string, name: string, quantity: number, card: Partial<Card>): DeckCardEntry {
  return {
    section_id: 1,
    quantity,
    foil_quantity: 0,
    card: {
      id,
      name,
      color_identity: [],
      cmc: null,
      type_line: null,
      ...card,
    } as Card,
  }
}

describe('deck stats', () => {
  it('weights colour, type, and mana curve breakdowns by copies', () => {
    const stats = calculateDeckStats([
      entry('cat', 'Sky Cat', 2, {
        color_identity: ['W', 'U'],
        cmc: 3,
        type_line: 'Artifact Creature — Cat',
      }),
      entry('land', 'Island', 4, {
        color_identity: ['U'],
        cmc: 0,
        type_line: 'Basic Land — Island',
      }),
    ])

    expect(stats.totalCopies).toBe(6)
    expect(stats.uniqueCards).toBe(2)
    expect(stats.landCopies).toBe(4)
    expect(stats.averageManaValue).toBe(3)
    expect(stats.manaCurve[3]?.count).toBe(2)
    expect(stats.colors.find((color) => color.key === 'U')?.count).toBe(6)
    expect(stats.cardTypes.find((type) => type.key === 'Creature')?.count).toBe(2)
    expect(stats.cardTypes.find((type) => type.key === 'Artifact')?.count).toBe(2)
  })

  it('calculates the chance of drawing at least one copy', () => {
    expect(drawProbability(60, 4, 7)).toBeCloseTo(0.3995, 3)
    expect(drawProbability(60, 60, 1)).toBe(1)
    expect(drawProbability(0, 4, 7)).toBe(0)
  })

  it('excludes command-zone, out-of-game, and maybeboard sections from draw odds by default', () => {
    expect(
      defaultDrawSectionIds([
        { id: 1, name: 'Commander', is_maybeboard: false },
        { id: 2, name: 'Creatures', is_maybeboard: false },
        { id: 3, name: 'Sideboard', is_maybeboard: false },
        { id: 4, name: 'Maybeboard', is_maybeboard: true },
        { id: 5, name: 'Signature Spells', is_maybeboard: false },
        { id: 6, name: 'Custom library pile', is_maybeboard: false },
        { id: 7, name: 'Cut candidates', is_maybeboard: true },
        { id: 8, name: 'Command Zone', is_maybeboard: false },
      ]),
    ).toEqual([2, 6])
  })

  it('judges a maybeboard by its flag, never by its name', () => {
    // A section merely *called* "Considering" that the owner kept in the deck is a library
    // section — the old name heuristic silently excluded it.
    expect(defaultDrawSectionIds([{ id: 1, name: 'Considering', is_maybeboard: false }])).toEqual([
      1,
    ])
    // ...and a renamed maybeboard stays excluded, which no name list could manage.
    expect(defaultDrawSectionIds([{ id: 1, name: 'Cuts', is_maybeboard: true }])).toEqual([])
  })
})
