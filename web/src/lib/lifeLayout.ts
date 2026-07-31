import { LIFE_LAYOUTS, type LifeLayout, type LifeRotation } from '@/lib/api/life'

/**
 * How a tracked game's seats are placed on screen.
 *
 * The whole "layout configurations to support different amounts and placement of players"
 * problem lives here, as pure functions, because it's the part that's easy to get subtly wrong
 * and hard to see in a screenshot: each arrangement wants different cell spans at each player
 * count, and a rotated seat's tap targets have to rotate with it.
 *
 * **Rotation convention.** A seat's rotation is degrees *clockwise* applied to its tile
 * content, and it names the table edge that seat reads from: `0` = the near edge (whoever holds
 * the device), `90` = the left edge, `180` = the far edge, `270` = the right. That mapping isn't
 * arbitrary — a clockwise quarter turn sends the text's "up" from the near edge to the left one.
 *
 * **The cell never rotates, its content does.** Rotating the grid cell would rotate its box in
 * the grid too, so a quarter-turned seat would overflow its column. Instead each cell stays
 * axis-aligned and the tile inside is turned, with its width and height swapped for the quarter
 * turns (via the `--life-tile-w/h` container-query units {@link seatCellStyle} sets) so the
 * turned tile still fills the cell. Tap targets live inside the rotated content, so they follow
 * it for free — the player across the table taps the half of the tile that is "up" from where
 * *they* are sitting.
 *
 * The vocabulary is mirrored on the server (`LAYOUTS` in `api/src/handlers/tools/life/mod.rs`),
 * which validates against its own copy; `layoutsMatchServer` in the spec pins the two together
 * so adding a slug on one side fails a test rather than a request.
 */

/** A layout plus the copy the picker shows for it. */
export interface LayoutOption {
  value: LifeLayout
  label: string
  /** Why you'd pick it — the physical arrangement it matches, at this player count. */
  hint: string
}

/**
 * How a two-bank table's split reads in words.
 *
 * Worded from the bank sizes rather than fixed per slug because that split is the *whole*
 * difference between `facing` and `facing-solo`, and it changes with the player count — "you
 * alone, two across" and "two on your side, three across" are the same layout at three seats
 * and at five.
 */
function bankHint(near: number, far: number): string {
  if (near === 1 && far === 1) return 'Across a table, flat in the middle'
  if (near === 1) return `You alone, ${far} across the table`
  if (far === 1) return `${near} on your side, one across`
  return `${near} on your side, ${far} across`
}

const OPTIONS: Record<LifeLayout, { label: string; hint: (playerCount: number) => string }> = {
  rows: { label: 'Stacked', hint: () => 'One row each — someone holds the phone' },
  grid: { label: 'Grid', hint: () => 'Two columns, all upright — a held tablet' },
  facing: {
    label: 'Facing',
    hint: (count) => bankHint(nearBankSize('facing', count), count - nearBankSize('facing', count)),
  },
  'facing-solo': {
    label: 'Solo side',
    hint: (count) =>
      bankHint(nearBankSize('facing-solo', count), count - nearBankSize('facing-solo', count)),
  },
  pinwheel: { label: 'Around the table', hint: () => 'One seat per edge, each facing in' },
}

/** Preference order for the picker, before the count's own default is pulled to the front. */
const PREFERENCE: LifeLayout[] = ['facing', 'facing-solo', 'pinwheel', 'grid', 'rows']

/**
 * Whether `layout` is worth offering at `playerCount` — an option that renders as something
 * other than its own name is worse than no option.
 *
 * `facing` needs two banks, and `pinwheel` reads as a table only for three or four seats.
 * `facing-solo` differs from `facing` only in which bank takes the odd seat, so at an even count
 * the two draw the identical mat and offering both would be a choice with no consequence.
 * `rows` and `grid` are held-device layouts and work at any count.
 *
 * Shared with the new-game dialog, which has to move a no-longer-valid layout off when the
 * player count changes — one predicate so the picker and that reset can't disagree.
 */
export function layoutAvailableFor(layout: LifeLayout, playerCount: number): boolean {
  if (layout === 'facing') return playerCount >= 2
  if (layout === 'facing-solo') return playerCount >= 3 && playerCount % 2 === 1
  if (layout === 'pinwheel') return playerCount === 3 || playerCount === 4
  return true
}

/** The layouts worth offering for a player count, in preference order with the default first. */
export function layoutOptionsFor(playerCount: number): LayoutOption[] {
  const available = PREFERENCE.filter((slug) => layoutAvailableFor(slug, playerCount))
  const preferred = defaultLayoutFor(playerCount)
  const ordered = [preferred, ...available.filter((slug) => slug !== preferred)]
  return ordered.map((value) => ({
    value,
    label: OPTIONS[value].label,
    hint: OPTIONS[value].hint(playerCount),
  }))
}

/** The layout each player count is normally played in — mirrors the server's default. */
export function defaultLayoutFor(playerCount: number): LifeLayout {
  if (playerCount <= 1) return 'rows'
  if (playerCount <= 3) return 'facing'
  if (playerCount === 4) return 'pinwheel'
  return 'grid'
}

/**
 * Narrow a stored layout slug to one this build can render, falling back to the count's default.
 * A session stored by a newer build (or hand-edited) still renders rather than blanking the mat.
 */
export function resolveLayout(layout: string, playerCount: number): LifeLayout {
  return (LIFE_LAYOUTS as readonly string[]).includes(layout)
    ? (layout as LifeLayout)
    : defaultLayoutFor(playerCount)
}

/** The two-bank layouts: the same mat, differing only in which bank the odd seat joins. */
function isFacing(layout: LifeLayout): boolean {
  return layout === 'facing' || layout === 'facing-solo'
}

/**
 * How many seats sit on the near side of a two-bank table — the side the device is being
 * operated from.
 *
 * `facing` gives the odd seat to the near bank (three players = two here, one across);
 * `facing-solo` sends it across instead (three players = you alone, two across). That one
 * difference is the entire distinction between the two arrangements, and it's why they're
 * separate slugs rather than a flag: which side of the table you're sitting on is a fact about
 * the room, not something a rotation or a seat reorder can express.
 *
 * Mirrors `near_bank_size` in `api/src/handlers/tools/life/mod.rs`.
 */
function nearBankSize(layout: LifeLayout, playerCount: number): number {
  return layout === 'facing-solo' ? Math.floor(playerCount / 2) : Math.ceil(playerCount / 2)
}

/** The rotation a layout seats each position at — mirrors the server's default. */
export function defaultRotationFor(
  layout: LifeLayout,
  position: number,
  playerCount: number,
): LifeRotation {
  // A single seat is the whole screen and always reads upright.
  if (playerCount <= 1) return 0
  if (isFacing(layout)) return position >= nearBankSize(layout, playerCount) ? 180 : 0
  if (layout === 'pinwheel') {
    if (playerCount === 3) return position === 1 ? 90 : position === 2 ? 270 : 0
    if (position === 1) return 90
    if (position === 2) return 180
    if (position === 3) return 270
  }
  return 0
}

/** Placement of one seat: which grid cell it occupies and how its content is turned. */
export interface SeatPlacement {
  /** `grid-column` shorthand, e.g. `'span 2'`. */
  column: string
  /** `grid-row` shorthand. */
  row: string
  /** Content rotation in degrees. */
  rotation: LifeRotation
}

/** The grid the whole mat is laid out on, plus each seat's cell. */
export interface MatPlacement {
  /** `grid-template-columns` value. */
  columns: string
  /** `grid-template-rows` value. */
  rows: string
  seats: SeatPlacement[]
}

/** Greatest common divisor, for splitting a `facing` table's two banks over one column track. */
function gcd(a: number, b: number): number {
  return b === 0 ? a : gcd(b, a % b)
}

/**
 * Lay `playerCount` seats out in `layout`.
 *
 * Every layout fills the mat completely — no dead space, and no seat arbitrarily smaller than
 * its neighbours — which is what the odd-count handling is for: three seats in a two-column grid
 * would leave a hole, so a seat spans instead.
 *
 * `rotations` (each seat's *stored* rotation) overrides the layout's default, because a player
 * may sit somewhere the layout didn't predict — a stored rotation is the truth once set. Pass
 * `undefined` for a seat to take the layout's own.
 */
export function matPlacement(
  layout: LifeLayout,
  playerCount: number,
  rotations: (LifeRotation | undefined)[] = [],
): MatPlacement {
  const count = Math.max(0, playerCount)
  const rotationAt = (position: number): LifeRotation =>
    rotations[position] ?? defaultRotationFor(layout, position, count)

  if (count === 0) return { columns: '1fr', rows: '1fr', seats: [] }

  // One seat is always the whole mat, whatever the layout says.
  if (count === 1) {
    return {
      columns: '1fr',
      rows: '1fr',
      seats: [{ column: 'span 1', row: 'span 1', rotation: rotationAt(0) }],
    }
  }

  if (layout === 'rows') {
    return {
      columns: '1fr',
      rows: `repeat(${count}, minmax(0, 1fr))`,
      seats: Array.from({ length: count }, (_, position) => ({
        column: 'span 1',
        row: 'span 1',
        rotation: rotationAt(position),
      })),
    }
  }

  if (isFacing(layout)) {
    // Two banks, one per row: the near bank along the bottom and the far bank (rotated 180°)
    // along the top. The banks can hold different numbers of seats, so the row is divided into
    // the smallest number of equal columns both bank sizes fit — each near seat spans
    // `cols/near`, each far seat `cols/far`. That keeps every tile in a bank the same width and
    // leaves no gap, at 3 seats (2 vs 1 — or 1 vs 2 for `facing-solo`) as much as at 5 (3 vs 2).
    const near = nearBankSize(layout, count)
    const far = count - near
    const columns = far === 0 ? near : (near * far) / gcd(near, far)
    return {
      columns: `repeat(${columns}, minmax(0, 1fr))`,
      rows: 'repeat(2, minmax(0, 1fr))',
      seats: Array.from({ length: count }, (_, position) => {
        const isNear = position < near
        const span = isNear ? columns / near : columns / far
        return {
          column: `span ${span}`,
          // The near bank is the bottom row — you read your own total closest to you.
          row: isNear ? '2' : '1',
          rotation: rotationAt(position),
        }
      }),
    }
  }

  if (layout === 'pinwheel' && (count === 3 || count === 4)) {
    // Each seat's cell sits on the side of the mat its player sits on, and each rotation is a
    // quarter turn from the last — a device in the middle of the table.
    if (count === 3) {
      return {
        columns: 'repeat(2, minmax(0, 1fr))',
        rows: 'repeat(2, minmax(0, 1fr))',
        seats: [
          // The near player takes the whole bottom; nobody sits opposite them.
          { column: 'span 2', row: '2', rotation: rotationAt(0) },
          { column: '1', row: '1', rotation: rotationAt(1) },
          { column: '2', row: '1', rotation: rotationAt(2) },
        ],
      }
    }
    return {
      columns: 'repeat(2, minmax(0, 1fr))',
      rows: 'repeat(2, minmax(0, 1fr))',
      seats: [
        { column: '1', row: '2', rotation: rotationAt(0) },
        { column: '1', row: '1', rotation: rotationAt(1) },
        { column: '2', row: '1', rotation: rotationAt(2) },
        { column: '2', row: '2', rotation: rotationAt(3) },
      ],
    }
  }

  // `grid` — and the fallback for a layout used at a count it doesn't specialise for. Two
  // columns filled top-to-bottom; an odd last seat spans the full width so the mat has no hole.
  const rows = Math.ceil(count / 2)
  const odd = count % 2 === 1
  return {
    columns: 'repeat(2, minmax(0, 1fr))',
    rows: `repeat(${rows}, minmax(0, 1fr))`,
    seats: Array.from({ length: count }, (_, position) => ({
      column: odd && position === count - 1 ? 'span 2' : 'span 1',
      row: 'span 1',
      rotation: rotationAt(position),
    })),
  }
}

/**
 * Inline style for one seat's grid cell.
 *
 * A quarter turn swaps the tile's axes, so the rotated content is sized from the cell's *other*
 * dimension — without this a 90°-turned tile is laid out at the cell's width and then rotated,
 * leaving it far too narrow and overflowing the cell vertically.
 */
export function seatCellStyle(placement: SeatPlacement): Record<string, string> {
  const quarterTurn = placement.rotation === 90 || placement.rotation === 270
  return {
    gridColumn: placement.column,
    gridRow: placement.row,
    '--life-tile-w': quarterTurn ? '100cqh' : '100cqw',
    '--life-tile-h': quarterTurn ? '100cqw' : '100cqh',
  }
}

/** Tailwind rotation utility for a seat's content. */
export function rotationClass(rotation: LifeRotation): string {
  if (rotation === 90) return 'rotate-90'
  if (rotation === 180) return 'rotate-180'
  if (rotation === 270) return '-rotate-90'
  return ''
}

/** The rotations the seat control offers, labelled by the table edge each one reads from. */
export const ROTATION_OPTIONS: { value: LifeRotation; label: string }[] = [
  { value: 0, label: 'Near edge' },
  { value: 90, label: 'Left edge' },
  { value: 180, label: 'Far edge' },
  { value: 270, label: 'Right edge' },
]

/** Starting-life presets: 20 for a duel, 30 for Brawl, 40 for Commander. */
export const STARTING_LIFE_PRESETS = [20, 30, 40] as const

/** The player counts the setup dialog offers. */
export const PLAYER_COUNT_OPTIONS = [1, 2, 3, 4, 5, 6] as const

/** Below this many games a win rate is too noisy to quote as one — say so instead. */
export const WIN_RATE_MIN_GAMES = 5

/** At or below this many life a seat is in danger, and the tile says so. */
export const DANGER_LIFE = 5
