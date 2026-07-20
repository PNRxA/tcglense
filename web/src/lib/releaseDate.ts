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
  const date = new Date(raw)
  if (Number.isNaN(date.getTime())) return null
  const upcoming = date.getTime() > Date.now()
  const when = date.toLocaleDateString(undefined, { year: 'numeric', month, day: 'numeric' })
  return { label: `${upcoming ? 'Releases' : 'Released'} ${when}`, upcoming }
}
