import type { BreakdownBucket, HoldingBreakdown } from '@/lib/api'

// The holdings breakdown panel's wording (issue #680): how each facet's bucket keys read
// and what colour their bars take. The keys are the server's
// (`handlers/shared/breakdown.rs` — rarity in Scryfall's spelling, the seven colour
// buckets, the type line's first card type lower-cased, `regular`/`foil`); only the labels
// and swatches are decided here, so an unknown key still renders as itself.

/** The four facets, in the order the panel's switch offers them. */
export type BreakdownFacet = 'rarity' | 'color' | 'card_type' | 'finish'

export const BREAKDOWN_FACETS: { key: BreakdownFacet; label: string }[] = [
  { key: 'rarity', label: 'Rarity' },
  { key: 'color', label: 'Colour' },
  { key: 'card_type', label: 'Type' },
  { key: 'finish', label: 'Finish' },
]

/** The seven colour-identity buckets, in the server's order, with their wording and the
 * mana swatch each bar takes. The hexes are the same literal MTG mana colours the deck
 * composition bars draw (server-supplied there) — domain data, not a state colour, which
 * is why they're not design-system tokens. Multicolour is the gold of a gold card. */
const COLOR_BUCKETS: Record<string, { label: string; color: string }> = {
  white: { label: 'White', color: '#e5e7eb' },
  blue: { label: 'Blue', color: '#3b82f6' },
  black: { label: 'Black', color: '#374151' },
  red: { label: 'Red', color: '#ef4444' },
  green: { label: 'Green', color: '#22c55e' },
  multicolor: { label: 'Multicolour', color: '#eab308' },
  colorless: { label: 'Colourless', color: '#a1a1aa' },
}

/** Rarity bars take the design system's rarity tokens; common stays the muted ink, and
 * anything the token vocabulary doesn't name (special, bonus, unknown) the accent. */
const RARITY_COLORS: Record<string, string> = {
  common: 'var(--muted-foreground)',
  uncommon: 'var(--rarity-uncommon)',
  rare: 'var(--rarity-rare)',
  mythic: 'var(--rarity-mythic)',
}

/** The foil finish takes the foil token; regular the accent. */
const FINISH_COLORS: Record<string, string> = {
  regular: 'var(--primary)',
  foil: 'var(--foil)',
}

/** Capitalise a lower-case key for display (`mythic` → `Mythic`, `unknown` stays a word). */
function titleCase(key: string): string {
  return key.charAt(0).toUpperCase() + key.slice(1)
}

/** The wording for one bucket key of a facet. */
export function bucketLabel(facet: BreakdownFacet, key: string): string {
  switch (facet) {
    case 'color':
      return COLOR_BUCKETS[key]?.label ?? titleCase(key)
    case 'rarity':
      return key === 'unknown' ? 'Unknown rarity' : titleCase(key)
    case 'card_type':
      return key === 'other' ? 'Other' : titleCase(key)
    case 'finish':
      return key === 'regular' ? 'Regular' : titleCase(key)
  }
}

/** The bar colour for one bucket key of a facet — a CSS colour value. */
export function bucketColor(facet: BreakdownFacet, key: string): string {
  switch (facet) {
    case 'color':
      return COLOR_BUCKETS[key]?.color ?? 'var(--primary)'
    case 'rarity':
      return RARITY_COLORS[key] ?? 'var(--primary)'
    case 'finish':
      return FINISH_COLORS[key] ?? 'var(--primary)'
    case 'card_type':
      return 'var(--primary)'
  }
}

/** One bar the panel draws: the bucket dressed with its label, colour and its share of
 * the facet's priced value (0–1; every bar 0 when nothing in the facet is priced). */
export interface BreakdownBar {
  key: string
  label: string
  color: string
  cards: number
  copies: number
  /** The bucket's raw USD value string (for the money formatter), null when unpriced. */
  valueUsd: string | null
  /** The bucket's fraction of the facet's priced total, 0–1. */
  share: number
}

/** Parse a server USD string to a number (0 for null/unparseable). */
function usd(value: string | null): number {
  if (!value) return 0
  const n = Number(value)
  return Number.isFinite(n) ? n : 0
}

/** Dress a facet's buckets as bars, in the server's order (which is the display order for
 * every facet: rarity/colour canonical, type most valuable first, finish regular then
 * foil). Shares are of the facet's own priced total, so the longest bar is not always
 * full-width — a facet whose value is spread evenly reads as such. */
export function breakdownBars(facet: BreakdownFacet, buckets: BreakdownBucket[]): BreakdownBar[] {
  const total = buckets.reduce((acc, bucket) => acc + usd(bucket.value_usd), 0)
  return buckets.map((bucket) => ({
    key: bucket.key,
    label: bucketLabel(facet, bucket.key),
    color: bucketColor(facet, bucket.key),
    cards: bucket.cards,
    copies: bucket.copies,
    valueUsd: bucket.value_usd,
    share: total > 0 ? usd(bucket.value_usd) / total : 0,
  }))
}

/** Whether a breakdown has anything to draw: at least one bucket somewhere. An empty
 * holding answers empty facets everywhere, and the panel hides rather than draws a blank. */
export function hasBreakdown(breakdown: HoldingBreakdown | undefined): boolean {
  if (!breakdown) return false
  return BREAKDOWN_FACETS.some(({ key }) => breakdown[key].length > 0)
}
