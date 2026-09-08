// The copy-count + finish filter on the collection / wish-list holdings lists (issue #677):
// "which cards do I own more than four of?", "which playsets am I one short of?", "foils I
// own". Pure grammar + prose, shared by `useCopiesFilter` (URL ↔ state), the query hooks
// (wire params) and `CopiesFilterMenu` (the chip's options and its trigger label).
//
// These are NOT part of the Scryfall `q=` grammar and must never be folded into it — they
// ride as their own `min_copies` / `max_copies` / `finish` query params, which the holdings
// listings honour and the public catalog endpoints (the show-ghosts mode's source) do not.

/** Which counter the copy bounds read. `any` = regular + foil; `regular` / `foil` read that
 * finish's count alone AND require at least one copy of it — so `foil` on its own means
 * "cards I hold any foil of". Matches the API's `finish=` token set. */
export type HoldingFinish = 'any' | 'regular' | 'foil'

/** The committed copy-count filter: an optional inclusive `[min, max]` bound on the copies
 * held, read through `finish`. */
export interface CopiesFilter {
  min?: number
  max?: number
  finish: HoldingFinish
}

/** Just the bounds half — what the `?copies=` token parses into. */
export interface CopiesBounds {
  min?: number
  max?: number
}

/** No filter at all: unbounded, both finishes. */
export const EMPTY_COPIES_FILTER: CopiesFilter = { finish: 'any' }

/** Whether a filter actually narrows the listing (a bound, or a finish other than `any`). */
export function isCopiesFilterActive(filter: CopiesFilter): boolean {
  return filter.min != null || filter.max != null || filter.finish !== 'any'
}

/** A whole, non-negative count, or null for anything else. */
function readCount(raw: string): number | null {
  if (!/^\d+$/.test(raw)) return null
  const n = Number(raw)
  return Number.isSafeInteger(n) ? n : null
}

/**
 * Parse the `?copies=` URL token into its bounds. The grammar is `N` (exactly N), `N-M`
 * (N to M), `N-` (at least N) and `-M` (at most M); anything else — junk, a negative, a
 * `max < min` — parses to no bounds at all rather than a filter the server would 422 on.
 *
 * Tolerant on purpose: a hand-typed `?copies=5+` arrives URL-decoded as `5 ` (a bare `+`
 * in a query string *is* a space), so a trailing `+` or space both read as "at least" —
 * but only over a token that could carry one (`5`, `5-`); everything else is merely
 * trimmed, so a padded `  2-3  ` still reads as the range it spells.
 */
export function parseCopiesToken(raw: string | undefined): CopiesBounds {
  if (typeof raw !== 'string') return {}
  const suffixed = /[+\s]$/.test(raw)
  const token = raw.replace(/[+\s]+$/, '').trim()
  if (!token) return {}
  if (suffixed && /^\d+-?$/.test(token)) {
    const min = readCount(token.replace(/-+$/, ''))
    return min == null ? {} : { min }
  }
  const exact = readCount(token)
  if (exact != null) return { min: exact, max: exact }
  const range = /^(\d*)-(\d*)$/.exec(token)
  if (!range) return {}
  const [, rawMin, rawMax] = range
  const min = rawMin ? readCount(rawMin) : undefined
  const max = rawMax ? readCount(rawMax) : undefined
  if (min === null || max === null) return {}
  if (min == null && max == null) return {}
  if (min != null && max != null && max < min) return {}
  return { ...(min != null ? { min } : {}), ...(max != null ? { max } : {}) }
}

/** Format bounds back into the canonical `?copies=` token — `undefined` when unbounded (so
 * the key drops out of the URL), and for an incoherent `max < min` pair. Round-trips with
 * {@link parseCopiesToken}. */
export function copiesToken(min?: number, max?: number): string | undefined {
  if (min == null && max == null) return undefined
  if (min != null && max != null) {
    if (max < min) return undefined
    return min === max ? String(min) : `${min}-${max}`
  }
  return min != null ? `${min}-` : `-${max}`
}

/** Read the `?finish=` URL token; anything unknown (or absent) is the unfiltered `any`. */
export function parseFinish(raw: unknown): HoldingFinish {
  return raw === 'regular' || raw === 'foil' ? raw : 'any'
}

/** How the chip's number is compared against the copies held. The URL and the wire only
 * know inclusive bounds, so each comparison is spelt as a bound pair: `gt N` is `min N+1`,
 * `lt N` is `max N-1`, and `between` is the two-sided range a hand-typed `?copies=2-3`
 * already means. */
export type CopiesComparator = 'eq' | 'gte' | 'gt' | 'lte' | 'lt' | 'between'

/** The comparator options the chip offers, in the order it lists them. */
export const COPIES_COMPARATORS: readonly { value: CopiesComparator; label: string }[] = [
  { value: 'eq', label: 'Exactly' },
  { value: 'gte', label: 'At least' },
  { value: 'gt', label: 'More than' },
  { value: 'lte', label: 'At most' },
  { value: 'lt', label: 'Less than' },
  { value: 'between', label: 'Between' },
]

/** A typed comparison: the comparator, its number, and — for `between` only — the upper
 * end of the range. */
export interface CopiesComparison {
  comparator: CopiesComparator
  value: number
  upper?: number
}

/**
 * Turn a comparison into the inclusive bounds the URL / wire carry, or null when it can't
 * be one: a non-integer or negative number, "less than 0" (nothing holds fewer than none),
 * or a `between` whose upper end is missing or below its lower. `gt` and `lt` shift by one
 * because the bounds are inclusive.
 */
export function boundsFromComparison(comparison: CopiesComparison): CopiesBounds | null {
  const { comparator, value, upper } = comparison
  const whole = (n: number | undefined): n is number =>
    n != null && Number.isSafeInteger(n) && n >= 0
  if (!whole(value)) return null
  switch (comparator) {
    case 'eq':
      return { min: value, max: value }
    case 'gte':
      return { min: value }
    case 'gt':
      return { min: value + 1 }
    case 'lte':
      return { max: value }
    case 'lt':
      return value === 0 ? null : { max: value - 1 }
    case 'between':
      if (!whole(upper) || upper < value) return null
      return { min: value, max: upper }
  }
}

/**
 * Read bounds back into the comparison the chip's form shows — the canonical spelling, so
 * `more than 4` re-opens as `at least 5` (the URL keeps only the bounds). Unbounded reads
 * as a blank `gte` (the form's default comparator with no number).
 */
export function comparisonFromBounds(bounds: CopiesBounds): CopiesComparison | null {
  const { min, max } = bounds
  if (min == null && max == null) return null
  if (min != null && max != null) {
    return min === max
      ? { comparator: 'eq', value: min }
      : { comparator: 'between', value: min, upper: max }
  }
  if (min != null) return { comparator: 'gte', value: min }
  return { comparator: 'lte', value: max as number }
}

/** Which counter the bounds read — the chip's finish toggle. */
export const FINISH_OPTIONS: readonly { value: HoldingFinish; label: string }[] = [
  { value: 'any', label: 'Regular or foil' },
  { value: 'regular', label: 'Regular only' },
  { value: 'foil', label: 'Foil only' },
]

/** The wire params for a holdings listing / export. `finish` is omitted at its `any`
 * default (the server's own default), and a `0` bound is meaningful so both numbers ride
 * whenever they're set. */
export function copiesFilterParams(filter: CopiesFilter): {
  minCopies?: number
  maxCopies?: number
  finish?: HoldingFinish
} {
  return {
    ...(filter.min != null ? { minCopies: filter.min } : {}),
    ...(filter.max != null ? { maxCopies: filter.max } : {}),
    ...(filter.finish !== 'any' ? { finish: filter.finish } : {}),
  }
}

/** The prose the count line and the chip trigger read — "4 foil copies", "5 or more
 * copies", "up to 3 copies", "regular copies" — or null when nothing is filtered. */
export function describeCopiesFilter(filter: CopiesFilter): string | null {
  if (!isCopiesFilterActive(filter)) return null
  const finish = filter.finish === 'any' ? '' : `${filter.finish} `
  const { min, max } = filter
  // A bare finish filter has no quantity to word: "foil copies" = any foil at all.
  if (min == null && max == null) return `${finish}copies`
  if (min != null && max != null) {
    if (min === max) return `${min} ${finish}${min === 1 ? 'copy' : 'copies'}`
    return `${min}–${max} ${finish}copies`
  }
  if (min != null) return `${min} or more ${finish}copies`
  return `up to ${max} ${finish}${max === 1 ? 'copy' : 'copies'}`
}
