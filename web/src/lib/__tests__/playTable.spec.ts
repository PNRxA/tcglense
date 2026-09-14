import { describe, expect, it } from 'vitest'
import {
  PLAY_DRAG_THRESHOLD,
  battlefieldLayout,
  cardAriaLabel,
  clampFraction,
  counterChips,
  exceedsDragThreshold,
  fanLayout,
  logLine,
  menuActionsFor,
  parseCount,
  pointToFraction,
  zoneLabel,
} from '@/lib/playTable'
import type { PlayCardView, PlayLogEntry, PlaySeatSnapshot, PlayZone } from '@/lib/api/play'

// The table's decisions, without a table.
//
// The two that matter most are the menu matrix (what a card may even offer, which is six zones
// × mine/theirs × two formats and unreadable as markup) and the coordinate maths every drop
// goes through — both are the kind of thing that breaks silently in a component test because
// "the menu rendered" and "the menu rendered the right things" look the same from outside.

function card(overrides: Partial<PlayCardView> = {}): PlayCardView {
  return {
    id: 1,
    def: {
      card_id: 'abc',
      game: 'mtg',
      name: 'Sol Ring',
      faces: [
        {
          name: 'Sol Ring',
          mana_cost: '{1}',
          type_line: 'Artifact',
          oracle_text: null,
          power: null,
          toughness: null,
          loyalty: null,
        },
      ],
      back_image: false,
      has_image: true,
      colors: [],
      cmc: 1,
      is_commander: false,
      is_token: false,
    },
    owner: 1,
    controller: 1,
    zone: 'battlefield',
    tapped: false,
    face_down: false,
    face_index: 0,
    revealed: false,
    counters: {},
    x: 0.5,
    y: 0.5,
    attached_to: null,
    power_toughness: null,
    ...overrides,
  }
}

function ids(zone: PlayZone, mine = true, format = 'commander', overrides = {}) {
  return menuActionsFor(card(overrides), zone, mine, format).map((action) => action.id)
}

describe('pointToFraction', () => {
  const rect = { left: 100, top: 50, width: 400, height: 200 }

  it('reads a pointer as a fraction of the board it is over', () => {
    expect(pointToFraction(rect, 300, 150)).toEqual({ x: 0.5, y: 0.5 })
    expect(pointToFraction(rect, 100, 50)).toEqual({ x: 0, y: 0 })
  })

  it('clamps a pointer dragged past the edge rather than storing an off-board card', () => {
    // The wire contract is 0..1; a card dropped past the right edge belongs at the edge,
    // not at 1.4 where nobody would ever see it again.
    expect(pointToFraction(rect, 900, 500)).toEqual({ x: 1, y: 1 })
    expect(pointToFraction(rect, -50, -50)).toEqual({ x: 0, y: 0 })
  })

  it('answers the centre for a board that has not laid out yet', () => {
    expect(pointToFraction({ left: 0, top: 0, width: 0, height: 0 }, 10, 10)).toEqual({
      x: 0.5,
      y: 0.5,
    })
  })
})

describe('clampFraction', () => {
  it('refuses NaN as well as out-of-range', () => {
    expect(clampFraction(Number.NaN)).toBe(0)
    expect(clampFraction(-1)).toBe(0)
    expect(clampFraction(2)).toBe(1)
    expect(clampFraction(0.25)).toBe(0.25)
  })
})

describe('exceedsDragThreshold', () => {
  it('treats a press that barely moved as a click, not a drag', () => {
    expect(exceedsDragThreshold(2, 2)).toBe(false)
    expect(exceedsDragThreshold(PLAY_DRAG_THRESHOLD, 0)).toBe(true)
    expect(exceedsDragThreshold(0, -PLAY_DRAG_THRESHOLD)).toBe(true)
  })
})

describe('fanLayout', () => {
  it('lays a small hand out with a gap and no overlap', () => {
    const fan = fanLayout(3, 1000, 100)
    expect(fan.overlapped).toBe(false)
    expect(fan.step).toBeGreaterThan(100)
    expect(fan.width).toBe(fan.step * 2 + 100)
  })

  it('overlaps only as much as it has to', () => {
    const fan = fanLayout(10, 500, 100)
    expect(fan.overlapped).toBe(true)
    expect(fan.width).toBeCloseTo(500, 5)
    expect(fan.step).toBeCloseTo((500 - 100) / 9, 5)
  })

  it('stops compressing at a floor, so a huge hand scrolls instead of stacking', () => {
    // Twenty cards in 200px would need a 5px step — the cards' names would be gone.
    const fan = fanLayout(20, 200, 100)
    expect(fan.step).toBeCloseTo(22, 5)
    expect(fan.width).toBeGreaterThan(200)
  })

  it('has nothing to lay out for an empty hand', () => {
    expect(fanLayout(0, 500, 100)).toEqual({ step: 0, width: 0, overlapped: false })
  })
})

describe('battlefieldLayout', () => {
  it('leaves free permanents where they are', () => {
    const items = battlefieldLayout([card({ id: 1, x: 0.2, y: 0.8 })])
    expect(items[0]).toMatchObject({ x: 0.2, y: 0.8, depth: 0 })
  })

  it('offsets an attached card and paints it behind its host', () => {
    const host = card({ id: 1, x: 0.4, y: 0.4 })
    const aura = card({ id: 2, x: 0.9, y: 0.9, attached_to: 1 })
    const items = battlefieldLayout([host, aura])
    const drawn = items.map((item) => item.card.id)
    // Deepest first: the host is painted last, so it stays readable.
    expect(drawn).toEqual([2, 1])
    const attached = items.find((item) => item.card.id === 2)
    expect(attached?.depth).toBe(1)
    expect(attached?.x).toBeCloseTo(0.412, 5)
    expect(attached?.y).toBeCloseTo(0.43, 5)
    expect(attached!.z).toBeLessThan(items.find((item) => item.card.id === 1)!.z)
  })

  it('survives an attachment cycle instead of looping forever', () => {
    // Nothing is rules-enforced at this table, so the server will happily store a cycle.
    const a = card({ id: 1, attached_to: 2 })
    const b = card({ id: 2, attached_to: 1 })
    expect(battlefieldLayout([a, b])).toHaveLength(2)
  })
})

describe('menuActionsFor', () => {
  it('offers a battlefield permanent its state, counters and every destination', () => {
    const actions = ids('battlefield')
    expect(actions).toContain('tap')
    expect(actions).toContain('face_down')
    expect(actions).toContain('plus_one')
    expect(actions).toContain('attach')
    expect(actions).toContain('clone')
    expect(actions).toContain('to_graveyard')
    expect(actions).toContain('to_command')
    // A permanent is already on the battlefield; "play" would be a no-op.
    expect(actions).not.toContain('play')
  })

  it('flips tap/untap and face up/down to whichever is the opposite of now', () => {
    expect(ids('battlefield', true, 'commander', { tapped: true })).toContain('untap')
    expect(ids('battlefield', true, 'commander', { tapped: true })).not.toContain('tap')
    expect(ids('battlefield', true, 'commander', { face_down: true })).toContain('face_up')
  })

  it('offers detach instead of attach once a card is attached', () => {
    const actions = ids('battlefield', true, 'commander', { attached_to: 9 })
    expect(actions).toContain('detach')
    expect(actions).not.toContain('attach')
  })

  it('offers a hand card the two ways to play it and revealing, but never tapping', () => {
    const actions = ids('hand')
    expect(actions).toEqual(
      expect.arrayContaining(['play', 'play_face_down', 'reveal', 'to_library_bottom']),
    )
    expect(actions).not.toContain('tap')
    // You can't send a card to the hand it is already in.
    expect(actions).not.toContain('to_hand')
  })

  it('calls playing from the command zone casting', () => {
    const actions = menuActionsFor(card({ zone: 'command' }), 'command', true, 'commander')
    expect(actions.find((action) => action.id === 'play')?.label).toBe('Cast')
  })

  it('offers someone else only what the engine would actually allow', () => {
    // Their permanent: take control of it, or look at it. Nothing else would be accepted.
    expect(ids('battlefield', false)).toEqual(['take_control', 'view'])
    expect(ids('graveyard', false)).toEqual(['view'])
    // Their hand isn't even visible, so there is nothing to offer at all.
    expect(ids('hand', false)).toEqual([])
  })

  it('hides the command zone outside Commander', () => {
    expect(ids('battlefield', true, 'constructed')).not.toContain('to_command')
    expect(ids('battlefield', true, 'commander')).toContain('to_command')
  })

  it('only steps loyalty on something that has loyalty', () => {
    expect(ids('battlefield')).not.toContain('loyalty_up')
    const planeswalker = card({
      def: {
        ...card().def!,
        faces: [
          {
            name: 'Jace',
            mana_cost: '{1}{U}',
            type_line: 'Planeswalker',
            oracle_text: null,
            power: null,
            toughness: null,
            loyalty: '3',
          },
        ],
      },
    })
    expect(
      menuActionsFor(planeswalker, 'battlefield', true, 'commander').map((a) => a.id),
    ).toContain('loyalty_up')
  })

  it('only offers transform to a card with a second face', () => {
    expect(ids('battlefield')).not.toContain('transform')
    const dfc = card({
      def: { ...card().def!, faces: [card().def!.faces[0]!, card().def!.faces[0]!] },
    })
    expect(menuActionsFor(dfc, 'battlefield', true, 'commander').map((a) => a.id)).toContain(
      'transform',
    )
  })

  it('marks a token leaving the battlefield as destructive — it stops existing', () => {
    const token = card({ def: { ...card().def!, is_token: true } })
    const actions = menuActionsFor(token, 'battlefield', true, 'commander')
    expect(actions.find((action) => action.id === 'to_graveyard')?.destructive).toBe(true)
    expect(actions.find((action) => action.id === 'tap')?.destructive).toBeUndefined()
  })
})

describe('cardAriaLabel', () => {
  it('names the card, then the state that matters', () => {
    expect(cardAriaLabel(card())).toBe('Sol Ring')
    expect(cardAriaLabel(card({ tapped: true, counters: { '+1/+1': 2 } }))).toBe(
      'Sol Ring, tapped, 2 +1/+1 counters',
    )
  })

  it('has no name to give for a card this viewer may not identify', () => {
    expect(cardAriaLabel(card({ def: null, face_down: true }))).toBe('Face-down card, face down')
  })
})

describe('counterChips', () => {
  it('drops counters that have been removed back to zero', () => {
    expect(counterChips(card({ counters: { '+1/+1': 0, charge: 3 } }))).toEqual([
      { name: 'charge', value: 3 },
    ])
  })
})

describe('logLine', () => {
  const seats = [
    { id: 1, seat_index: 0, name: 'Ana' },
    { id: 2, seat_index: 1, name: 'Bo' },
  ] as PlaySeatSnapshot[]

  function entry(overrides: Partial<PlayLogEntry>): PlayLogEntry {
    return { id: 1, at: '2026-01-01T00:00:00Z', kind: 'action', seat: 1, text: '…', ...overrides }
  }

  it('narrates an action and punctuates chat as speech', () => {
    expect(logLine(entry({ text: 'drew a card' }), seats)).toBe('Ana drew a card')
    expect(logLine(entry({ kind: 'chat', seat: 2, text: 'nice draw' }), seats)).toBe(
      'Bo: nice draw',
    )
  })

  it('leaves a system line alone, and a seat that has gone unnamed', () => {
    expect(logLine(entry({ kind: 'system', seat: null, text: 'The game began' }), seats)).toBe(
      'The game began',
    )
    expect(logLine(entry({ seat: 99, text: 'conceded' }), seats)).toBe('conceded')
  })
})

describe('zoneLabel and parseCount', () => {
  it('names every zone', () => {
    expect(zoneLabel('command')).toBe('Command zone')
    expect(zoneLabel('library')).toBe('Library')
  })

  it('clamps a prompt answer into range and rejects a non-number', () => {
    expect(parseCount('3', 1, 10)).toBe(3)
    expect(parseCount('99', 1, 10)).toBe(10)
    expect(parseCount('0', 1, 10)).toBe(1)
    expect(parseCount('', 1, 10)).toBeNull()
    expect(parseCount(null, 1, 10)).toBeNull()
  })
})
