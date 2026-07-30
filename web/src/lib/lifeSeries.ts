import type { LifeEvent, LifeSeat } from '@/lib/api'

/**
 * Turning a game's flat event log into the shapes the history surfaces draw — and giving each
 * seat a stable colour.
 *
 * Two things are worth doing once, here, rather than in each component:
 *
 * - **Per-seat life over time.** The API returns one interleaved list of changes for the whole
 *   game (which is what the ledger wants), but a chart wants one line per seat, and each line has
 *   to start at that seat's *starting* life rather than at its first change. Deriving that in a
 *   pure function means the sparkline, the full chart and the scoreboard all plot the same
 *   numbers.
 * - **Seat colour by position.** Colour is keyed to the seat's position, not its index in a
 *   filtered list, so a seat keeps its colour when another is removed. Identity is never colour
 *   alone: every surface that colours a seat also names it.
 */

/** One point on a seat's life line. `at` is ms since the game started. */
export interface LifePoint {
  at: number
  life: number
  /** The event that produced this point, or null for the starting-life origin. */
  eventId: number | null
}

/** One seat's life over the course of the game. */
export interface LifeLine {
  playerId: number
  position: number
  name: string
  points: LifePoint[]
}

/**
 * Build one line per seat.
 *
 * Every line begins with an origin point at `t=0` holding the seat's starting life, so a seat
 * that hasn't been touched still draws (a flat line at its starting total) instead of vanishing
 * from the chart — "nothing happened to me yet" is information.
 */
export function lifeLines(seats: LifeSeat[], events: LifeEvent[], startedAt: string): LifeLine[] {
  const origin = Date.parse(startedAt)
  const base = Number.isNaN(origin) ? 0 : origin
  const byPlayer = new Map<number, LifePoint[]>()
  for (const seat of seats) {
    byPlayer.set(seat.id, [{ at: 0, life: seat.starting_life, eventId: null }])
  }
  for (const event of events) {
    const points = byPlayer.get(event.player_id)
    // An event whose seat has been removed has no line to join — skip it rather than
    // inventing one.
    if (!points) continue
    const at = Date.parse(event.created_at)
    points.push({
      at: Math.max(0, (Number.isNaN(at) ? base : at) - base),
      life: event.life_after,
      eventId: event.id,
    })
  }
  return seats.map((seat) => ({
    playerId: seat.id,
    position: seat.position,
    name: seat.name,
    points: byPlayer.get(seat.id) ?? [],
  }))
}

/** The life range across every line, padded so a flat line isn't drawn on the axis itself. */
export function lifeExtent(lines: LifeLine[]): { min: number; max: number } {
  let min = Infinity
  let max = -Infinity
  for (const line of lines) {
    for (const point of line.points) {
      if (point.life < min) min = point.life
      if (point.life > max) max = point.life
    }
  }
  if (min === Infinity) return { min: 0, max: 1 }
  // A game where nobody's total has changed yet would otherwise have zero height.
  if (min === max) return { min: min - 1, max: max + 1 }
  return { min, max }
}

/** The longest elapsed time across every line — the chart's x extent. */
export function lifeDuration(lines: LifeLine[]): number {
  let max = 0
  for (const line of lines) {
    // Every point, not just the last: `lifeLines` clamps a timestamp that would run backwards, so
    // a line's final point is not guaranteed to be its latest one.
    for (const point of line.points) if (point.at > max) max = point.at
  }
  return max
}

/**
 * Seat colours, by position, over the theme's existing `--chart-*` tokens.
 *
 * The order is chosen so adjacent seats never share a hue family in either theme (the light
 * theme's `chart-4` and `chart-5` are both warm yellows, so they're kept apart). A sixth seat
 * takes the foreground colour rather than inventing a new token — at six players the legend and
 * names are doing the identifying anyway.
 */
const SEAT_COLOR_VARS = [
  'var(--chart-1)',
  'var(--chart-2)',
  'var(--chart-4)',
  'var(--chart-3)',
  'var(--chart-5)',
  'var(--foreground)',
] as const

/** The stroke/fill colour for a seat, by position. Wraps past the sixth seat. */
export function seatColor(position: number): string {
  const index =
    ((position % SEAT_COLOR_VARS.length) + SEAT_COLOR_VARS.length) % SEAT_COLOR_VARS.length
  return SEAT_COLOR_VARS[index] as string
}

/**
 * A short, plain description of one life change — what the ledger row reads as.
 * An absolute correction is described as "set to", not as a gain or loss, since that's what
 * happened.
 */
export function describeChange(event: LifeEvent): string {
  if (event.kind === 'set') return `set to ${event.life_after}`
  if (event.delta > 0) return `gained ${event.delta}`
  if (event.delta < 0) return `lost ${Math.abs(event.delta)}`
  // A tap at the life floor moves nothing; say so rather than "gained 0".
  return 'no change'
}

/**
 * A compact relative time ("just now", "4m", "1h 12m") for a history row.
 *
 * Deliberately relative to the *game*, not to the wall clock: during play "12m in" is what you
 * want to know, and after the game a timestamp on every one of a hundred rows is noise.
 */
export function elapsedLabel(ms: number): string {
  if (ms < 60_000) return 'start'
  const minutes = Math.floor(ms / 60_000)
  if (minutes < 60) return `${minutes}m`
  const hours = Math.floor(minutes / 60)
  return `${hours}h ${minutes % 60}m`
}

/** How long a game ran, from its start and (optional) finish. Null while still in progress. */
export function sessionDuration(startedAt: string, finishedAt: string | null): number | null {
  if (!finishedAt) return null
  const start = Date.parse(startedAt)
  const end = Date.parse(finishedAt)
  if (Number.isNaN(start) || Number.isNaN(end) || end < start) return null
  return end - start
}

/** A duration as "38m" / "1h 04m", for a finished game's summary. */
export function durationLabel(ms: number): string {
  const minutes = Math.round(ms / 60_000)
  if (minutes < 60) return `${minutes}m`
  const hours = Math.floor(minutes / 60)
  return `${hours}h ${String(minutes % 60).padStart(2, '0')}m`
}

/** Format a win rate as a whole percentage, or `null` when there's nothing to rate. */
export function winRateLabel(rate: number | null): string | null {
  return rate === null ? null : `${Math.round(rate * 100)}%`
}
