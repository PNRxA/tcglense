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
 * The **two-bank** layouts: two facing edges of the device with the seats split between them.
 *
 * Four of them, from two independent choices — which is the honest model, because each
 * combination is a different table rather than a different look at the same one:
 *
 * - **`axis`** — which pair of edges the banks sit on. `near-far` splits the mat into a bottom
 *   and a top row, for a device lying with its short edge towards you; `left-right` splits it
 *   into a left and a right column, for one lying lengthways *between* the two sides. On a
 *   landscape screen that second split is the one that matches the room.
 * - **`oddSeatCrosses`** — where the extra seat of an odd table goes. `false` keeps it in your
 *   own bank (three players = two here, one across); `true` sends it over (three players = you
 *   alone, two across). That's what lets the lone seat be on *either* side of the table.
 *
 * Neither is a rotation or a seat order a player can nudge into place, so each combination gets
 * its own slug. At an even count the banks split evenly and the `oddSeatCrosses` pair coincides
 * — see {@link layoutAvailableFor}.
 *
 * Mirrors `two_bank` in `api/src/handlers/tools/life/mod.rs`.
 */
interface TwoBank {
  axis: 'near-far' | 'left-right'
  oddSeatCrosses: boolean
  /** Rotation for a seat in your own bank, and for one in the bank opposite. */
  rotations: [LifeRotation, LifeRotation]
}

const TWO_BANK_LAYOUTS = [
  'facing',
  'facing-solo',
  'sides',
  'sides-solo',
] as const satisfies readonly LifeLayout[]
type TwoBankLayout = (typeof TWO_BANK_LAYOUTS)[number]

const TWO_BANK: Record<TwoBankLayout, TwoBank> = {
  facing: { axis: 'near-far', oddSeatCrosses: false, rotations: [0, 180] },
  'facing-solo': { axis: 'near-far', oddSeatCrosses: true, rotations: [0, 180] },
  // A left-edge player reads at 90° and a right-edge one at 270° — the same convention the
  // pinwheel's side seats use, so a seat's cell is always on the side its player sits on.
  sides: { axis: 'left-right', oddSeatCrosses: false, rotations: [90, 270] },
  'sides-solo': { axis: 'left-right', oddSeatCrosses: true, rotations: [90, 270] },
}

/** The two-bank description of `layout`, or `undefined` for the single-bank layouts. */
function twoBankOf(layout: LifeLayout): TwoBank | undefined {
  return (TWO_BANK_LAYOUTS as readonly string[]).includes(layout)
    ? TWO_BANK[layout as TwoBankLayout]
    : undefined
}

/**
 * How many seats sit in **your own** bank of a two-bank table — the near one for a `near-far`
 * split, the left one for `left-right`, in both cases the side the device is operated from.
 *
 * Mirrors `near_bank_size` in `api/src/handlers/tools/life/mod.rs`.
 */
function nearBankSize(layout: LifeLayout, playerCount: number): number {
  return twoBankOf(layout)?.oddSeatCrosses
    ? Math.floor(playerCount / 2)
    : Math.ceil(playerCount / 2)
}

/**
 * How a two-bank table's split reads in words.
 *
 * Worded from the bank sizes and the axis rather than fixed per slug, because that split is the
 * *whole* difference between the four two-bank layouts and it changes with the player count —
 * "you alone, two across" and "two on your side, three across" are the same layout at three
 * seats and at five.
 */
function bankHint(axis: TwoBank['axis'], near: number, far: number): string {
  if (axis === 'left-right') {
    if (near === 1 && far === 1) return 'Lengthways between two sides'
    if (near === 1) return `You alone on the left, ${far} on the right`
    if (far === 1) return `${near} on the left, one on the right`
    return `${near} on the left, ${far} on the right`
  }
  if (near === 1 && far === 1) return 'Across a table, flat in the middle'
  if (near === 1) return `You alone, ${far} across the table`
  if (far === 1) return `${near} on your side, one across`
  return `${near} on your side, ${far} across`
}

/** The hint for a two-bank slug, resolved from the split it will actually draw at this count. */
function twoBankHint(layout: TwoBankLayout, playerCount: number): string {
  const near = nearBankSize(layout, playerCount)
  return bankHint(TWO_BANK[layout].axis, near, playerCount - near)
}

const OPTIONS: Record<LifeLayout, { label: string; hint: (playerCount: number) => string }> = {
  rows: { label: 'Stacked', hint: () => 'One row each — someone holds the phone' },
  grid: { label: 'Grid', hint: () => 'Two columns, all upright — a held tablet' },
  facing: { label: 'Facing', hint: (count) => twoBankHint('facing', count) },
  // Labelled by the edge *you* sit at when you're the lone seat, in the same table-edge
  // vocabulary as ROTATION_OPTIONS — "near" and "left" are the two sides the odd seat can take.
  'facing-solo': { label: 'Solo near', hint: (count) => twoBankHint('facing-solo', count) },
  sides: { label: 'Sideways', hint: (count) => twoBankHint('sides', count) },
  'sides-solo': { label: 'Solo left', hint: (count) => twoBankHint('sides-solo', count) },
  pinwheel: { label: 'Around the table', hint: () => 'One seat per edge, each facing in' },
}

/** Preference order for the picker, before the count's own default is pulled to the front. */
const PREFERENCE: LifeLayout[] = [
  'facing',
  'facing-solo',
  'sides',
  'sides-solo',
  'pinwheel',
  'grid',
  'rows',
]

/**
 * Whether `layout` is worth offering at `playerCount` — an option that renders as something
 * other than its own name is worse than no option.
 *
 * A two-bank layout needs two banks, so it wants at least two seats — and a `-solo` variant
 * differs from its sibling only in which bank takes the *odd* seat, so at an even count the two
 * draw the identical mat and offering both would be a choice with no consequence. `pinwheel`
 * reads as a table only for three or four seats. `rows` and `grid` are held-device layouts and
 * work at any count.
 *
 * Note both axes stay on offer at every count they fit: the picker can't see which way round the
 * device is being held, and a pod may well rotate it mid-game, so `sides` is a choice rather
 * than something inferred from the viewport.
 *
 * Shared with the new-game dialog, which has to move a no-longer-valid layout off when the
 * player count changes — one predicate so the picker and that reset can't disagree.
 */
export function layoutAvailableFor(layout: LifeLayout, playerCount: number): boolean {
  const bank = twoBankOf(layout)
  if (bank) {
    return bank.oddSeatCrosses ? playerCount >= 3 && playerCount % 2 === 1 : playerCount >= 2
  }
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

/** The rotation a layout seats each position at — mirrors the server's default. */
export function defaultRotationFor(
  layout: LifeLayout,
  position: number,
  playerCount: number,
): LifeRotation {
  // A single seat is the whole screen and always reads upright.
  if (playerCount <= 1) return 0
  const bank = twoBankOf(layout)
  if (bank) {
    const [own, opposite] = bank.rotations
    return position >= nearBankSize(layout, playerCount) ? opposite : own
  }
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

  const bank = twoBankOf(layout)
  if (bank) {
    // Two banks on opposite edges, one per track of the split axis: for `near-far` the bottom
    // row is yours and the top row is across; for `left-right` the left column is yours and the
    // right column is across. Either way you read your own total from the edge you're sitting at.
    //
    // The banks can hold different numbers of seats, so the *other* axis is divided into the
    // smallest number of equal tracks both bank sizes fit — each seat spans `tracks/bank`. That
    // keeps every tile in a bank the same size and leaves no gap, at 3 seats (2 vs 1, or 1 vs 2
    // for a `-solo` variant) as much as at 5 (3 vs 2).
    const near = nearBankSize(layout, count)
    const far = count - near
    const tracks = far === 0 ? near : (near * far) / gcd(near, far)

    // Both lines are stated explicitly rather than left to auto-placement. A `left-right` bank
    // is a definite *column* with a row span, and the grid auto-placement cursor only ever moves
    // forward: by the time the second bank is placed the cursor has advanced past the top of its
    // column, so it would grow a phantom extra row instead of starting again at the top. Naming
    // the start line sidesteps the whole cursor — and doing it on both axes keeps one code path.
    // Your own bank is the bottom row of a `near-far` mat and the left column of a `left-right`
    // one; the bank opposite takes the other track.
    const nearFar = bank.axis === 'near-far'
    const ownTrack = nearFar ? '2' : '1'
    const oppositeTrack = nearFar ? '1' : '2'

    const seats = Array.from({ length: count }, (_, position) => {
      const isNear = position < near
      const span = tracks / (isNear ? near : far)
      const start = (isNear ? position : position - near) * span + 1
      const split = isNear ? ownTrack : oppositeTrack
      const along = `${start} / span ${span}`
      return {
        column: nearFar ? along : split,
        row: nearFar ? split : along,
        rotation: rotationAt(position),
      }
    })

    const split = 'repeat(2, minmax(0, 1fr))'
    const along = `repeat(${tracks}, minmax(0, 1fr))`
    return nearFar ? { columns: along, rows: split, seats } : { columns: split, rows: along, seats }
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

/**
 * The rotations the seat control offers.
 *
 * `arrow` points at the edge of the *screen* the seat's player is sitting at, which is what the
 * control draws — "they're over there" is a direction you can point to, where "90°" is a number
 * you have to decode. Note it is deliberately **not** the rotation applied to the tile: turning
 * a tile 90° clockwise makes it readable from the **left**, so an arrow drawn from the rotation
 * would point a left-hand player to the right.
 *
 * `label` stays the accessible name — an icon-only control still has to say what it does.
 */
export const ROTATION_OPTIONS: {
  value: LifeRotation
  label: string
  arrow: 'up' | 'down' | 'left' | 'right'
}[] = [
  { value: 0, label: 'Sitting at the near edge', arrow: 'down' },
  { value: 90, label: 'Sitting at the left edge', arrow: 'left' },
  { value: 180, label: 'Sitting at the far edge', arrow: 'up' },
  { value: 270, label: 'Sitting at the right edge', arrow: 'right' },
]

/** Starting-life presets: 20 for a duel, 30 for Brawl, 40 for Commander. */
export const STARTING_LIFE_PRESETS = [20, 30, 40] as const

/** The player counts the setup dialog offers. */
export const PLAYER_COUNT_OPTIONS = [1, 2, 3, 4, 5, 6] as const

/** Below this many games a win rate is too noisy to quote as one — say so instead. */
export const WIN_RATE_MIN_GAMES = 5

/** At or below this many life a seat is in danger, and the tile says so. */
export const DANGER_LIFE = 5
