import type { MoverWindow, ValueChange } from '@/lib/api'

// The wording seam for a signed money movement — the collection landing's value-change lines
// (under the combined and cards "Total value" stats and the sealed section's "Products value"
// stat) and the movers panel's per-row deltas both format a signed USD string and a
// percentage the same way, so the sign glyphs, the neutral zero and the "which token colours
// this" decision live here once rather than in two templates. The window vocabulary the two
// surfaces share — the seven `?window=` tokens, their short labels and the sentence each
// reads as — lives here too, so a picker on one surface and a line on the other can't drift.

/** The window the landing's value-change lines open on: a week. Daily moves are often tiny or
 * empty on a young collection, while longer windows hide the news (the movers panel reasons
 * the same way). */
export const DEFAULT_CHANGE_WINDOW: MoverWindow = 'week'

/** The seven movement windows in picker order, with the short label each shows. */
export const CHANGE_WINDOW_OPTIONS: readonly { value: MoverWindow; label: string }[] = [
  { value: 'day', label: '1D' },
  { value: 'week', label: '7D' },
  { value: 'month', label: '30D' },
  { value: 'year', label: '1Y' },
  { value: 'two_year', label: '2Y' },
  { value: 'three_year', label: '3Y' },
  { value: 'all_time', label: 'All' },
]

/** Whether a stored/URL value is one of the seven window tokens. */
export function isChangeWindow(value: unknown): value is MoverWindow {
  return CHANGE_WINDOW_OPTIONS.some((opt) => opt.value === value)
}

/** The short label (`7D`) for a window token. */
export function changeWindowLabel(window: MoverWindow): string {
  return CHANGE_WINDOW_OPTIONS.find((opt) => opt.value === window)?.label ?? window
}

/** The clause a movement over the window reads as in a sentence ("over the last 7 days"). */
export function changeWindowSentence(window: MoverWindow): string {
  switch (window) {
    case 'day':
      return "since the previous day's captured prices"
    case 'week':
      return 'over the last 7 days'
    case 'month':
      return 'over the last 30 days'
    case 'year':
      return 'over the last year'
    case 'two_year':
      return 'over the last 2 years'
    case 'three_year':
      return 'over the last 3 years'
    case 'all_time':
      return 'since the earliest captured prices'
  }
}

/** Which way a movement reads: `up` (gain), `down` (loss) or `flat` (no movement). Drives the
 * success / destructive / muted token choice in every surface that shows one. */
export type ChangeDirection = 'up' | 'down' | 'flat'

/** The sign of a signed decimal (or numeric) change. A non-finite value reads as flat. */
export function changeDirection(change: string | number): ChangeDirection {
  const n = typeof change === 'number' ? change : Number(change)
  if (!Number.isFinite(n) || n === 0) return 'flat'
  return n > 0 ? 'up' : 'down'
}

/**
 * A signed money string from the API's signed decimal (`"-3.50"`), rendered through the
 * caller's money formatter (which converts to the display currency and would otherwise print
 * `$-3.50`): the magnitude is formatted and the sign re-applied as a `+` or a real minus
 * (U+2212, whose glyph width matches the plus). A zero carries no sign. `null` when the
 * formatter has nothing to show for the magnitude.
 */
export function formatSignedMoney(
  change: string | number,
  formatUsd: (raw: string | null | undefined) => string | null,
): string | null {
  const n = typeof change === 'number' ? change : Number(change)
  if (!Number.isFinite(n)) return typeof change === 'string' ? change : null
  const magnitude = formatUsd(Math.abs(n).toFixed(2))
  if (magnitude == null) return null
  if (n === 0) return magnitude
  return `${n > 0 ? '+' : '−'}${magnitude}`
}

/** A signed one-decimal percentage (`+2.8%`, `−0.5%`, `0.0%`), or `null` when the API had no
 * percentage to give (a zero baseline). */
export function formatSignedPct(pct: number | null | undefined): string | null {
  if (pct == null || !Number.isFinite(pct)) return null
  const magnitude = Math.abs(pct).toFixed(1)
  if (Number(magnitude) === 0) return `${magnitude}%`
  return `${pct > 0 ? '+' : '−'}${magnitude}%`
}

/** A stat's movement over a window, ready to render: the signed money text, the optional
 * percentage chip, the direction for colouring, the window it covers, and the reference date
 * the figure is measured to. */
export interface StatChange {
  text: string
  pctText: string | null
  direction: ChangeDirection
  /** The window the movement covers (its short label is the line's tag). */
  window: MoverWindow
  /** The `YYYY-MM-DD` snapshot date the movement is measured to, when known. */
  asOf: string | null
}

/**
 * Shape one holding kind's (or the whole basket's) `ValueChange` into a renderable line for
 * `window`, or `null` when there is no movement to show — nothing priced, or no capture
 * reaching back to the window's baseline yet, in which case the stat shows its value alone.
 */
export function describeValueChange(
  change: ValueChange | null | undefined,
  formatUsd: (raw: string | null | undefined) => string | null,
  window: MoverWindow,
): StatChange | null {
  if (!change?.change_usd) return null
  const text = formatSignedMoney(change.change_usd, formatUsd)
  if (text == null) return null
  return {
    text,
    pctText: formatSignedPct(change.change_pct),
    direction: changeDirection(change.change_usd),
    window,
    asOf: change.as_of,
  }
}

/** A short `Sep 22` rendering of a `YYYY-MM-DD` snapshot date for the movement's tooltip, or
 * the raw string when it doesn't parse. */
export function formatAsOfDate(asOf: string): string {
  const date = new Date(`${asOf}T00:00:00`)
  if (Number.isNaN(date.getTime())) return asOf
  return new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric' }).format(date)
}
