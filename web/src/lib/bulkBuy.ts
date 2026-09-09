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

// The most characters the encoded `?c=` list may run to. The API caps the list at 500
// card rows, but a row for a printing TCGplayer doesn't list falls back to a name form
// that encodes to 40–70 characters, so rows alone don't bound the link; 7 000 keeps the
// whole URL under the ~8 KB request-line ceiling common to reverse proxies, whatever
// the mix. Rows past the budget are left off and the option's note says how many.
export const MASS_ENTRY_URL_BUDGET = 7_000

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

export interface MassEntryLink {
  href: string
  /** Rows that fit the URL budget, in order. */
  sent: number
  /** Rows left off because the link would have run past `MASS_ENTRY_URL_BUDGET`. */
  dropped: number
}

// The mass-entry link: every card row then every sealed-product row, each
// percent-encoded, joined by a literal `||` (TCGplayer splits on the raw delimiter) —
// up to the URL budget, past which the remaining rows are dropped and counted.
export function tcgplayerMassEntryLink(list: BuyList, productLine: string): MassEntryLink {
  const rows = [
    ...list.cards.filter((card) => copies(card) > 0).map(massEntryRow),
    ...list.products.filter((product) => copies(product) > 0).map(massEntryProductRow),
  ].map(encodeURIComponent)
  const kept: string[] = []
  let length = 0
  for (const row of rows) {
    const next = length + row.length + (kept.length ? 2 : 0)
    if (next > MASS_ENTRY_URL_BUDGET) break
    kept.push(row)
    length = next
  }
  const c = kept.join('||')
  return {
    href: `${TCGPLAYER_MASS_ENTRY_URL}?c=${c}&productline=${encodeURIComponent(productLine)}`,
    sent: kept.length,
    dropped: rows.length - kept.length,
  }
}

export function tcgplayerMassEntryUrl(list: BuyList, productLine: string): string {
  return tcgplayerMassEntryLink(list, productLine).href
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

// How many card rows TCGplayer receives by exact product id versus by name, and how many
// the URL budget left off — said beside the button so neither is a surprise on the page.
export function massEntryNote(list: BuyList, link: MassEntryLink): string {
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
  if (link.dropped > 0) {
    parts.push(
      `the last ${link.dropped} ${link.dropped === 1 ? 'row' : 'rows'} left off (the link would be too long — narrow the list to send them)`,
    )
  }
  return `${parts.join(' — ')}.`
}

// One line on what the list holds: "12 cards (18 copies) and 2 sealed products".
export function buyListSummary(list: BuyList): string {
  const cardCopies = list.cards.reduce((sum, row) => sum + copies(row), 0)
  const cards = `${list.cards.length.toLocaleString()} ${list.cards.length === 1 ? 'card' : 'cards'} (${cardCopies.toLocaleString()} ${cardCopies === 1 ? 'copy' : 'copies'})`
  if (list.products.length === 0) return cards
  const products = `${list.products.length.toLocaleString()} sealed ${list.products.length === 1 ? 'product' : 'products'}`
  return `${cards} and ${products}`
}

// What the API's cap cut, worded per list — the card rows and the sealed products are
// capped independently, so a products-only cut must not read as "card rows". `null` when
// nothing was cut.
export function truncationNote(list: BuyList): string | null {
  if (!list.truncated) return null
  const cardsCut = list.total_cards - list.cards.length
  const productsCut = list.total_products - list.products.length
  const parts: string[] = []
  if (cardsCut > 0) {
    parts.push(
      `only the first ${list.cards.length.toLocaleString()} card rows are sent (${cardsCut.toLocaleString()} more on the list)`,
    )
  }
  if (productsCut > 0) {
    parts.push(
      `only the first ${list.products.length.toLocaleString()} sealed products are sent (${productsCut.toLocaleString()} more on the list)`,
    )
  }
  if (parts.length === 0) return null
  const sentence = parts.join(' and ')
  return `${sentence.charAt(0).toUpperCase()}${sentence.slice(1)}. Narrow the list with a search or a set to buy the rest.`
}

const MTG_OPTIONS = (list: BuyList): BulkBuyOption[] => {
  const massEntry = tcgplayerMassEntryLink(list, TCGPLAYER_PRODUCT_LINES.mtg ?? 'Magic')
  return [
    {
      kind: 'link',
      name: 'TCGplayer',
      region: 'Global',
      href: massEntry.href,
      note: massEntryNote(list, massEntry),
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
}

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
