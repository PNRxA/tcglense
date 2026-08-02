import { describe, expect, it } from 'vitest'
import {
  COMMANDER_DAMAGE,
  COUNTER_META,
  countersFor,
  defaultCountersFor,
  indexCounters,
  isCounterKind,
  isLethalValue,
  lethalReason,
  LIFE_COUNTERS,
  visibleCounters,
  worstCommanderDamage,
  type LifeCounterKind,
} from '@/lib/lifeCounters'
import type { LifeCounter, LifeSeat } from '@/lib/api'

// The counter vocabulary is mirrored on the server, which validates against its own copy — so
// the mirror is pinned here rather than trusted, exactly as `lifeLayout.spec` pins the layout
// slugs. The rest of these cases are the readings that are easy to get subtly wrong: commander
// damage is per source (7 from three opponents is not lethal), a counter that has been switched
// off still shows what it holds, and nothing here ever *finishes* a game.

const counterRow = (
  player_id: number,
  counter: string,
  value: number,
  source_player_id: number | null = null,
): LifeCounter => ({ player_id, counter, source_player_id, value })

const seat = (id: number, life = 40): LifeSeat => ({
  id,
  position: id - 1,
  name: `Player ${id}`,
  deck_id: null,
  deck_name: null,
  commander_card_id: null,
  commander_name: null,
  starting_life: 40,
  life,
  rotation: 0,
  result: 'none',
})

describe('the counter vocabulary mirrors the server', () => {
  it('offers exactly the counters the server will accept, in its display order', () => {
    // Mirrors OPTIONAL_COUNTERS in api/src/handlers/tools/life/counters.rs, order included —
    // that list is both what the API validates a session's `counters` against and the order it
    // normalises them into, so a slug added on one side only would either be rejected by the API
    // or render in a different place from where it is stored.
    expect([...LIFE_COUNTERS]).toEqual(['commander_damage', 'poison', 'energy', 'experience'])
  })

  it('agrees on which formats open with the damage matrix on', () => {
    // Mirrors COMMANDER_FORMATS + default_counters_for in the same file: the dialog's toggles
    // have to land on the same answer the server would have chosen on its own.
    for (const format of ['commander', 'edh', 'brawl', 'oathbreaker', 'duel commander']) {
      expect(defaultCountersFor(format)).toEqual([COMMANDER_DAMAGE])
    }
    // Case and padding are normalised the same way the server's `trim().to_lowercase()` does.
    expect(defaultCountersFor(' EDH ')).toEqual([COMMANDER_DAMAGE])
    // ...and nothing else does. `null` is the no-format case a plain duel starts with.
    expect(defaultCountersFor('standard')).toEqual([])
    expect(defaultCountersFor(null)).toEqual([])
  })

  it('knows every slug it renders and no others', () => {
    for (const kind of LIFE_COUNTERS) {
      expect(isCounterKind(kind)).toBe(true)
      expect(COUNTER_META[kind].label.length).toBeGreaterThan(0)
    }
    expect(isCounterKind('life')).toBe(false)
    expect(isCounterKind('stun')).toBe(false)
  })
})

describe('indexing a game’s counter state', () => {
  it('splits commander damage by source and keeps the rest by kind', () => {
    const indexed = indexCounters([
      counterRow(1, 'commander_damage', 7, 2),
      counterRow(1, 'commander_damage', 6, 3),
      counterRow(1, 'poison', 4),
      counterRow(2, 'energy', 9),
    ])
    const alice = countersFor(indexed, 1)
    expect(alice.commanderDamage.get(2)).toBe(7)
    expect(alice.commanderDamage.get(3)).toBe(6)
    expect(alice.values.poison).toBe(4)
    expect(countersFor(indexed, 2).values.energy).toBe(9)
    // A seat that has moved nothing reads as empty rather than undefined.
    expect(countersFor(indexed, 99).commanderDamage.size).toBe(0)
    expect(countersFor(indexed, 99).values.poison).toBeUndefined()
  })

  it('measures commander damage against the worst single source, never the sum', () => {
    // 7 from each of three opponents is 21 in total and lethal from none of them — reading the
    // sum here would call a living player dead.
    const indexed = indexCounters([
      counterRow(1, 'commander_damage', 7, 2),
      counterRow(1, 'commander_damage', 7, 3),
      counterRow(1, 'commander_damage', 7, 4),
    ])
    expect(worstCommanderDamage(countersFor(indexed, 1))).toBe(7)
    expect(lethalReason(seat(1), countersFor(indexed, 1))).toBeNull()
  })

  it('drops a slug this build doesn’t know, and damage with no source', () => {
    // Forward compatibility: a newer server counter must not crash the mat, and a sourceless
    // damage row can't be attributed to a commander — reading it as "from seat 0" would invent
    // a lethal opponent.
    const indexed = indexCounters([
      counterRow(1, 'stun', 3),
      counterRow(1, 'commander_damage', 21, null),
    ])
    expect(countersFor(indexed, 1).commanderDamage.size).toBe(0)
    expect(lethalReason(seat(1), countersFor(indexed, 1))).toBeNull()
  })
})

describe('what the mat shows', () => {
  it('shows what the game tracks plus anything that still holds a value', () => {
    // Turning a counter off is a display choice that deliberately doesn't delete its rows, so
    // hiding one that still holds a value would turn "I don't need this" into invisible state.
    expect(visibleCounters(['poison'], [])).toEqual(['poison'])
    expect(visibleCounters([], [counterRow(1, 'energy', 3)])).toEqual(['energy'])
    // A counter that folded back to zero is not state worth a row.
    expect(visibleCounters([], [counterRow(1, 'energy', 0)])).toEqual([])
    // Always in vocabulary order, whatever order the two inputs are in.
    expect(
      visibleCounters(['experience', 'commander_damage'], [counterRow(1, 'poison', 1)]),
    ).toEqual(['commander_damage', 'poison', 'experience'])
  })
})

describe('being out of the game', () => {
  it('reports the reason and reports it for the counters too, not only life', () => {
    const none = countersFor(indexCounters([]), 1)
    expect(lethalReason(seat(1, 12), none)).toBeNull()
    expect(lethalReason(seat(1, 0), none)).toBe('out of life')

    // 21 from one commander, on a seat still at full life — which is the whole reason #595
    // exists: before it, this game read as if nobody had died.
    const damaged = countersFor(indexCounters([counterRow(1, 'commander_damage', 21, 2)]), 1)
    expect(lethalReason(seat(1, 40), damaged)).toBe('21 commander damage')

    const poisoned = countersFor(indexCounters([counterRow(1, 'poison', 10)]), 1)
    expect(lethalReason(seat(1, 40), poisoned)).toBe('10 poison')
  })

  it('only calls a counter lethal when it can be', () => {
    expect(isLethalValue('commander_damage', 20)).toBe(false)
    expect(isLethalValue('commander_damage', 21)).toBe(true)
    expect(isLethalValue('poison', 10)).toBe(true)
    // Energy and experience accumulate without ever ending a game, however high they go.
    for (const kind of ['energy', 'experience'] as LifeCounterKind[]) {
      expect(isLethalValue(kind, 999)).toBe(false)
      expect(COUNTER_META[kind].lethalAt).toBeNull()
    }
  })
})
