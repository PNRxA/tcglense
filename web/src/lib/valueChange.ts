import type { ValueChange } from '@/lib/api'

// The wording seam for a signed money movement — the collection landing's daily-change lines
// (under each "Total value" stat) and the movers panel's per-row deltas both format a signed
// USD string and a percentage the same way, so the sign glyphs, the neutral zero and the
// "which token colours this" decision live here once rather than in two templates.

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

/** A stat's daily movement, ready to render: the signed money text, the optional percentage
 * chip, the direction for colouring, and the reference date the figure is measured to. */
export interface StatChange {
  text: string
  pctText: string | null
  direction: ChangeDirection
  /** The `YYYY-MM-DD` snapshot date the movement is measured to, when known. */
  asOf: string | null
}

/**
 * Shape one holding kind's (or the whole basket's) `ValueChange` into a renderable line, or
 * `null` when there is no movement to show — nothing priced, or no baseline capture yet (a
 * single captured day), in which case the stat shows its value alone.
 */
export function describeValueChange(
  change: ValueChange | null | undefined,
  formatUsd: (raw: string | null | undefined) => string | null,
): StatChange | null {
  if (!change?.change_usd) return null
  const text = formatSignedMoney(change.change_usd, formatUsd)
  if (text == null) return null
  return {
    text,
    pctText: formatSignedPct(change.change_pct),
    direction: changeDirection(change.change_usd),
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
