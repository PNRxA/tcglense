import { describe, expect, it } from 'vitest'
import { LIFE_LAYOUTS, type LifeLayout, type LifeRotation } from '@/lib/api/life'
import {
  defaultLayoutFor,
  defaultRotationFor,
  layoutAvailableFor,
  layoutOptionsFor,
  matPlacement,
  resolveLayout,
  rotationClass,
  seatCellStyle,
} from '@/lib/lifeLayout'

// The layout maths is the part of the life counter that is easy to get subtly wrong and hard to
// see in a screenshot, so it's tested here rather than through a mounted mat: a hole in the grid,
// a seat sized from the wrong axis after a quarter turn, or a rotation the server would reject.

const COUNTS = [1, 2, 3, 4, 5, 6] as const

/** How many tracks a `grid-template-*` value declares. */
function trackCount(template: string): number {
  const repeat = /repeat\((\d+)/.exec(template)
  if (repeat) return Number(repeat[1])
  return template.trim().split(/\s+/).length
}

/**
 * One axis of a seat's grid shorthand: where it starts (null = leave it to auto-placement) and
 * how many tracks it covers. Handles the three forms the placement maths emits — `span N`,
 * a bare line number, and `N / span M`.
 */
function parseTrack(value: string): { start: number | null; span: number } {
  const spanOnly = /^span (\d+)$/.exec(value)
  if (spanOnly) return { start: null, span: Number(spanOnly[1]) }
  const startSpan = /^(\d+) \/ span (\d+)$/.exec(value)
  if (startSpan) return { start: Number(startSpan[1]), span: Number(startSpan[2]) }
  return { start: Number(value), span: 1 }
}

/**
 * Every cell a placement covers, as `row:col` keys — so a hole or an overlap is visible.
 *
 * Models CSS auto-placement closely enough for these grids: a seat with a definite line on an
 * axis is pinned there, and an auto one takes the first free block big enough for its span.
 * The two-bank layouts pin both axes precisely so they don't depend on this model at all.
 */
function coveredCells(layout: LifeLayout, count: number): string[] {
  const placement = matPlacement(layout, count)
  const columnCount = trackCount(placement.columns)
  const rowCount = trackCount(placement.rows)
  const taken = new Set<string>()
  const cells: string[] = []

  const block = (row: number, column: number, rowSpan: number, columnSpan: number) =>
    Array.from({ length: rowSpan }, (_, r) =>
      Array.from({ length: columnSpan }, (_, c) => `${row + r}:${column + c}`),
    ).flat()

  for (const seat of placement.seats) {
    const { start: columnStart, span: columnSpan } = parseTrack(seat.column)
    const { start: rowStart, span: rowSpan } = parseTrack(seat.row)
    const candidateRows =
      rowStart !== null
        ? [rowStart]
        : Array.from({ length: Math.max(1, rowCount - rowSpan + 1) }, (_, i) => i + 1)
    const candidateColumns =
      columnStart !== null
        ? [columnStart]
        : Array.from({ length: Math.max(1, columnCount - columnSpan + 1) }, (_, i) => i + 1)

    let placedRow = -1
    let placedColumn = -1
    for (const row of candidateRows) {
      for (const column of candidateColumns) {
        if (block(row, column, rowSpan, columnSpan).every((key) => !taken.has(key))) {
          placedRow = row
          placedColumn = column
          break
        }
      }
      if (placedRow !== -1) break
    }
    expect(placedRow, `${layout}/${count}: a seat had nowhere to go`).toBeGreaterThan(0)
    for (const key of block(placedRow, placedColumn, rowSpan, columnSpan)) {
      cells.push(key)
      taken.add(key)
    }
  }
  return cells
}

describe('layout vocabulary', () => {
  it('matches the server, which validates against its own copy of the list', () => {
    // Mirrors LAYOUTS in api/src/handlers/tools/life/mod.rs. A slug added on one side only
    // would either be rejected by the API or render as something else, so pin them together.
    expect([...LIFE_LAYOUTS].sort()).toEqual([
      'facing',
      'facing-solo',
      'grid',
      'pinwheel',
      'rows',
      'sides',
      'sides-solo',
    ])
  })

  it('defaults to the arrangement each player count is normally played in', () => {
    expect(defaultLayoutFor(1)).toBe('rows')
    expect(defaultLayoutFor(2)).toBe('facing')
    expect(defaultLayoutFor(3)).toBe('facing')
    expect(defaultLayoutFor(4)).toBe('pinwheel')
    expect(defaultLayoutFor(6)).toBe('grid')
  })

  it('only offers a layout at counts where it renders as its own name', () => {
    const slugs = (count: number) => layoutOptionsFor(count).map((option) => option.value)
    // A lone player has no table to sit around and nobody to face.
    expect(slugs(1)).not.toContain('facing')
    expect(slugs(1)).not.toContain('pinwheel')
    // "Around the table" is a pod of three or four; at six it would just be the grid.
    expect(slugs(4)).toContain('pinwheel')
    expect(slugs(6)).not.toContain('pinwheel')
    // A `-solo` variant only exists where a bank actually takes an odd seat — at an even count
    // it draws the identical mat to its sibling, which is a choice with no consequence.
    for (const solo of ['facing-solo', 'sides-solo'] as const) {
      expect(slugs(3)).toContain(solo)
      expect(slugs(5)).toContain(solo)
      expect(slugs(2)).not.toContain(solo)
      expect(slugs(4)).not.toContain(solo)
      expect(slugs(1)).not.toContain(solo)
    }
    // Both axes stay on offer wherever they fit: the picker can't tell which way round the
    // device is being held, so the landscape split is a choice, never inferred.
    for (const count of [2, 3, 4, 5, 6]) expect(slugs(count)).toContain('sides')
    expect(slugs(1)).not.toContain('sides')
    // The count's default is offered first, so the preselected option is the top one.
    for (const count of COUNTS) expect(slugs(count)[0]).toBe(defaultLayoutFor(count))
    // Every offered option agrees with the predicate the new-game dialog resets against.
    for (const count of COUNTS) {
      for (const layout of LIFE_LAYOUTS) {
        expect(slugs(count).includes(layout), `${layout}/${count}`).toBe(
          layoutAvailableFor(layout, count),
        )
      }
    }
  })

  it('describes a two-bank table by the split it will actually draw', () => {
    const hint = (layout: LifeLayout, count: number) =>
      layoutOptionsFor(count).find((option) => option.value === layout)?.hint
    // The four three-player arrangements are told apart by their copy, not just their preview —
    // and each names the edge its lone seat is on, which is the whole point of having four.
    expect(hint('facing', 3)).toBe('2 on your side, one across')
    expect(hint('facing-solo', 3)).toBe('You alone, 2 across the table')
    expect(hint('sides', 3)).toBe('2 on the left, one on the right')
    expect(hint('sides-solo', 3)).toBe('You alone on the left, 2 on the right')
    // The wording follows the count, not the slug.
    expect(hint('facing', 5)).toBe('3 on your side, 2 across')
    expect(hint('sides-solo', 5)).toBe('2 on the left, 3 on the right')
  })

  it('falls back to a renderable layout for an unknown stored slug', () => {
    // A session written by a newer build must still render rather than blanking the mat.
    expect(resolveLayout('spiral', 4)).toBe('pinwheel')
    expect(resolveLayout('', 2)).toBe('facing')
    expect(resolveLayout('grid', 4)).toBe('grid')
  })
})

describe('default rotations', () => {
  it('mirrors the server so a rematch reproduces the same table', () => {
    // Mirrors default_rotation_for in api/src/handlers/tools/life/mod.rs.
    const facing = (count: number) =>
      Array.from({ length: count }, (_, position) => defaultRotationFor('facing', position, count))
    expect(facing(2)).toEqual([0, 180])
    expect(facing(3)).toEqual([0, 0, 180])
    expect(facing(5)).toEqual([0, 0, 0, 180, 180])

    const rotations = (layout: LifeLayout) => (count: number) =>
      Array.from({ length: count }, (_, position) => defaultRotationFor(layout, position, count))

    // The odd seat goes across instead of staying in your bank — the mirror of the plain split.
    const solo = rotations('facing-solo')
    expect(solo(3)).toEqual([0, 180, 180])
    expect(solo(5)).toEqual([0, 0, 180, 180, 180])
    // An even table splits evenly either way, so the two coincide there.
    expect(solo(4)).toEqual(facing(4))

    // The left/right axis is the same split read from the side edges instead.
    expect(rotations('sides')(3)).toEqual([90, 90, 270])
    expect(rotations('sides-solo')(3)).toEqual([90, 270, 270])
    expect(rotations('sides')(5)).toEqual([90, 90, 90, 270, 270])
    expect(rotations('sides-solo')(5)).toEqual([90, 90, 270, 270, 270])

    const pinwheel = (count: number) =>
      Array.from({ length: count }, (_, position) =>
        defaultRotationFor('pinwheel', position, count),
      )
    expect(pinwheel(4)).toEqual([0, 90, 180, 270])
    expect(pinwheel(3)).toEqual([0, 90, 270])
  })

  it('keeps held layouts upright and never turns a lone seat away from its reader', () => {
    expect(defaultRotationFor('rows', 3, 5)).toBe(0)
    expect(defaultRotationFor('grid', 2, 4)).toBe(0)
    for (const layout of LIFE_LAYOUTS) expect(defaultRotationFor(layout, 0, 1)).toBe(0)
  })

  it('only ever produces rotations the server will store', () => {
    for (const layout of LIFE_LAYOUTS) {
      for (const count of COUNTS) {
        for (let position = 0; position < count; position += 1) {
          expect([0, 90, 180, 270]).toContain(defaultRotationFor(layout, position, count))
        }
      }
    }
  })
})

describe('mat placement', () => {
  it('places exactly one cell per seat, at every layout and count', () => {
    for (const layout of LIFE_LAYOUTS) {
      for (const count of COUNTS) {
        expect(matPlacement(layout, count).seats).toHaveLength(count)
      }
    }
  })

  it('fills the whole mat with no holes and no overlaps', () => {
    for (const layout of LIFE_LAYOUTS) {
      for (const count of COUNTS) {
        const cells = coveredCells(layout, count)
        // No cell claimed twice...
        expect(new Set(cells).size, `${layout}/${count} overlaps`).toBe(cells.length)
        // ...and the cells exactly tile the declared grid, so no gap is left showing.
        const placement = matPlacement(layout, count)
        expect(cells.length, `${layout}/${count} does not tile its grid`).toBe(
          trackCount(placement.columns) * trackCount(placement.rows),
        )
      }
    }
  })

  it('splits a facing table into two banks, the far one upside-down', () => {
    // Three players: two near, one far spanning the whole top row.
    const three = matPlacement('facing', 3)
    expect(three.rows).toBe('repeat(2, minmax(0, 1fr))')
    expect(three.seats.map((seat) => seat.row)).toEqual(['2', '2', '1'])
    expect(three.seats.map((seat) => seat.rotation)).toEqual([0, 0, 180])
    expect(three.seats[2]?.column).toBe('1 / span 2')

    // Five players: banks of three and two over a shared six-column track, so every tile in
    // a bank is the same width and the row has no gap.
    const five = matPlacement('facing', 5)
    expect(five.columns).toBe('repeat(6, minmax(0, 1fr))')
    expect(five.seats.map((seat) => seat.column)).toEqual([
      '1 / span 2',
      '3 / span 2',
      '5 / span 2',
      '1 / span 3',
      '4 / span 3',
    ])
  })

  it('mirrors that split for a solo variant, so the lone seat can be either bank', () => {
    // The gap this closes: with `facing` the lone seat is always the far one. Here it's yours —
    // one tile along the bottom, two upside-down across the top.
    const three = matPlacement('facing-solo', 3)
    expect(three.seats.map((seat) => seat.row)).toEqual(['2', '1', '1'])
    expect(three.seats.map((seat) => seat.rotation)).toEqual([0, 180, 180])
    expect(three.seats.map((seat) => seat.column)).toEqual([
      '1 / span 2',
      '1 / span 1',
      '2 / span 1',
    ])

    // Exactly the mirror of `facing` at the same count: same grid, opposite banks.
    const facing = matPlacement('facing', 3)
    expect(three.columns).toBe(facing.columns)
    expect(three.rows).toBe(facing.rows)
    expect(three.seats.map((seat) => seat.row)).toEqual(
      facing.seats.map((seat) => (seat.row === '1' ? '2' : '1')).reverse(),
    )
  })

  it('turns the same two banks a quarter turn for a lengthways device', () => {
    // `sides` is `facing` rotated: the banks become left and right columns, read from the left
    // (90°) and right (270°) edges. This is the split a landscape screen wants.
    const three = matPlacement('sides', 3)
    expect(three.columns).toBe('repeat(2, minmax(0, 1fr))')
    expect(three.rows).toBe('repeat(2, minmax(0, 1fr))')
    expect(three.seats.map((seat) => seat.column)).toEqual(['1', '1', '2'])
    expect(three.seats.map((seat) => seat.row)).toEqual(['1 / span 1', '2 / span 1', '1 / span 2'])
    expect(three.seats.map((seat) => seat.rotation)).toEqual([90, 90, 270])

    // And its solo variant moves the lone seat to the *left* — the landscape half of the ask.
    const solo = matPlacement('sides-solo', 3)
    expect(solo.seats.map((seat) => seat.column)).toEqual(['1', '2', '2'])
    expect(solo.seats.map((seat) => seat.row)).toEqual(['1 / span 2', '1 / span 1', '2 / span 1'])
    expect(solo.seats.map((seat) => seat.rotation)).toEqual([90, 270, 270])

    // Both axes divide their tracks the same way — only which axis is split differs.
    for (const count of COUNTS) {
      const near = matPlacement('facing', count)
      const side = matPlacement('sides', count)
      expect(side.columns, `sides/${count}`).toBe(near.rows)
      expect(side.rows, `sides/${count}`).toBe(near.columns)
    }
  })

  it('pins both lines of a two-bank seat rather than trusting auto-placement', () => {
    // A `left-right` bank is a definite column with a row span, and the auto-placement cursor
    // only moves forward — by the second bank it has advanced past the top of its column and
    // would grow a phantom row. Every two-bank cell therefore names its start line.
    for (const layout of ['facing', 'facing-solo', 'sides', 'sides-solo'] as const) {
      for (const count of COUNTS.filter((n) => n > 1)) {
        for (const seat of matPlacement(layout, count).seats) {
          expect(seat.column.startsWith('span '), `${layout}/${count} column`).toBe(false)
          expect(seat.row.startsWith('span '), `${layout}/${count} row`).toBe(false)
        }
      }
    }
  })

  it('seats a pinwheel one per edge, each cell on the side its player sits on', () => {
    const four = matPlacement('pinwheel', 4)
    // near = bottom-left, left = top-left, far = top-right, right = bottom-right.
    expect(four.seats.map((seat) => [seat.column, seat.row, seat.rotation])).toEqual([
      ['1', '2', 0],
      ['1', '1', 90],
      ['2', '1', 180],
      ['2', '2', 270],
    ])
  })

  it('gives a lone seat the whole mat whatever the layout says', () => {
    for (const layout of LIFE_LAYOUTS) {
      const placement = matPlacement(layout, 1)
      expect(placement.columns).toBe('1fr')
      expect(placement.rows).toBe('1fr')
      expect(placement.seats[0]?.rotation).toBe(0)
    }
  })

  it('spans an odd last seat across the grid rather than leaving a hole', () => {
    const five = matPlacement('grid', 5)
    expect(five.seats[4]?.column).toBe('span 2')
    expect(five.seats[0]?.column).toBe('span 1')
  })

  it('lets a stored rotation override the layout, since a player may sit anywhere', () => {
    const rotations: (LifeRotation | undefined)[] = [90, undefined]
    const placement = matPlacement('facing', 2, rotations)
    expect(placement.seats[0]?.rotation).toBe(90)
    // The unset seat still takes the layout's own.
    expect(placement.seats[1]?.rotation).toBe(180)
  })

  it('is empty, not broken, for a table with no seats', () => {
    expect(matPlacement('grid', 0).seats).toEqual([])
    expect(matPlacement('grid', -3).seats).toEqual([])
  })
})

describe('seat cell style', () => {
  it('swaps the tile axes on a quarter turn so a rotated tile still fills its cell', () => {
    const upright = seatCellStyle({ column: '1', row: '1', rotation: 0 })
    expect(upright['--life-tile-w']).toBe('100cqw')
    expect(upright['--life-tile-h']).toBe('100cqh')

    // A 90/270 tile is laid out along the cell's other axis before being turned into place.
    for (const rotation of [90, 270] as const) {
      const turned = seatCellStyle({ column: '1', row: '1', rotation })
      expect(turned['--life-tile-w']).toBe('100cqh')
      expect(turned['--life-tile-h']).toBe('100cqw')
    }

    // A half turn keeps the axes: it's still the same box, just upside-down.
    const flipped = seatCellStyle({ column: '1', row: '1', rotation: 180 })
    expect(flipped['--life-tile-w']).toBe('100cqw')
  })

  it('passes the grid shorthand straight through', () => {
    const style = seatCellStyle({ column: 'span 2', row: '2', rotation: 0 })
    expect(style.gridColumn).toBe('span 2')
    expect(style.gridRow).toBe('2')
  })
})

describe('rotationClass', () => {
  it('maps each stored rotation to a utility, upright to none', () => {
    expect(rotationClass(0)).toBe('')
    expect(rotationClass(90)).toBe('rotate-90')
    expect(rotationClass(180)).toBe('rotate-180')
    // 270 clockwise is a quarter turn anticlockwise — the shorter class Tailwind ships.
    expect(rotationClass(270)).toBe('-rotate-90')
  })
})
