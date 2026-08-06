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

  // Any heading whose single number spans more than one certainty spells the split out beneath
  // itself, because no per-section blurb below can reconcile a total that pools them. This is
  // built before the branches on purpose: a collector box is routinely pool + a randomized
  // insert with nothing guaranteed, and gating the line on `guaranteed > 0` would drop the pool
  // framing from exactly the product the reported bug was about (600 pool + 2 inserts must not
  // collapse back into one undifferentiated "602").
  const parts: string[] = []
  if (counts.guaranteed > 0) parts.push(`${counts.guaranteed.toLocaleString()} guaranteed`)
  if (counts.pool > 0) parts.push(`${counts.pool.toLocaleString()} in the pull pool`)
  if (counts.variable > 0) parts.push(`${counts.variable.toLocaleString()} sometimes included`)
  const split =
    parts.length < 2
      ? ''
      : counts.pool > 0
        ? `${parts.join(' · ')} — a copy opens some of the pool, not all of it.`
        : `${parts.join(' · ')}.`

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
    // Pool and/or randomized maybes, nothing guaranteed. The count carries no unit (it spans
    // two kinds of "maybe"), so the split below is what keeps the pool legible.
    return { title: 'What you might get', count: `(${n})`, blurb: split }
  }
  return { title: "What's guaranteed, what's random", count: `(${n})`, blurb: split }
}

/** One at-a-glance chip. `id` picks the icon in ProductOverview; `hint` is appended to the
 * button's tooltip. Every label is self-contained: the strip is `flex-wrap`, so a label that
 * leaned on the chip beside it would lose its antecedent the moment the row wrapped — and a
 * screen reader reads each button alone regardless. */
export type ProductCardChip = {
  id: 'guaranteed' | 'pull' | 'exclusive' | 'variable'
  count: number
  label: string
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
      // "of the pool", not "of them" — the chip names what it is a slice of, so it survives the
      // row wrapping away from the pull chip (and being read on its own).
      label: family
        ? `of the pool, exclusive to ${family}`
        : "of the pool, exclusive to this product's boosters",
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
 * not 2. This is the only count on the page that *is* a count of things you physically get,
 * and it still says nothing about cards.
 *
 * A malformed row contributes nothing rather than being clamped up to one: the count's whole
 * job is to agree with the `N×` the rows underneath it render, so counting a `0×` row as an
 * item would break it in exactly the case the clamp was meant to cover.
 */
export function boxItemCount(components: ProductComponent[]): number {
  return components.reduce((sum, component) => sum + Math.max(0, component.quantity), 0)
}
