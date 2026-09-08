import type { ReleaseCalendar, ReleaseWindow, SecretLairDropRelease, SetRelease } from '@/lib/api'
import { parseReleaseDate } from '@/lib/releaseDate'

// The release calendar's pure half (issue #679): where the page lives, which window it asks
// the API for, and how the API's two date-ascending lists (sets, Secret Lair drops) fold into
// the month sections the view renders. Nothing here is reactive — the view marries these to a
// query — and nothing here invents a date: every entry's `date` is the catalog's own.

/** Where a game's release calendar lives. The nav registry, the all-games hub and the sitemap
 *  all build it from here. */
export function releasesPath(game: string): string {
  return `/releases/${encodeURIComponent(game)}`
}

/** The element id of the alert settings' release heads-up section — what the calendar's
 *  "get a heads-up" button deep-links to. Owned here so the button and the section can't
 *  drift apart on the spelling. */
export const RELEASE_HEADS_UP_ANCHOR = 'release-headsups'

/** The deep link itself: the alert settings page, scrolled to the release opt-ins. */
export const RELEASE_HEADS_UP_PATH = `/alerts#${RELEASE_HEADS_UP_ANCHOR}`

/** Months before the current one the window reaches back — "what just landed". */
export const MONTHS_BACK = 1

/** Months after the current one the window reaches forward — "what's coming". Together
 *  with `MONTHS_BACK` that's seven calendar months, comfortably inside the API's 366-day cap. */
export const MONTHS_AHEAD = 5

/** A date-only `YYYY-MM-DD` for a local calendar day (never `toISOString`, which would shift
 *  the day for any viewer not on UTC). */
export function isoDate(date: Date): string {
  const y = date.getFullYear()
  const m = String(date.getMonth() + 1).padStart(2, '0')
  const d = String(date.getDate()).padStart(2, '0')
  return `${y}-${m}-${d}`
}

/**
 * The window the calendar asks for, **month-aligned**: the first day of the month
 * `MONTHS_BACK` months ago through the last day of the month `MONTHS_AHEAD` months ahead.
 * Aligned to month boundaries — rather than "today ± N days" — so the request URL, and the
 * CDN entry behind it, is the same for every visitor for a whole calendar month.
 */
export function calendarWindow(today: Date): ReleaseWindow {
  const from = new Date(today.getFullYear(), today.getMonth() - MONTHS_BACK, 1)
  // Day 0 of the month after the last one is that last month's final day.
  const to = new Date(today.getFullYear(), today.getMonth() + MONTHS_AHEAD + 1, 0)
  return { from: isoDate(from), to: isoDate(to) }
}

/** One calendar entry: a set (with what it ships) or a Secret Lair drop, dated. */
export type CalendarEntry =
  | { kind: 'set'; key: string; date: string; release: SetRelease }
  | { kind: 'drop'; key: string; date: string; release: SecretLairDropRelease }

/** One month section of the calendar. Every month inside the window is present — an empty
 *  one renders as "nothing scheduled" rather than vanishing, so a gap reads as a fact. */
export interface CalendarMonth {
  /** `YYYY-MM`, the section key. */
  key: string
  /** e.g. "September 2026", in the viewer's locale. */
  label: string
  /** Whether this is the viewer's current month. */
  current: boolean
  entries: CalendarEntry[]
}

/** The `YYYY-MM` of a `YYYY-MM-DD`. */
export function monthKey(iso: string): string {
  return iso.slice(0, 7)
}

/** The locale label of a `YYYY-MM` month. */
export function monthLabel(key: string): string {
  const [y, m] = key.split('-').map(Number)
  return new Date(y ?? 0, (m ?? 1) - 1, 1).toLocaleDateString(undefined, {
    month: 'long',
    year: 'numeric',
  })
}

/** Every `YYYY-MM` from the month of `from` through the month of `to`, inclusive. */
export function monthKeysBetween(from: string, to: string): string[] {
  const start = parseReleaseDate(from)
  const end = parseReleaseDate(to)
  if (!start || !end || end < start) return []
  const keys: string[] = []
  const cursor = new Date(start.getFullYear(), start.getMonth(), 1)
  const last = new Date(end.getFullYear(), end.getMonth(), 1)
  while (cursor <= last) {
    keys.push(monthKey(isoDate(cursor)))
    cursor.setMonth(cursor.getMonth() + 1)
  }
  return keys
}

/**
 * Fold the API's two lists into month sections, oldest month first, entries date-ascending
 * within a month. On a shared day a set precedes a drop (a set is the bigger landmark), then
 * by name. Every month of the window is present even when empty.
 */
export function calendarMonths(calendar: ReleaseCalendar, today: Date): CalendarMonth[] {
  const entries: CalendarEntry[] = [
    ...calendar.sets.map<CalendarEntry>((release) => ({
      kind: 'set',
      key: `set:${release.set.code}`,
      date: release.released_at,
      release,
    })),
    ...calendar.secret_lair_drops.map<CalendarEntry>((release) => ({
      kind: 'drop',
      key: `drop:${release.slug}`,
      date: release.released_at,
      release,
    })),
  ]
  entries.sort(
    (a, b) =>
      a.date.localeCompare(b.date) ||
      kindRank(a) - kindRank(b) ||
      entryName(a).localeCompare(entryName(b)),
  )

  const byMonth = new Map<string, CalendarEntry[]>()
  for (const key of monthKeysBetween(calendar.from, calendar.to)) byMonth.set(key, [])
  for (const entry of entries) {
    const key = monthKey(entry.date)
    // An entry outside the window's months (the API never sends one) still gets a home
    // rather than being dropped on the floor.
    const bucket = byMonth.get(key) ?? []
    bucket.push(entry)
    byMonth.set(key, bucket)
  }

  const currentKey = monthKey(isoDate(today))
  return [...byMonth.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([key, monthEntries]) => ({
      key,
      label: monthLabel(key),
      current: key === currentKey,
      entries: monthEntries,
    }))
}

function kindRank(entry: CalendarEntry): number {
  return entry.kind === 'set' ? 0 : 1
}

/** The display name of an entry — the set's, or the drop's title. */
export function entryName(entry: CalendarEntry): string {
  return entry.kind === 'set' ? entry.release.set.name : entry.release.title
}

/** Whether an entry's date is after today (a date-only compare, so release day itself reads
 *  as released). */
export function isUpcoming(entry: CalendarEntry, today: Date): boolean {
  return entry.date > isoDate(today)
}

/** Where an entry's own page is: a set's catalog page, or the Secret Lair set page filtered
 *  to the drop (the by-drop view's own `?drop=` title filter). */
export function entryPath(game: string, entry: CalendarEntry): string {
  const g = encodeURIComponent(game)
  if (entry.kind === 'set') return `/cards/${g}/sets/${encodeURIComponent(entry.release.set.code)}`
  const search = new URLSearchParams({ drop: entry.release.title })
  return `/cards/${g}/sets/${encodeURIComponent(entry.release.set_code)}?${search}`
}

/** What an entry is, as a chip: the classification the heads-ups use, worded the same way
 *  the alert settings word their opt-ins. */
export function entryKindLabel(entry: CalendarEntry): string {
  if (entry.kind === 'drop') return 'Secret Lair drop'
  return entry.release.secret_lair ? 'Secret Lair set' : 'Set'
}

/** Human labels for the set types the calendar admits (Scryfall's `set_type` vocabulary).
 *  An unlisted slug is humanised generically rather than shown raw. */
const SET_TYPE_LABEL: Readonly<Record<string, string>> = {
  core: 'Core set',
  expansion: 'Expansion',
  commander: 'Commander',
  draft_innovation: 'Draft innovation',
  masters: 'Masters',
  funny: 'Un-set',
  box: 'Box set',
}

export function setTypeLabel(slug: string | null | undefined): string | null {
  if (!slug) return null
  const known = SET_TYPE_LABEL[slug]
  if (known) return known
  const words = slug.replace(/_/g, ' ').trim()
  return words ? words.charAt(0).toUpperCase() + words.slice(1) : null
}
