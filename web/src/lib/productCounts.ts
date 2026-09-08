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

import type { PackOpening, ProductCardSection, ProductComponent, ProductEv } from '@/lib/api'

/**
 * The sections the product page actually renders. A plain `booster`/`exclusive` section
 * flagged `inherited` is dropped: every one of its cards arrived through a **listed**
 * sub-product (the "What's in the box" list links it), so its pull pool belongs on that
 * product's own page — showing it here doubled the same pool up and read as extra cards
 * (the user clicks through instead). Everything else stays: a component section is never
 * inherited, and an inherited `contains`/`variable` section still states a guarantee this
 * page would otherwise lose. Shared by ProductCards (the section blocks) and
 * ProductOverview (the chips), so the two can never disagree about what's hidden.
 */
export function visibleProductSections(manifest: ProductCardSection[]): ProductCardSection[] {
  return manifest.filter(
    (section) => !(section.inherited && (section.key === 'booster' || section.key === 'exclusive')),
  )
}

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
 * reported as guaranteed. A **component** section (an unlisted sub-product's cards) folds
 * into the same certainty bucket its key names — what a land pack guarantees, the box
 * guarantees. Callers pass the *visible* manifest ({@link visibleProductSections}), so a
 * hidden inherited pool never counts. A card two sub-packs share is counted once per
 * section (each section's own total is exact); the summed bucket can therefore run a hair
 * high across packs — acceptable, where the alternative (deduping) would misstate a pack's
 * own contents.
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

// ---------- Booster expected value + the seeded opener (issue #682) ----------
//
// The booster sheets add two NEW kinds of number to this page, and neither one is a count of
// what a copy contains: an **expectation** (what an average pack is worth over many openings,
// at today's prices) and a **simulation** (what one seeded roll of the dice happened to deal).
// Both are money figures a visitor will read as a promise unless the words around them say
// otherwise, which is why their wording lives here beside the manifest's — the same seam, the
// same rule. Every string below is written so it cannot be read as containment or as a
// guarantee: an average is always said to be an average, a run is always said to be one run,
// and the only per-pack card count on the page ({@link cardsPerPackLabel}) is worded *per
// pack*, never as the product's contents.

/** The EV panel's heading: the title, the unit its money figure is in, and the sentence that
 * keeps that figure honest. */
export type ProductEvHeading = { title: string; unit: string; blurb: string }

/** The half of the blurb every EV heading carries, whatever the unit: what the number IS
 * (an average, at today's prices) and what it is NOT (this pack's worth). */
const EV_AVERAGE_BLURB =
  "An average over many openings at today's prices — not what any one pack holds."

/**
 * Word the expected-value headline for one copy of the product. The unit is the whole
 * distinction: a single booster's EV is quoted **per pack**, while a box's is **per copy** —
 * and a per-copy figure is meaningless until the reader knows how many packs a copy opens, so
 * the blurb says so outright. Neither form ever states a copy's *contents*.
 */
export function expectedValueHeading(ev: ProductEv): ProductEvHeading {
  const packs = ev.packs.reduce((sum, pack) => sum + Math.max(0, pack.quantity), 0)
  const title = 'Expected value'
  if (packs === 1) return { title, unit: 'per pack, on average', blurb: EV_AVERAGE_BLURB }
  // 0 packs shouldn't reach here (the API answers `null` for a product with no boosters), but
  // "one copy opens 0 packs" would be a worse thing to print than nothing at all.
  const opens = packs > 1 ? `One copy opens ${packs.toLocaleString()} packs. ` : ''
  return { title, unit: 'per copy, on average', blurb: `${opens}${EV_AVERAGE_BLURB}` }
}

/** The EV read against what the product actually costs today. */
export type EvVersusPrice = { ratio: number; label: string }

/**
 * Compare the expected value with the current market price, as a share of it — never as a
 * gain, a loss, or anything a reader could act on as advice ("you'll get back", "profit"):
 * an expectation over many openings says nothing about the copy in front of them, and the
 * label keeps the word "Expected" in front of the number so the comparison inherits it.
 * `null` when the product has no price to compare against.
 */
export function evVersusPrice(
  evUsd: string,
  priceUsd: string | null | undefined,
): EvVersusPrice | null {
  if (!priceUsd) return null
  const ev = Number(evUsd)
  const price = Number(priceUsd)
  if (!Number.isFinite(ev) || !Number.isFinite(price) || price <= 0) return null
  const ratio = ev / price
  return { ratio, label: `Expected value is ${Math.round(ratio * 100)}% of the current price` }
}

/**
 * How often a card is pulled, from the API's `one_in` ("one in N packs, on average").
 * Under 10 the figure keeps a decimal — the difference between one pack in 2 and one in 2.5
 * is the whole answer at that end — and above it rounds to a whole number, where a decimal
 * would imply a precision the with-replacement approximation doesn't have. At 1.5 or under
 * the ratio stops being informative ("1 in 1.2 packs" reads as a guarantee), so it's worded
 * as frequency instead.
 */
export function oddsLabel(oneIn: number): string {
  if (!Number.isFinite(oneIn)) return 'rarely'
  if (oneIn <= 0) return 'rarely'
  if (oneIn <= 1.5) return 'most packs'
  const rounded = oneIn < 10 ? Math.round(oneIn * 10) / 10 : Math.round(oneIn)
  return `1 in ${rounded.toLocaleString()} packs`
}

/**
 * The one legitimate per-pack card count on a sealed product's page: it comes from the
 * booster's own sheet configuration, so it says how many cards a pack deals — and it is
 * worded **per pack** for exactly that reason, never as "cards in this product". A booster
 * whose variants deal different numbers of cards has a fractional expectation, which is
 * rendered as the range it really is (`14–15 cards per pack`) rather than a fake decimal.
 * Empty string when there is no count to state, so the caller renders nothing.
 */
/**
 * A `0..1` priced share as the "N% of picks priced" annotation, shown only below one. It
 * mirrors the API's own `percent` guard at both ends: a share short of one is never printed
 * as `100%` (the line only exists because something is unpriced, so "100% of picks priced"
 * would contradict the caveat beside it — `>99%` is what a 99.7% share honestly is), and a
 * non-zero share is never `0%`. Empty string at or above one, where the caller shows nothing.
 */
export function pricedShareLabel(share: number): string {
  if (!Number.isFinite(share) || share >= 1) return ''
  if (share <= 0) return '0% of picks priced'
  const pct = share * 100
  if (pct < 0.5) return '<1% of picks priced'
  if (pct >= 99.5) return '>99% of picks priced'
  return `${Math.round(pct)}% of picks priced`
}

export function cardsPerPackLabel(cardsPerPack: number): string {
  if (!Number.isFinite(cardsPerPack) || cardsPerPack <= 0) return ''
  const rounded = Math.round(cardsPerPack)
  if (Math.abs(cardsPerPack - rounded) < 0.005) {
    return `${rounded.toLocaleString()} ${rounded === 1 ? 'card' : 'cards'} per pack`
  }
  return `${Math.floor(cardsPerPack).toLocaleString()}–${Math.ceil(
    cardsPerPack,
  ).toLocaleString()} cards per pack`
}

/**
 * A booster's display name for a heading — upstream's own key (`play`, `collector`) when it
 * states none. Shared by the EV panel and the opener so one booster is never named two ways
 * on the same page.
 */
export function boosterLabel(pack: { name: string | null; booster_code: string }): string {
  return pack.name ?? `${pack.booster_code} booster`
}

/** The opener's result heading: what the run was, the caption its money figure carries, and
 * the sentence that stops that figure being read as the product's worth. */
export type OpeningSummary = { title: string; value: string; blurb: string }

/**
 * Word a simulated opening's total. Everything here is past tense and singular to this run —
 * "dealt", "this run", "one roll of the dice" — because the number is a sample of one, and a
 * lucky roll printed as a bare total is the most misleading figure this page could show. When
 * some pulls had no market price the blurb says how many, so the total is never read as
 * complete.
 */
export function openingSummary(opening: PackOpening): OpeningSummary {
  const packs = opening.packs.length
  const dealt = opening.priced_count + opening.unpriced_count
  const title =
    packs === 1
      ? 'What this run dealt'
      : `What this run dealt across ${packs.toLocaleString()} packs`
  const parts = ['This run is one roll of the dice, not what a pack is worth.']
  if (opening.unpriced_count > 0) {
    parts.push(
      `${opening.unpriced_count.toLocaleString()} of the ${dealt.toLocaleString()} cards dealt had no market price and count as $0.`,
    )
  }
  return { title, value: 'pulled in this run', blurb: parts.join(' ') }
}

/**
 * The run's total read against what those copies cost at today's price — the same "share of"
 * framing {@link evVersusPrice} uses, and for the same reason: a total is not a promise. The
 * subject stays *this run* throughout, because a simulation's total says even less about the
 * next copy than an expectation does. `null` when the product has no price to compare with.
 */
export function openingVersusPrice(
  opening: PackOpening,
  priceUsd: string | null | undefined,
): string | null {
  if (!priceUsd) return null
  const price = Number(priceUsd)
  const value = Number(opening.value_usd)
  const copies = Math.max(1, opening.copies)
  if (!Number.isFinite(price) || price <= 0 || !Number.isFinite(value)) return null
  const share = Math.round((value / (price * copies)) * 100)
  return copies === 1
    ? `This run dealt ${share}% of what one copy costs at today's price`
    : `This run dealt ${share}% of what ${copies.toLocaleString()} copies cost at today's price`
}
