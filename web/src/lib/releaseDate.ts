// Shared release-date labelling. A date in the future reads as "Releases …", a past one as
// "Released …", so an upcoming set, Secret Lair drop, or card printing shows when it's *due*
// rather than claiming it already came out. Used by the set page's by-drop headers and the
// card / sealed-product detail headers.

export interface ReleaseLabel {
  /** e.g. "Releases Jul 25, 2026" (future) or "Released Jul 25, 2026" (past). */
  label: string
  /** Whether the date is in the future. */
  upcoming: boolean
}

/**
 * A human release label that flips its verb by tense: "Releases {date}" for a future date,
 * "Released {date}" for a past one. Returns `null` for a missing or unparseable date so callers
 * can drop the chip entirely. `month` picks the date style ('long' → "July 25, 2026", 'short' →
 * "Jul 25, 2026").
 */
export function formatReleaseLabel(
  raw: string | null | undefined,
  month: 'short' | 'long' = 'long',
): ReleaseLabel | null {
  if (!raw) return null
  const date = parseReleaseDate(raw)
  if (!date || Number.isNaN(date.getTime())) return null
  const upcoming = date.getTime() > Date.now()
  const when = date.toLocaleDateString(undefined, { year: 'numeric', month, day: 'numeric' })
  return { label: `${upcoming ? 'Releases' : 'Released'} ${when}`, upcoming }
}

// `released_at` is a date-only string ('YYYY-MM-DD'). `new Date('2026-08-01')` parses it as UTC
// midnight, which then renders and tense-compares a day early for any viewer west of UTC (e.g.
// showing "Released July 31" for an Aug 1 release that hasn't happened). Parse a date-only value
// as *local* midnight so both the displayed day and the future/past verb match the user's
// calendar. Anything with a time component (or non-matching) falls back to the native parser.
function parseReleaseDate(raw: string): Date | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(raw.trim())
  if (m) return new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]))
  const date = new Date(raw)
  return Number.isNaN(date.getTime()) ? null : date
}
