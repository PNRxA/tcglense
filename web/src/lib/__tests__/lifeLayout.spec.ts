import { describe, expect, it } from 'vitest'
import { LIFE_LAYOUTS, type LifeLayout, type LifeRotation } from '@/lib/api/life'
import {
  defaultLayoutFor,
  defaultRotationFor,
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
 * Every cell a placement covers, as `row:col` keys — so a hole or an overlap is visible.
 *
 * Models CSS auto-placement closely enough for these grids: a seat with a definite row packs
 * along that row, a fully auto seat takes the first run of free cells wide enough for its span.
 */
function coveredCells(layout: LifeLayout, count: number): string[] {
  const placement = matPlacement(layout, count)
  const columnCount = trackCount(placement.columns)
  const rowCount = trackCount(placement.rows)
  const taken = new Set<string>()
  const cells: string[] = []

  for (const seat of placement.seats) {
    const span = seat.column.startsWith('span ') ? Number(seat.column.slice(5)) : 1
    const explicitColumn = seat.column.startsWith('span ') ? null : Number(seat.column)
    const explicitRow = seat.row.startsWith('span ') ? null : Number(seat.row)
    const candidateRows =
      explicitRow !== null ? [explicitRow] : Array.from({ length: rowCount }, (_, i) => i + 1)
    const candidateColumns =
      explicitColumn !== null
        ? [explicitColumn]
        : Array.from({ length: Math.max(1, columnCount - span + 1) }, (_, i) => i + 1)

    let placedRow = -1
    let placedColumn = -1
    for (const row of candidateRows) {
      for (const column of candidateColumns) {
        const free = Array.from({ length: span }, (_, o) => `${row}:${column + o}`).every(
          (key) => !taken.has(key),
        )
        if (free) {
          placedRow = row
          placedColumn = column
          break
        }
      }
      if (placedRow !== -1) break
    }
    expect(placedRow, `${layout}/${count}: a seat had nowhere to go`).toBeGreaterThan(0)
    for (let offset = 0; offset < span; offset += 1) {
      const key = `${placedRow}:${placedColumn + offset}`
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
    expect([...LIFE_LAYOUTS].sort()).toEqual(['facing', 'grid', 'pinwheel', 'rows'])
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
    // The count's default is offered first, so the preselected option is the top one.
    for (const count of COUNTS) expect(slugs(count)[0]).toBe(defaultLayoutFor(count))
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
    expect(three.seats[2]?.column).toBe('span 2')

    // Five players: banks of three and two over a shared six-column track, so every tile in
    // a bank is the same width and the row has no gap.
    const five = matPlacement('facing', 5)
    expect(five.columns).toBe('repeat(6, minmax(0, 1fr))')
    expect(five.seats.map((seat) => seat.column)).toEqual([
      'span 2',
      'span 2',
      'span 2',
      'span 3',
      'span 3',
    ])
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
