import { describe, expect, it } from 'vitest'
import type { LifeEvent, LifeSeat } from '@/lib/api'
import {
  describeChange,
  durationLabel,
  elapsedLabel,
  lifeDuration,
  lifeExtent,
  lifeLines,
  seatColor,
  sessionDuration,
  winRateLabel,
} from '@/lib/lifeSeries'

const START = '2026-07-30T12:00:00.000Z'

function seat(over: Partial<LifeSeat> & { id: number; position: number }): LifeSeat {
  return {
    name: `Player ${over.position + 1}`,
    deck_id: null,
    deck_name: null,
    starting_life: 40,
    life: 40,
    rotation: 0,
    result: 'none',
    ...over,
  } as LifeSeat
}

function event(over: Partial<LifeEvent> & { id: number; player_id: number }): LifeEvent {
  return {
    delta: -1,
    life_after: 39,
    kind: 'adjust',
    created_at: START,
    ...over,
  } as LifeEvent
}

describe('lifeLines', () => {
  it('starts every seat at its own starting life, so an untouched seat still draws', () => {
    const lines = lifeLines([seat({ id: 1, position: 0, starting_life: 40 })], [], START)
    expect(lines).toHaveLength(1)
    // One origin point: a flat line at 40, not an empty series that vanishes from the chart.
    expect(lines[0]?.points).toEqual([{ at: 0, life: 40, eventId: null }])
  })

  it('turns the interleaved event log into one series per seat', () => {
    const seats = [seat({ id: 1, position: 0 }), seat({ id: 2, position: 1 })]
    const events = [
      event({ id: 10, player_id: 1, delta: -3, life_after: 37 }),
      event({ id: 11, player_id: 2, delta: -5, life_after: 35 }),
      event({ id: 12, player_id: 1, delta: -2, life_after: 35 }),
    ]
    const lines = lifeLines(seats, events, START)
    expect(lines[0]?.points.map((p) => p.life)).toEqual([40, 37, 35])
    expect(lines[1]?.points.map((p) => p.life)).toEqual([40, 35])
    // Each point remembers its event, so the chart can point back at the ledger row.
    expect(lines[0]?.points.map((p) => p.eventId)).toEqual([null, 10, 12])
  })

  it('measures time from the start of the game, never negative', () => {
    const lines = lifeLines(
      [seat({ id: 1, position: 0 })],
      [
        event({ id: 1, player_id: 1, created_at: '2026-07-30T12:05:00.000Z' }),
        // A row stamped before the session start (clock skew) clamps to the origin rather
        // than drawing off the left of the chart.
        event({ id: 2, player_id: 1, created_at: '2026-07-30T11:59:00.000Z' }),
      ],
      START,
    )
    expect(lines[0]?.points.map((p) => p.at)).toEqual([0, 300_000, 0])
  })

  it('drops an event whose seat has been removed instead of inventing a line for it', () => {
    const lines = lifeLines(
      [seat({ id: 1, position: 0 })],
      [event({ id: 1, player_id: 99 })],
      START,
    )
    expect(lines).toHaveLength(1)
    expect(lines[0]?.points).toHaveLength(1)
  })

  it('survives an unparseable start timestamp', () => {
    const lines = lifeLines(
      [seat({ id: 1, position: 0 })],
      [event({ id: 1, player_id: 1 })],
      'nope',
    )
    expect(lines[0]?.points.every((p) => Number.isFinite(p.at))).toBe(true)
  })
})

describe('extents', () => {
  it('gives a flat game some height rather than drawing on the axis', () => {
    const lines = lifeLines([seat({ id: 1, position: 0, starting_life: 20 })], [], START)
    expect(lifeExtent(lines)).toEqual({ min: 19, max: 21 })
  })

  it('spans every seat', () => {
    const lines = lifeLines(
      [seat({ id: 1, position: 0 }), seat({ id: 2, position: 1 })],
      [
        event({ id: 1, player_id: 1, life_after: 12 }),
        event({ id: 2, player_id: 2, life_after: 55 }),
      ],
      START,
    )
    expect(lifeExtent(lines)).toEqual({ min: 12, max: 55 })
  })

  it('has a usable extent with no lines at all', () => {
    expect(lifeExtent([])).toEqual({ min: 0, max: 1 })
    expect(lifeDuration([])).toBe(0)
  })

  it('takes the duration from the latest change across all seats', () => {
    const lines = lifeLines(
      [seat({ id: 1, position: 0 }), seat({ id: 2, position: 1 })],
      [
        event({ id: 1, player_id: 1, created_at: '2026-07-30T12:02:00.000Z' }),
        event({ id: 2, player_id: 2, created_at: '2026-07-30T12:09:00.000Z' }),
      ],
      START,
    )
    expect(lifeDuration(lines)).toBe(540_000)
  })
})

describe('seatColor', () => {
  it('is stable per position and distinct across a full table', () => {
    const six = [0, 1, 2, 3, 4, 5].map(seatColor)
    expect(new Set(six).size).toBe(6)
    // Keyed on the seat's position, not a list index, so removing a seat doesn't recolour
    // the others.
    expect(seatColor(2)).toBe(six[2])
  })

  it('wraps rather than returning nothing past the sixth seat', () => {
    expect(seatColor(6)).toBe(seatColor(0))
    expect(seatColor(-1)).toBe(seatColor(5))
  })
})

describe('describeChange', () => {
  it('says what happened, and calls a correction a correction', () => {
    expect(describeChange(event({ id: 1, player_id: 1, delta: -3, life_after: 37 }))).toBe('lost 3')
    expect(describeChange(event({ id: 1, player_id: 1, delta: 2, life_after: 42 }))).toBe(
      'gained 2',
    )
    // An absolute correction is not a gain or a loss, even though it moved the total.
    expect(
      describeChange(event({ id: 1, player_id: 1, kind: 'set', delta: -9, life_after: 31 })),
    ).toBe('set to 31')
    // A tap at the life floor moves nothing; don't claim it did.
    expect(describeChange(event({ id: 1, player_id: 1, delta: 0, life_after: -9999 }))).toBe(
      'no change',
    )
  })
})

describe('time labels', () => {
  it('reads elapsed time relative to the game', () => {
    expect(elapsedLabel(0)).toBe('start')
    expect(elapsedLabel(59_000)).toBe('start')
    expect(elapsedLabel(4 * 60_000)).toBe('4m')
    expect(elapsedLabel(72 * 60_000)).toBe('1h 12m')
  })

  it('formats a finished game duration with padded minutes', () => {
    expect(durationLabel(38 * 60_000)).toBe('38m')
    expect(durationLabel(64 * 60_000)).toBe('1h 04m')
  })

  it('has no duration for a game still in progress, or a nonsense pair', () => {
    expect(sessionDuration(START, null)).toBeNull()
    expect(sessionDuration(START, '2026-07-30T11:00:00.000Z')).toBeNull()
    expect(sessionDuration('nope', '2026-07-30T13:00:00.000Z')).toBeNull()
    expect(sessionDuration(START, '2026-07-30T12:45:00.000Z')).toBe(45 * 60_000)
  })
})

describe('winRateLabel', () => {
  it('rounds to a whole percent, and says nothing when there is nothing to rate', () => {
    expect(winRateLabel(null)).toBeNull()
    expect(winRateLabel(0)).toBe('0%')
    expect(winRateLabel(1 / 3)).toBe('33%')
    expect(winRateLabel(1)).toBe('100%')
  })
})
