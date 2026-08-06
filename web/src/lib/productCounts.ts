// The counting + labelling seam for a sealed product's card manifest and its box composition,
// shared by ProductCards (the heading) and ProductOverview (the chips) so the two can never
// disagree about what a number means.
//
// The one rule everything here exists to enforce: **no number on a sealed-product page is a
// count of physical cards.** The API has no such datum — `sealed_contents` has no quantity
// column (a precon's 30 Forests are one row) and no pack size is ingested. A section `total`
// counts *distinct catalog cards* in a pool; `component.quantity` counts *pieces in the box*.
// Those are different kinds of number, and the copy built here keeps them visibly different —
// a booster's ~600-card pull pool must never be worded as "600 cards in this product".

import type { ProductCardSection, ProductComponent } from '@/lib/api'

/** Distinct-card counts per certainty. `pool` and `variable` are disjoint; `exclusive` is a
 * **subset** of `pool`, never added to it. */
export type ProductCardCounts = {
  /** `contains` — in every copy (as modelled: at least one; the quantity is unknown). */
  guaranteed: number
  /** `exclusive` + `booster` — the pool a copy's boosters draw a random subset from. */
  pool: number
  /** The slice of `pool` no other booster family in the set can produce. */
  exclusive: number
  /** `variable` — a randomized / either-or configuration. */
  variable: number
  /** Everything not guaranteed. */
  possible: number
  /** Every distinct card, at any certainty. */
  total: number
}

/**
 * Fold a (possibly search-filtered) sections manifest into per-certainty counts. An
 * unrecognised section key falls into `variable`, the weakest claim — mirroring the server's
 * `CardSection::classify`, so a section key the SPA hasn't caught up with can never be
 * reported as guaranteed.
 */
export function productCardCounts(manifest: ProductCardSection[]): ProductCardCounts {
  let guaranteed = 0
  let pool = 0
  let exclusive = 0
  let variable = 0
  for (const section of manifest) {
    const n = Math.max(0, section.total)
    if (section.key === 'contains') guaranteed += n
    else if (section.key === 'exclusive') {
      pool += n
      exclusive += n
    } else if (section.key === 'booster') pool += n
    else variable += n
  }
  return {
    guaranteed,
    pool,
    exclusive,
    variable,
    possible: pool + variable,
    total: guaranteed + pool + variable,
  }
}

/** The cards section's `<h2>`: a noun matching the strongest claim the product actually makes,
 * the count carrying the unit that number really has, and — for a mixed product only — one
 * line reconciling the two certainties its single number spans. */
export type ProductCardsHeading = { title: string; count: string; blurb: string }

/**
 * Word the heading by which certainties are present. `filtered` says a card search is narrowing
 * the manifest, which changes what the number *is*: a match count rather than a pool size. No
 * form needs singular/plural inflection — `(1)` and `(1-card pool)` both read correctly at one.
 */
export function productCardsHeading(
  counts: ProductCardCounts,
  filtered = false,
): ProductCardsHeading {
  const n = counts.total.toLocaleString()
  // Search-only state (the section stays mounted so the filter can be cleared): nothing
  // matched, so no certainty is known — claim none. A bare "Cards in this product (0)" would
  // read on a booster as "this product has no cards" rather than "your search found none".
  if (counts.total === 0) return { title: 'Cards', count: '(0)', blurb: '' }
  // Guaranteed-only (a precon deck, a Secret Lair, a fixed-promo product) — containment is
  // true, so the original wording stands.
  if (counts.possible === 0) return { title: 'Cards in this product', count: `(${n})`, blurb: '' }
  if (counts.guaranteed === 0) {
    // A pure pull pool — the case that read "600 cards in this product" for a 15-card booster.
    if (counts.variable === 0) {
      return {
        title: 'What you can pull',
        // The unit goes *inside* the parenthesis so the number can't be read as copies — but
        // "-card pool" is a claim about the pool's SIZE, so a search drops it: those N are the
        // cards that matched, not the pool.
        count: filtered ? `(${n})` : `(${n}-card pool)`,
        blurb: '',
      }
    }
    return { title: 'What you might get', count: `(${n})`, blurb: '' }
  }
  // Mixed: one heading spanning two certainties, so it gets the page's only extra line — and it
  // spells the split out, because a bundle's single total otherwise hides a ~600-card pull pool
  // inside a number that looks like contents (1 promo + 600 pool reads as "601 cards").
  const parts = [`${counts.guaranteed.toLocaleString()} guaranteed`]
  if (counts.pool > 0) parts.push(`${counts.pool.toLocaleString()} in the pull pool`)
  if (counts.variable > 0) parts.push(`${counts.variable.toLocaleString()} sometimes included`)
  return {
    title: "What's guaranteed, what's random",
    count: `(${n})`,
    blurb:
      counts.pool > 0
        ? `${parts.join(' · ')} — a copy opens some of the pool, not all of it.`
        : `${parts.join(' · ')}.`,
  }
}

/** One at-a-glance chip. `id` picks the icon in ProductOverview; `hint` is appended to the
 * button's tooltip; `aria` replaces the visible label in the accessible name when that label
 * leans on the chip beside it (a screen reader may read the button alone). */
export type ProductCardChip = {
  id: 'guaranteed' | 'pull' | 'exclusive' | 'variable'
  count: number
  label: string
  aria?: string
  hint: string
}

/**
 * The card chips, in descending order of certainty, already filtered to the non-empty ones.
 * Each certainty gets its own chip over a **disjoint** count, so no two chips can be read as
 * adding up wrongly — except the exclusives chip, deliberately phrased "of them …" so it
 * reads as a slice of the pull chip that always precedes it. It's suppressed when the whole
 * pool is exclusive, where it would merely restate that chip. `family` is the exclusives'
 * booster-family label, or null.
 */
export function productCardChips(
  counts: ProductCardCounts,
  family: string | null,
): ProductCardChip[] {
  const chips: ProductCardChip[] = []
  if (counts.guaranteed > 0)
    chips.push({
      id: 'guaranteed',
      count: counts.guaranteed,
      label: counts.guaranteed === 1 ? 'guaranteed card' : 'guaranteed cards',
      hint: 'distinct cards — extra copies of the same card count once',
    })
  if (counts.pool > 0)
    chips.push({
      id: 'pull',
      // "cards in the pull pool", not "cards you can pull" — the latter parses as "you can
      // pull 600 cards", which is the very claim this whole change exists to stop making.
      count: counts.pool,
      label: counts.pool === 1 ? 'card in the pull pool' : 'cards in the pull pool',
      hint: "the whole pool these boosters draw from, not one pack's worth",
    })
  if (counts.exclusive > 0 && counts.exclusive < counts.pool)
    chips.push({
      id: 'exclusive',
      count: counts.exclusive,
      label: family ? `of them exclusive to ${family}` : 'of them booster-exclusive',
      // "of them" back-references the pull chip visually; read alone by a screen reader it has
      // no antecedent, so the accessible name names the pool outright.
      aria: family
        ? `of the pull pool, exclusive to ${family}`
        : "of the pull pool, exclusive to this product's boosters",
      hint: 'a slice of the pull pool, not extra cards',
    })
  if (counts.variable > 0)
    chips.push({
      id: 'variable',
      count: counts.variable,
      label: counts.variable === 1 ? 'card it might include' : 'cards it might include',
      hint: 'a randomized configuration — a copy holds some of these, not all',
    })
  return chips
}

/**
 * How many physical pieces the box holds: the sum of the component quantities, not the number
 * of line items — a booster box is one `30× Play Booster` row plus a topper, i.e. 31 items,
 * not 2. Clamped like the API's own `quantity >= 1`. This is the only count on the page that
 * *is* a count of things you physically get, and it still says nothing about cards.
 */
export function boxItemCount(components: ProductComponent[]): number {
  return components.reduce((sum, component) => sum + Math.max(1, component.quantity), 0)
}
