import type { DeckCardEntry, DeckSection, PreconCardEntry } from '@/lib/api'

// A precon's cards, in the shape the deck views already render.
//
// The API states a precon in **boards** (`commander` / `main` / `side`) with one finish per
// row; a deck page renders **sections** with a regular+foil pair per card. Adapting one to
// the other here is what lets the precon page reuse the whole deck display engine —
// `useDeckCardDisplay`'s filters, `DeckSectionNav`, `DeckCardRow`, `DeckTextList`, the card
// size preference — instead of growing a second, subtly different deck renderer.
//
// The board vocabulary is **mirrored** from the API (`entities::precon_deck_card::PreconBoard`),
// like `lifeLayout.ts` mirrors the life-counter layouts and `legality.ts` the format table: a
// board added on one side only would render here as an unnamed bucket. The tests below pin
// the list, so a fourth board fails here rather than showing up blank in the UI.

/** The boards a published decklist is stated in, in reading order. */
export const PRECON_BOARDS = ['commander', 'main', 'side'] as const

export type PreconBoard = (typeof PRECON_BOARDS)[number]

/** Section heading per board. "Deck" rather than "Mainboard" reads right next to a command
 *  zone, and matches how a precon's own packaging describes itself. */
const BOARD_LABEL: Readonly<Record<PreconBoard, string>> = {
  commander: 'Command zone',
  main: 'Deck',
  side: 'Sideboard',
}

/** Synthetic section ids, assigned by board order. A precon has no `deck_sections` rows, but
 *  the deck display engine keys everything on a numeric section id — these are that key, and
 *  they are stable for a given board so a nav anchor doesn't move between renders. */
function boardSectionId(board: PreconBoard): number {
  return PRECON_BOARDS.indexOf(board)
}

/** Whether a board string is one this build knows how to render. */
function isKnownBoard(board: string): board is PreconBoard {
  return (PRECON_BOARDS as readonly string[]).includes(board)
}

/** A precon's cards as deck sections + deck card entries.
 *
 * Only boards that actually carry cards become sections, so a 60-card starter deck shows no
 * empty "Command zone" heading. A precon row's single finish folds into the deck card's
 * regular/foil pair — the same fold the copy endpoint performs server-side, so the page and
 * the deck you copy from it show the same counts. An unknown board (the API added one this
 * build doesn't know) is **kept**, filed under its raw name rather than dropped: a card
 * missing from a decklist is a worse failure than an oddly-named section.
 */
export function preconBoards(cards: PreconCardEntry[]): {
  sections: DeckSection[]
  entries: DeckCardEntry[]
} {
  const entries: DeckCardEntry[] = []
  const seen = new Map<number, string>()
  // Unknown boards get ids past the known ones, in first-seen order.
  const unknownIds = new Map<string, number>()

  for (const card of cards) {
    let sectionId: number
    let name: string
    if (isKnownBoard(card.board)) {
      sectionId = boardSectionId(card.board)
      name = BOARD_LABEL[card.board]
    } else {
      sectionId = unknownIds.get(card.board) ?? PRECON_BOARDS.length + unknownIds.size
      unknownIds.set(card.board, sectionId)
      name = card.board
    }
    seen.set(sectionId, name)
    entries.push({
      card: card.card,
      section_id: sectionId,
      quantity: card.foil ? 0 : card.quantity,
      foil_quantity: card.foil ? card.quantity : 0,
    })
  }

  const sections: DeckSection[] = [...seen.entries()]
    .sort(([a], [b]) => a - b)
    .map(([id, name]) => ({
      id,
      name,
      position: id,
      // Nothing a precon ships is "being considered" — a sideboard is a real sideboard.
      is_maybeboard: false,
    }))
  return { sections, entries }
}
