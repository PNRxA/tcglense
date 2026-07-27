import type { DeckCardEntry, DeckSection } from '@/lib/api'

// The deck as plain text (issue #570) — what the "text" view shows on screen and what its
// copy button puts on the clipboard. Deliberately the dumbest possible decklist format,
// `<copies> <name>` under a section heading, because that's the lingua franca every deck
// site accepts on paste (and what this repo's own text importer parses back — headings the
// bare grammar would misread get the API's bracket escape, see `sectionHeaderLine`).
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

/** Characters the importer's header trim set eats off either edge (`~ / : [ ]`). */
const HEADER_EDGE = ['~', '/', ':', '[', ']']

/** Unicode whitespace as the importer splits on it — JS `\s` plus NEL, which it omits. */
const WHITESPACE = /[\s\u0085]/

/** Largest quantity the importer keeps; it clamps to i32 before testing for a card row. */
const MAX_QUANTITY = 2147483647

/**
 * Does the bare grammar read this name's first token as a card count? Mirrors the importer:
 * split on the first whitespace, drop a trailing `x`/`X` ("3x Spells"), then require a plain
 * integer that clamps to something positive.
 */
function hasLeadingQuantity(name: string): boolean {
  const split = name.search(WHITESPACE)
  if (split < 0) return false
  const token = name.slice(0, split).replace(/[xX]+$/, '')
  if (!/^[+-]?\d+$/.test(token)) return false
  return Math.min(Math.max(Number(token), 0), MAX_QUANTITY) > 0
}

/**
 * Render `name` as a header line the importer reads back as the same section — the mirror of
 * the API's `render_text_section_header` (api/src/deck_import/parser.rs), which this format
 * has to agree with because the export endpoint and this copy button feed the same parser.
 *
 * A bare name is ambiguous when the grammar claims it for something else: a leading positive
 * quantity ("2 Drops") reads as a card row, a leading `#` as a comment, and edge characters
 * from the trim set (`~ / : [ ]`) get eaten. Those wrap in one bracket pair, which the
 * importer's `custom_section_header` strips back off verbatim — so the escape is injective and
 * a literal "[2 Drops]" stays distinct from the escaped form of "2 Drops". Interior line
 * breaks flatten to spaces so a header can never leak an extra card-shaped line into the list.
 */
export function sectionHeaderLine(name: string): string {
  const flat = name.replace(/[\r\n]/g, ' ')
  const ambiguous =
    hasLeadingQuantity(flat) ||
    flat.startsWith('#') ||
    HEADER_EDGE.some((char) => flat.startsWith(char) || flat.endsWith(char))
  return ambiguous ? `[${flat}]` : flat
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
    blocks.push(
      [sectionHeaderLine(section.name), ...lines.map((line) => `${line.copies} ${line.name}`)].join(
        '\n',
      ),
    )
  }
  return blocks.join('\n\n')
}
