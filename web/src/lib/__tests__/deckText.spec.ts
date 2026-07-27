import { describe, expect, it } from 'vitest'
import type { Card, DeckCardEntry } from '@/lib/api'
import { deckListText, entryCopies, sectionHeaderLine, textLines } from '@/lib/deckText'

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

  it('escapes a heading the importer would otherwise read as a card row', () => {
    const cardsBySection = new Map<number, DeckCardEntry[]>([
      [4, [entry(4, 'a', 'Llanowar Elves', 4)]],
    ])
    expect(deckListText([{ id: 4, name: '2 Drops' }], cardsBySection)).toBe(
      '[2 Drops]\n4 Llanowar Elves',
    )
  })
})

// These mirror `renders_ambiguous_section_headers_so_they_round_trip` in
// api/src/deck_import/parser.rs one for one — the API's `render_text_section_header` is the
// authority for this format, and the two must not drift.
describe('sectionHeaderLine', () => {
  it('leaves an unambiguous name bare', () => {
    expect(sectionHeaderLine('Ramp')).toBe('Ramp')
    expect(sectionHeaderLine('Creatures')).toBe('Creatures')
    // No whitespace to split on, so the bare grammar never reads a count here.
    expect(sectionHeaderLine('2Drops')).toBe('2Drops')
  })

  it('brackets a name the bare grammar would claim for something else', () => {
    expect(sectionHeaderLine('2 Drops')).toBe('[2 Drops]')
    expect(sectionHeaderLine('3x Spells')).toBe('[3x Spells]')
    expect(sectionHeaderLine('# Notes')).toBe('[# Notes]')
    expect(sectionHeaderLine('Ramp:')).toBe('[Ramp:]')
    expect(sectionHeaderLine(':')).toBe('[:]')
    expect(sectionHeaderLine('~')).toBe('[~]')
    expect(sectionHeaderLine('2 Drops [v2]')).toBe('[2 Drops [v2]]')
  })

  it('only brackets a leading count the importer would actually keep', () => {
    // The importer clamps to i32 and requires a positive quantity, so these stay card-free
    // and round-trip bare.
    expect(sectionHeaderLine('0 Drops')).toBe('0 Drops')
    expect(sectionHeaderLine('-2 Drops')).toBe('-2 Drops')
  })

  it('is injective, so a literal bracketed name stays distinct from an escaped one', () => {
    expect(sectionHeaderLine('[2 Drops]')).toBe('[[2 Drops]]')
    expect(sectionHeaderLine('[]')).toBe('[[]]')
  })

  it('flattens a stored line break so a heading cannot leak a card-shaped line', () => {
    expect(sectionHeaderLine('Notes\n4 Lightning Bolt')).toBe('Notes 4 Lightning Bolt')
  })
})

describe('entryCopies', () => {
  it('sums the two finishes', () => {
    expect(entryCopies(entry(1, 'a', 'Sol Ring', 2, 1))).toBe(3)
  })
})
