import { describe, expect, it } from 'vitest'
import type { Card, PreconCardEntry } from '@/lib/api'
import { PRECON_BOARDS, preconBoards } from '@/lib/precons'

function entry(board: string, id: string, quantity: number, foil = false): PreconCardEntry {
  return {
    board,
    quantity,
    foil,
    card: { id, name: `Card ${id}` } as Card,
  }
}

describe('preconBoards', () => {
  it('names each board and orders them command zone, deck, sideboard', () => {
    const { sections } = preconBoards([
      entry('side', 'c', 2),
      entry('main', 'b', 20),
      entry('commander', 'a', 1, true),
    ])
    expect(sections.map((s) => s.name)).toEqual(['Command zone', 'Deck', 'Sideboard'])
    // `position` mirrors the id so the section nav's order matches the render order.
    expect(sections.map((s) => s.position)).toEqual([0, 1, 2])
    expect(sections.every((s) => !s.is_maybeboard)).toBe(true)
  })

  it('creates no section for a board the deck has no cards on', () => {
    const { sections } = preconBoards([entry('main', 'a', 1)])
    expect(sections.map((s) => s.name)).toEqual(['Deck'])
  })

  it("folds a precon row's single finish into the deck entry's two counts", () => {
    const { entries } = preconBoards([entry('commander', 'a', 1, true), entry('main', 'b', 3)])
    expect(entries[0]).toMatchObject({ quantity: 0, foil_quantity: 1, section_id: 0 })
    expect(entries[1]).toMatchObject({ quantity: 3, foil_quantity: 0, section_id: 1 })
  })

  it('folds the two finishes of one printing on a board into a single entry', () => {
    // The Jumpstart / bundle-land-pack shape: upstream lists the printing twice, once per
    // finish. Two entries here would render two tiles under one `v-for` key — and would not
    // match the deck the copy endpoint writes, which folds them.
    const { entries } = preconBoards([
      entry('main', 'a', 20),
      entry('main', 'a', 1, true),
      entry('main', 'b', 1),
    ])
    expect(entries).toHaveLength(2)
    expect(entries[0]).toMatchObject({ quantity: 20, foil_quantity: 1 })
    expect(entries[1]!.card.id).toBe('b')
  })

  it('keeps the same printing apart when it sits on different boards', () => {
    const { entries } = preconBoards([entry('commander', 'a', 1, true), entry('main', 'a', 1)])
    expect(entries).toHaveLength(2)
    expect(entries.map((e) => e.section_id)).toEqual([0, 1])
  })

  it('keeps a card on an unrecognised board rather than dropping it', () => {
    // The API grew a board this build doesn't know: a decklist missing cards is worse than
    // an oddly-named section.
    const { sections, entries } = preconBoards([entry('main', 'a', 1), entry('planes', 'b', 1)])
    expect(sections.map((s) => s.name)).toEqual(['Deck', 'planes'])
    expect(entries).toHaveLength(2)
    expect(entries[1]!.section_id).toBe(PRECON_BOARDS.length)
  })

  it('handles an empty decklist', () => {
    expect(preconBoards([])).toEqual({ sections: [], entries: [] })
  })
})

describe('PRECON_BOARDS', () => {
  // Mirrored from the API's `PreconBoard` (api/src/entities/precon_deck_card.rs). A board
  // added there and not here renders under its raw slug; pinning the list makes that a
  // failing test rather than a quiet UI regression.
  it('matches the API board vocabulary', () => {
    expect([...PRECON_BOARDS]).toEqual(['commander', 'main', 'side'])
  })
})

// The server synthesises the same sections for its analysis reads (`handlers/precons/analysis.rs`
// `SECTIONS`), and the stats panel round-trips their ids: it builds its library checkboxes from
// THIS side's sections but ticks them against the server's `default_library_section_ids` and
// echoes them back as `?sections=`. A divergence is not a type error — it is every checkbox
// unchecked and a filter that matches nothing. Pinned on both sides, like `lifeLayout.ts`.
describe('precon section vocabulary (mirrored server-side)', () => {
  it('assigns the ids and names handlers/precons/analysis.rs assigns', () => {
    const { sections } = preconBoards([
      entry('side', 'c', 1),
      entry('main', 'b', 1),
      entry('commander', 'a', 1),
    ])
    expect(sections.map((s) => [s.id, s.name])).toEqual([
      [0, 'Command zone'],
      [1, 'Deck'],
      [2, 'Sideboard'],
    ])
  })

  it('keys ids to the board, not to which boards are present', () => {
    // A precon with no command zone still gives its mainboard id 1 — the server does the
    // same, so a `?sections=1` round-trip means the same thing on both sides.
    const { sections } = preconBoards([entry('main', 'b', 1), entry('side', 'c', 1)])
    expect(sections.map((s) => s.id)).toEqual([1, 2])
    expect(PRECON_BOARDS.indexOf('main')).toBe(1)
  })
})
