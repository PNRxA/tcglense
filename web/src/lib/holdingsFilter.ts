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

/** The copy-count presets the filter chip offers, as `?copies=` tokens (`''` = no bound).
 * Deliberately a short curated ladder rather than two number inputs: these are the
 * questions the filter exists to answer. */
export const COPIES_PRESETS: readonly { value: string; label: string }[] = [
  { value: '', label: 'Any count' },
  { value: '1', label: '1 copy' },
  { value: '2-3', label: '2–3 copies' },
  { value: '4', label: 'Playset (4)' },
  { value: '5-', label: '5 or more' },
]

/** Which counter the bounds read — the chip's second radio group. */
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
