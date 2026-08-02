import type { LifeCounter, LifeSeat } from '@/lib/api'

/**
 * The counters a seat carries besides life, and how to read a game's counter state.
 *
 * Pure, like `lib/lifeLayout` beside it, and for the same reason: which counters a game shows,
 * what each one means, and when one of them is lethal are all decisions that are easy to get
 * subtly wrong and invisible in a screenshot.
 *
 * **The vocabulary is mirrored on the server** (`counters.rs` in
 * `api/src/handlers/tools/life/`), which validates against its own copy — `countersMatchServer`
 * in the spec pins the two together, so adding a slug on one side fails a test rather than a
 * request.
 *
 * **Nothing here derives a value.** The server folds the history and sends the current value of
 * every counter with the game (`LifeSessionDetail.counters`), and every tap answers with the
 * counter it moved, so the client indexes what it is given rather than re-implementing the
 * replay. A second fold here is exactly how the mat and the history would come to disagree.
 */

/** A counter beyond life. `life` itself is the seat's own total, not one of these. */
export const LIFE_COUNTERS = ['commander_damage', 'poison', 'energy', 'experience'] as const
export type LifeCounterKind = (typeof LIFE_COUNTERS)[number]

/** The only counter keyed by *who* dealt it — 21 damage is counted per commander. */
export const COMMANDER_DAMAGE: LifeCounterKind = 'commander_damage'

/** How a counter reads on screen. */
export interface CounterMeta {
  kind: LifeCounterKind
  label: string
  /** One-word form for a chip on a seat tile, where the full label won't fit. */
  short: string
  /** What it does, for the picker that turns it on. */
  hint: string
  /**
   * The value at which this counter has *by itself* ended the game for its seat, or null when
   * it never does. Energy and experience accumulate without ever being lethal.
   */
  lethalAt: number | null
}

export const COUNTER_META: Record<LifeCounterKind, CounterMeta> = {
  commander_damage: {
    kind: 'commander_damage',
    label: 'Commander damage',
    short: 'CMD',
    hint: 'Damage from each opponent’s commander, counted separately — 21 from one is lethal.',
    lethalAt: 21,
  },
  poison: {
    kind: 'poison',
    label: 'Poison',
    short: 'Poison',
    hint: 'Poison counters. Ten is lethal.',
    lethalAt: 10,
  },
  energy: {
    kind: 'energy',
    label: 'Energy',
    short: 'Energy',
    hint: 'Energy counters — a resource you spend, never lethal.',
    lethalAt: null,
  },
  experience: {
    kind: 'experience',
    label: 'Experience',
    short: 'XP',
    hint: 'Experience counters — they only ever go up.',
    lethalAt: null,
  },
}

/** The formats whose games open with the commander-damage matrix on. */
const COMMANDER_FORMATS = ['commander', 'edh', 'brawl', 'oathbreaker', 'duel commander']

/**
 * What a new game of `format` tracks by default — mirroring `default_counters_for` server-side,
 * so the dialog's checkboxes agree with what the server would have chosen anyway.
 */
export function defaultCountersFor(format: string | null | undefined): LifeCounterKind[] {
  if (!format) return []
  return COMMANDER_FORMATS.includes(format.trim().toLowerCase()) ? [COMMANDER_DAMAGE] : []
}

/** Narrow a slug off the wire to one this build knows how to render. */
export function isCounterKind(slug: string): slug is LifeCounterKind {
  return (LIFE_COUNTERS as readonly string[]).includes(slug)
}

/**
 * The counters to show for a game: the ones it tracks, **plus** any a seat has actually
 * recorded.
 *
 * The union matters. Turning a counter off is a display choice that deliberately doesn't delete
 * what was recorded (the server keeps the rows), so hiding a row that still holds a value would
 * turn "I don't need this" into invisible state — and for commander damage that's the state
 * that decides who won.
 */
export function visibleCounters(
  tracked: readonly string[],
  counters: readonly LifeCounter[],
): LifeCounterKind[] {
  const recorded = new Set(counters.filter((row) => row.value !== 0).map((row) => row.counter))
  return LIFE_COUNTERS.filter((kind) => tracked.includes(kind) || recorded.has(kind))
}

/** One seat's counter state, indexed for the tile and the dialog. */
export interface SeatCounters {
  /** The sourceless counters, by kind. Absent = never moved = 0. */
  values: Partial<Record<LifeCounterKind, number>>
  /** Commander damage by source seat id. */
  commanderDamage: Map<number, number>
}

const emptySeatCounters = (): SeatCounters => ({ values: {}, commanderDamage: new Map() })

/** Index a game's counter rows by seat, so a tile is one map lookup rather than a scan. */
export function indexCounters(counters: readonly LifeCounter[]): Map<number, SeatCounters> {
  const byPlayer = new Map<number, SeatCounters>()
  for (const row of counters) {
    if (!isCounterKind(row.counter)) continue
    let seat = byPlayer.get(row.player_id)
    if (!seat) {
      seat = emptySeatCounters()
      byPlayer.set(row.player_id, seat)
    }
    if (row.counter === COMMANDER_DAMAGE) {
      // A row with no source can't be attributed to a commander, so it isn't damage anyone
      // took from one — the server refuses to write one, and reading it as "from seat 0" would
      // invent a lethal source.
      if (row.source_player_id !== null) seat.commanderDamage.set(row.source_player_id, row.value)
    } else {
      seat.values[row.counter] = row.value
    }
  }
  return byPlayer
}

/** One seat's counters, or an empty set for a seat that has moved none. */
export function countersFor(indexed: Map<number, SeatCounters>, playerId: number): SeatCounters {
  return indexed.get(playerId) ?? emptySeatCounters()
}

/** The most damage a single commander has dealt this seat — the number 21 is measured against. */
export function worstCommanderDamage(seat: SeatCounters): number {
  let worst = 0
  for (const value of seat.commanderDamage.values()) worst = Math.max(worst, value)
  return worst
}

/**
 * Why this seat is out of the game, if it is — as a phrase to show, or null.
 *
 * A **suggestion**, never an action: reaching 21 commander damage ends a game at the table, but
 * a session that finishes itself would be recording a result nobody confirmed, and a recorded
 * result is immutable and counts towards the per-deck record. So this marks the seat and leaves
 * the finishing to a person.
 */
export function lethalReason(seat: LifeSeat, counters: SeatCounters): string | null {
  if (seat.life <= 0) return 'out of life'
  const worst = worstCommanderDamage(counters)
  const cmdLethal = COUNTER_META.commander_damage.lethalAt ?? Infinity
  if (worst >= cmdLethal) return `${worst} commander damage`
  const poison = counters.values.poison ?? 0
  const poisonLethal = COUNTER_META.poison.lethalAt ?? Infinity
  if (poison >= poisonLethal) return `${poison} poison`
  return null
}

/** Whether a counter's value is at or past the point that ends the game on its own. */
export function isLethalValue(kind: LifeCounterKind, value: number): boolean {
  const at = COUNTER_META[kind].lethalAt
  return at !== null && value >= at
}
