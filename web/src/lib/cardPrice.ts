// Picking the single USD price to surface for a card in a compact list/tile.
// Some printings are sold only as foils (no regular USD price), so we fall back
// to the foil price rather than showing nothing — mirroring the backend's
// price-sort fallback (`price_usd` then `price_usd_foil`) so the shown price and
// the price sort agree.

export interface DisplayPrice {
  /** USD amount, exactly as stored (a decimal string). */
  amount: string
  /** True when `amount` is the foil price, shown only because the card has no
   * regular USD price (a foil-only printing). */
  foil: boolean
}

/** The USD fields both `CardPrices` and `ProductPrices` carry — the only ones this
 * picker reads, so it serves cards and sealed products alike. */
export interface UsdPrices {
  usd: string | null
  usd_foil: string | null
}

/** The USD price to show for a card or product in a list/tile: the regular price if
 * present, otherwise the foil price; null when neither is priced. */
export function displayUsdPrice(prices: UsdPrices): DisplayPrice | null {
  if (prices.usd) return { amount: prices.usd, foil: false }
  if (prices.usd_foil) return { amount: prices.usd_foil, foil: true }
  return null
}
