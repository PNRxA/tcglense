import { request } from './client'
import type { ReleaseCalendar } from './generated'

// ---------- Release calendar (public) ----------
//
// `GET /api/games/{game}/releases?from&to` (issue #679): the sets releasing inside a date
// window — each with the preconstructed decks and sealed products it ships — and the Secret
// Lair drops, both date-ascending. It is the page behind the release heads-ups, and lists
// exactly what they would notify about: the API builds both from one definition of "a
// release", so this read can never disagree with a subscriber's notification.
//
// The same for every visitor (nothing per-user rides it), so it is CDN + ETag cached like the
// rest of the catalog. The nested rows are the shapes their own listings publish (`CardSet`,
// `PreconDeck`, `Product`), so the tiles already built for those render an entry unchanged.

export type { ReleaseCalendar, SecretLairDropRelease, SetRelease } from './generated'

/** The inclusive `YYYY-MM-DD` bounds of a calendar read. Both are sent explicitly — the API
 * defaults to "today + 90 days" when they're absent, but a URL that depends on the clock is a
 * URL the CDN can't share across visitors. */
export interface ReleaseWindow {
  from: string
  to: string
}

/** The calendar path for a game and window, `from` before `to`, so one window is one URL. */
export function releaseCalendarPath(game: string, window: ReleaseWindow): string {
  const search = new URLSearchParams({ from: window.from, to: window.to })
  return `/api/games/${encodeURIComponent(game)}/releases?${search}`
}

export function getReleaseCalendar(
  game: string,
  window: ReleaseWindow,
  signal?: AbortSignal,
): Promise<ReleaseCalendar> {
  return request<ReleaseCalendar>(releaseCalendarPath(game, window), { signal })
}
