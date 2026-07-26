import type { DeckCardEntry, DeckSection } from '@/lib/api'

// The deck as plain text (issue #570) — what the "text" view shows on screen and what its
// copy button puts on the clipboard. Deliberately the dumbest possible decklist format,
// `<copies> <name>` under a bare section heading, because that's the lingua franca every
// deck site accepts on paste (and what this repo's own text importer parses back).
//
// Not the same thing as the *export* endpoint, which emits printing-exact rows (set code,
// collector number, finish) so a deck round-trips losslessly. This is for a human pasting
// into a forum post or a search box, so it names cards and nothing else.

/** Total copies of an entry, regular + foil — the text view never splits the two. */
export function entryCopies(entry: DeckCardEntry): number {
  return entry.quantity + entry.foil_quantity
}

/**
 * Fold a section's entries down to one line per card *name*, in the order given. Separate
 * printings of the same card collapse into a single line: a text list is about what the
 * deck plays, and "3 Lightning Bolt / 1 Lightning Bolt" reads as an error.
 */
export function textLines(entries: DeckCardEntry[]): Array<{ name: string; copies: number }> {
  const byName = new Map<string, number>()
  for (const entry of entries) {
    const copies = entryCopies(entry)
    if (copies <= 0) continue
    byName.set(entry.card.name, (byName.get(entry.card.name) ?? 0) + copies)
  }
  return [...byName.entries()].map(([name, copies]) => ({ name, copies }))
}

/**
 * The whole deck as a paste-ready string: each section's name, then its lines, with a blank
 * line between sections. Sections with no cards are skipped entirely (a heading with nothing
 * under it is noise on the clipboard).
 */
export function deckListText(
  sections: Array<Pick<DeckSection, 'id' | 'name'>>,
  cardsBySection: Map<number, DeckCardEntry[]>,
): string {
  const blocks: string[] = []
  for (const section of sections) {
    const lines = textLines(cardsBySection.get(section.id) ?? [])
    if (lines.length === 0) continue
    blocks.push([section.name, ...lines.map((line) => `${line.copies} ${line.name}`)].join('\n'))
  }
  return blocks.join('\n\n')
}
