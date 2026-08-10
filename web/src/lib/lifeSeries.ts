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
 *
 * **Only `life` events plot here.** Since #595 `life_after` holds whatever counter its row
 * names, so folding a commander-damage row into this series would draw a seat's life crashing
 * to 7 when it never moved — and, because the chart carries each seat's last value forward and
 * rescales its axis across every line, one such point corrupts the whole pod's chart.
 */
export function lifeLines(seats: LifeSeat[], events: LifeEvent[], startedAt: string): LifeLine[] {
  const origin = Date.parse(startedAt)
  const base = Number.isNaN(origin) ? 0 : origin
  const byPlayer = new Map<number, LifePoint[]>()
  for (const seat of seats) {
    byPlayer.set(seat.id, [{ at: 0, life: seat.starting_life, eventId: null }])
  }
  for (const event of events) {
    if (event.counter !== 'life') continue
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
 * One column of the history chart: every seat's total immediately after one recorded change.
 *
 * The chart plots these by their **index**, not their timestamp, so every change gets the same
 * width. A game is a sequence of things happening, and the ten quiet minutes while someone reads
 * a card are a break in play, not a hole in the chart — time spacing crushed a turn's worth of
 * trades into a few pixels and drew a long flat corridor beside it. The clock isn't thrown away:
 * it rides along on each step, and the axis ticks and the tooltip read it back out.
 */
export interface LifeStep {
  /** Position on the (even) x axis — 0 is the starting totals, 1 the first change, and so on. */
  step: number
  /** ms since the game started, for the axis/tooltip labels. */
  at: number
  /** The event this step records, or null for the starting-life origin at step 0. */
  eventId: number | null
  /**
   * Every seat's total at this step, keyed by seat id — carried forward for the seats this change
   * didn't touch, so each line has a value in every column rather than gapping between its own
   * events.
   */
  lives: Record<number, number>
}

/**
 * Fold the per-seat lines back into one evenly-spaced column per recorded change.
 *
 * Ordering is by **event id**, not by timestamp: `lifeLines` clamps a row stamped before the game
 * started, so `at` can run backwards while ids never do.
 *
 * A line's first point is that seat's starting total (which is what `lifeLines` guarantees), so
 * every seat has a value at step 0 and an untouched seat still draws a flat line across the whole
 * chart. A line with no points at all — a seat the caller handed in empty — takes no column and
 * no key, rather than punching a hole in every row.
 */
export function lifeSteps(lines: LifeLine[]): LifeStep[] {
  const lives: Record<number, number> = {}
  const changes: { playerId: number; point: LifePoint }[] = []
  for (const line of lines) {
    const [origin, ...rest] = line.points
    if (!origin) continue
    lives[line.playerId] = origin.life
    for (const point of rest) changes.push({ playerId: line.playerId, point })
  }
  if (Object.keys(lives).length === 0) return []
  changes.sort((a, b) => (a.point.eventId ?? 0) - (b.point.eventId ?? 0))
  const steps: LifeStep[] = [{ step: 0, at: 0, eventId: null, lives: { ...lives } }]
  changes.forEach(({ playerId, point }, index) => {
    lives[playerId] = point.life
    steps.push({ step: index + 1, at: point.at, eventId: point.eventId, lives: { ...lives } })
  })
  return steps
}

/**
 * Seat colours, by position, over the theme's existing `--chart-*` tokens.
 *
 * The design system's chart palette (ember, teal, violet, gold, blue — see
 * docs/design-system.md) is CVD-validated **in token order** in both themes, so seats take the
 * tokens in that order; re-shuffling it would re-create adjacent hue clashes the validator
 * already ruled out. A sixth seat takes the foreground colour rather than inventing a new
 * token — at six players the legend and names are doing the identifying anyway.
 */
const SEAT_COLOR_VARS = [
  'var(--chart-1)',
  'var(--chart-2)',
  'var(--chart-3)',
  'var(--chart-4)',
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
 * A short, plain description of one change — what the ledger row reads as.
 *
 * An absolute correction is described as "set to", not as a gain or loss, since that's what
 * happened. A change to a **counter** names it (and, for commander damage, whose commander
 * dealt it): "gained 7" and "took 7 commander damage from Bao" are the same row shape but very
 * different events, and a ledger you undo from has to tell two adjacent ones apart.
 *
 * `sourceName` resolves the damage source; a source that has left the table has no name left to
 * give, and the phrase falls back to naming the counter alone.
 */
export function describeChange(event: LifeEvent, sourceName?: string): string {
  const counter = event.counter === 'life' ? null : (COUNTER_LABELS[event.counter] ?? event.counter)
  if (event.kind === 'set') {
    return counter ? `set ${counter} to ${event.life_after}` : `set to ${event.life_after}`
  }
  if (event.delta === 0) {
    // A tap at a floor or ceiling moves nothing; say so rather than "gained 0".
    return counter ? `no change to ${counter}` : 'no change'
  }
  if (!counter) return event.delta > 0 ? `gained ${event.delta}` : `lost ${Math.abs(event.delta)}`
  const from = sourceName ? ` from ${sourceName}` : ''
  const verb = event.delta > 0 ? 'took' : 'shed'
  return `${verb} ${Math.abs(event.delta)} ${counter}${from}`
}

/** How each counter reads inside a ledger phrase ("took 7 commander damage from Bao"). */
const COUNTER_LABELS: Record<string, string> = {
  commander_damage: 'commander damage',
  poison: 'poison',
  energy: 'energy',
  experience: 'experience',
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

/**
 * A game clock ("0:42", "12:07", "1:04:19") for a point in the history.
 *
 * The history chart's x axis counts changes rather than minutes, so the clock is what puts the
 * time back — to the second, because two changes seconds apart would both read as "start" at
 * `elapsedLabel`'s minute resolution, which is exactly the pair the chart now draws side by side.
 */
export function clockLabel(ms: number): string {
  const total = Math.max(0, Math.round(ms / 1000))
  const hours = Math.floor(total / 3600)
  const minutes = Math.floor(total / 60) % 60
  const seconds = total % 60
  const mm = hours > 0 ? String(minutes).padStart(2, '0') : String(minutes)
  return `${hours > 0 ? `${hours}:` : ''}${mm}:${String(seconds).padStart(2, '0')}`
}

/** A history step's heading: which change it is, and when it happened. */
export function stepLabel(step: LifeStep): string {
  if (step.eventId === null) return 'Start'
  return `Change ${step.step} · ${clockLabel(step.at)}`
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
