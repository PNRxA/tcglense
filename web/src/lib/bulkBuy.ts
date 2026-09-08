import type { BuyList, BuyListCard, BuyListProduct } from '@/lib/api'
import type { SupportedCurrency } from '@/lib/currency'

// "Buy all" (issue #292): turn the wish list's shopping list — the API's `BuyList`
// rows, each with its counts and the printing's TCGplayer product id — into one option
// per bulk-entry store. Two kinds of store exist, and the distinction is the whole point:
//
// - a **link** store takes the list in its URL (TCGplayer's Mass Entry: `?c=` holds the
//   rows, `||`-separated, each `{qty}-{productId}` or, for a printing TCGplayer doesn't
//   list, `{qty} Name [SET] number` — the format read off TCGplayer's own page bundle, and
//   pinned by the spec), so the button is a plain outbound link with the cart prefilled;
// - a **paste** store offers no URL prefill we could verify (MTG Mate's decklist search —
//   a bot wall blocked fetching its form), so the option copies a `{qty} Name` list to
//   the clipboard and opens the page for the user to paste into — an honest paste, never
//   a link that silently lands on an empty box.
//
// The rows come from the API and the stores live here (the same split as buyLinks.ts):
// a store added to this registry needs nothing server-side. Keyed by game slug; an
// unknown game offers nothing.

export interface BulkBuyLink {
  kind: 'link'
  name: string
  region: string
  href: string
  /** What the button does, in a sentence — shown beside it. */
  note: string
}

export interface BulkBuyPaste {
  kind: 'paste'
  name: string
  region: string
  /** The page to open once the list is on the clipboard. */
  href: string
  /** The list to copy, one `{qty} Name` line per card. */
  list: string
  note: string
}

export type BulkBuyOption = BulkBuyLink | BulkBuyPaste

export const TCGPLAYER_MASS_ENTRY_URL = 'https://www.tcgplayer.com/massentry'
export const MTG_MATE_DECKLIST_URL = 'https://www.mtgmate.com.au/cards/decklist_search'

// TCGplayer's `productline` value per game slug.
const TCGPLAYER_PRODUCT_LINES: Record<string, string> = { mtg: 'Magic' }

// Copies wanted of a row, both finishes: a bulk-entry page picks the finish itself
// (TCGplayer's mass entry selects printings on the page, not in the URL), so the two
// counts merge into one line.
function copies(row: { quantity: number; foil_quantity: number }): number {
  return row.quantity + row.foil_quantity
}

// One TCGplayer mass-entry row: by product id when the catalog holds one (the exact
// printing, no name matching), else by name + set + collector number, which the page
// resolves itself. Un-encoded; `tcgplayerMassEntryUrl` encodes each row as TCGplayer's
// own share-link builder does.
export function massEntryRow(card: BuyListCard): string {
  const qty = copies(card)
  if (card.tcgplayer_id != null) return `${qty}-${card.tcgplayer_id}`
  return `${qty} ${card.name} [${card.set_code.toUpperCase()}] ${card.collector_number}`
}

export function massEntryProductRow(product: BuyListProduct): string {
  return `${copies(product)}-${product.product_id}`
}

// The mass-entry link: every card row then every sealed-product row, each
// percent-encoded, joined by a literal `||` (TCGplayer splits on the raw delimiter).
export function tcgplayerMassEntryUrl(list: BuyList, productLine: string): string {
  const rows = [
    ...list.cards.filter((card) => copies(card) > 0).map(massEntryRow),
    ...list.products.filter((product) => copies(product) > 0).map(massEntryProductRow),
  ]
  const c = rows.map(encodeURIComponent).join('||')
  return `${TCGPLAYER_MASS_ENTRY_URL}?c=${c}&productline=${encodeURIComponent(productLine)}`
}

// A plain decklist: `{qty} Name` per card, folded by name (two printings of one card are
// one line to a store that stocks singles by name) in first-seen order. Sealed products
// are left out — a decklist box takes singles.
export function decklistText(cards: BuyListCard[]): string {
  const byName = new Map<string, number>()
  for (const card of cards) {
    const qty = copies(card)
    if (qty <= 0) continue
    byName.set(card.name, (byName.get(card.name) ?? 0) + qty)
  }
  return [...byName].map(([name, qty]) => `${qty} ${name}`).join('\n')
}

// How many card rows TCGplayer receives by exact product id versus by name — said
// beside the button so a list with unlisted printings isn't a surprise on the page.
export function massEntryNote(list: BuyList): string {
  const cards = list.cards.filter((card) => copies(card) > 0)
  const byId = cards.filter((card) => card.tcgplayer_id != null).length
  const byName = cards.length - byId
  const parts = [`Opens Mass Entry with your list filled in`]
  if (byName > 0) {
    parts.push(
      `${byId} of ${cards.length} printings by exact product, ${byName} by name for TCGplayer to match`,
    )
  }
  if (list.products.length > 0) {
    parts.push(
      `${list.products.length} sealed ${list.products.length === 1 ? 'product' : 'products'} included`,
    )
  }
  return `${parts.join(' — ')}.`
}

const MTG_OPTIONS = (list: BuyList): BulkBuyOption[] => [
  {
    kind: 'link',
    name: 'TCGplayer',
    region: 'Global',
    href: tcgplayerMassEntryUrl(list, TCGPLAYER_PRODUCT_LINES.mtg ?? 'Magic'),
    note: massEntryNote(list),
  },
  {
    kind: 'paste',
    name: 'MTG Mate',
    region: 'Australia',
    href: MTG_MATE_DECKLIST_URL,
    list: decklistText(list.cards),
    note: 'Copies the card list, then opens MTG Mate’s decklist search for you to paste it into (singles only).',
  },
]

const OPTIONS_BY_GAME: Record<string, (list: BuyList) => BulkBuyOption[]> = {
  mtg: MTG_OPTIONS,
}

// Currencies whose holder most likely shops in Australia — their store leads the list.
const AUSTRALIA_FIRST: ReadonlySet<SupportedCurrency> = new Set(['AUD', 'NZD'])

export function bulkBuyOptionsFor(
  game: string,
  list: BuyList,
  currency: SupportedCurrency = 'USD',
): BulkBuyOption[] {
  const options = OPTIONS_BY_GAME[game]?.(list) ?? []
  if (!AUSTRALIA_FIRST.has(currency)) return options
  // A stable partition: the Australian options first, everything else in registry order.
  return [
    ...options.filter((option) => option.region === 'Australia'),
    ...options.filter((option) => option.region !== 'Australia'),
  ]
}
