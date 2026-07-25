import { describe, expect, it } from 'vitest'
import type { Card, DeckCardEntry } from '@/lib/api'
import { deckListText, entryCopies, textLines } from '@/lib/deckText'

function entry(
  sectionId: number,
  id: string,
  name: string,
  quantity: number,
  foilQuantity = 0,
): DeckCardEntry {
  return {
    section_id: sectionId,
    quantity,
    foil_quantity: foilQuantity,
    card: { id, name } as Card,
  }
}

describe('textLines', () => {
  it('counts regular and foil copies as one line', () => {
    expect(textLines([entry(1, 'a', 'Sol Ring', 1, 2)])).toEqual([{ name: 'Sol Ring', copies: 3 }])
  })

  it('folds separate printings of a card into a single line', () => {
    expect(
      textLines([
        entry(1, 'bolt-lea', 'Lightning Bolt', 3),
        entry(1, 'bolt-m10', 'Lightning Bolt', 1),
      ]),
    ).toEqual([{ name: 'Lightning Bolt', copies: 4 }])
  })

  it('drops entries with no copies', () => {
    expect(textLines([entry(1, 'a', 'Ghost', 0), entry(1, 'b', 'Real', 1)])).toEqual([
      { name: 'Real', copies: 1 },
    ])
  })
})

describe('deckListText', () => {
  const sections = [
    { id: 1, name: 'Creatures' },
    { id: 2, name: 'Empty' },
    { id: 3, name: 'Lands' },
  ]

  it('renders each populated section as a heading plus its lines', () => {
    const cardsBySection = new Map<number, DeckCardEntry[]>([
      [1, [entry(1, 'a', 'Birds of Paradise', 1), entry(1, 'b', 'Llanowar Elves', 4)]],
      [2, []],
      [3, [entry(3, 'c', 'Forest', 12)]],
    ])
    expect(deckListText(sections, cardsBySection)).toBe(
      'Creatures\n1 Birds of Paradise\n4 Llanowar Elves\n\nLands\n12 Forest',
    )
  })

  it('is empty when nothing has copies, so the copy button has nothing to write', () => {
    expect(deckListText(sections, new Map())).toBe('')
  })
})

describe('entryCopies', () => {
  it('sums the two finishes', () => {
    expect(entryCopies(entry(1, 'a', 'Sol Ring', 2, 1))).toBe(3)
  })
})
