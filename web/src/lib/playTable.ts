import type { PlayCardView, PlayLogEntry, PlaySeatSnapshot, PlayZone } from '@/lib/api/play'
import { seatColor as chartSeatColor } from '@/lib/lifeSeries'

/**
 * The play table's pure maths and vocabulary — everything the table needs to decide that has
 * no DOM, no store and no socket in it.
 *
 * The table is the one surface in the app built out of pointer gestures rather than forms, so
 * the parts that are easy to get subtly wrong live here instead of inside a component: where a
 * pointer lands on the battlefield, when a press becomes a drag, how a hand of nineteen cards
 * fans into six inches of phone, and — the big one — **which menu entries a card is even
 * allowed to offer**. That last one is a matrix (six zones × mine/theirs × two formats) that
 * would otherwise be a thicket of `v-if`s nobody could read or test; as a list of descriptors
 * it is a table you can assert against.
 *
 * Nothing here sends an action. `menuActionsFor` says *what may be offered*; the component
 * maps the chosen id onto a `PlayAction`. The engine (the server) validates everything again
 * anyway — this is about not offering a player a button that can only fail.
 */

/** A pointer position resolved to the battlefield's own 0..1 coordinate space. */
export interface PlayPoint {
  x: number
  y: number
}

/** The slice of a `DOMRect` the coordinate maths needs (so tests need no layout). */
export interface PlayRect {
  left: number
  top: number
  width: number
  height: number
}

/** Clamp to the 0..1 fraction the wire contract stores positions in. */
export function clampFraction(value: number): number {
  if (!Number.isFinite(value)) return 0
  if (value < 0) return 0
  if (value > 1) return 1
  return value
}

/**
 * Where a pointer sits inside a battlefield, as the 0..1 fractions `set_position` carries.
 *
 * Fractions, not pixels, because the same board is read by everyone: an opponent renders my
 * battlefield at a third of the size in their strip, and a phone renders it at a quarter of
 * the desktop width. A zero-area rect (a board that hasn't laid out yet) answers the centre
 * rather than dividing by zero.
 */
export function pointToFraction(rect: PlayRect, clientX: number, clientY: number): PlayPoint {
  if (rect.width <= 0 || rect.height <= 0) return { x: 0.5, y: 0.5 }
  return {
    x: clampFraction((clientX - rect.left) / rect.width),
    y: clampFraction((clientY - rect.top) / rect.height),
  }
}

/**
 * How far a pointer must travel before a press counts as a drag rather than a click.
 *
 * A card's click (tap/untap) and its drag (move) start with the same gesture, and a finger
 * never presses perfectly still — without a threshold every tap on a touchscreen would also
 * nudge the card a pixel and write a `set_position`.
 */
export const PLAY_DRAG_THRESHOLD = 6

/** Whether a pointer has moved far enough from its origin to be a drag. */
export function exceedsDragThreshold(dx: number, dy: number, threshold = PLAY_DRAG_THRESHOLD) {
  return Math.hypot(dx, dy) >= threshold
}

/** Where a card lands when it is played without a position (double-click from hand). */
export const PLAY_DEFAULT_DROP: PlayPoint = { x: 0.5, y: 0.5 }

/** How the hand lays its cards out at a given width. */
export interface PlayFan {
  /** Distance between adjacent card left edges. */
  step: number
  /** Total width the laid-out hand occupies. */
  width: number
  /** Whether cards had to overlap to fit (they fan rather than sit in a row). */
  overlapped: boolean
}

/** Cards never slide further under each other than this — the name must stay readable. */
const MIN_FAN_STEP_RATIO = 0.22

/**
 * Lay a hand out in `width` pixels.
 *
 * Up to the point where they fit, cards sit side by side with a small gap. Past it they
 * overlap by exactly as much as is needed, down to a floor of ~22% of a card's width — below
 * that a fan stops being a hand and becomes a stack, so the row scrolls instead.
 */
export function fanLayout(count: number, width: number, cardWidth: number): PlayFan {
  if (count <= 0 || cardWidth <= 0) return { step: 0, width: 0, overlapped: false }
  const gap = Math.round(cardWidth * 0.08)
  const loose = cardWidth + gap
  if (count === 1) return { step: loose, width: cardWidth, overlapped: false }
  if (loose * (count - 1) + cardWidth <= width) {
    return { step: loose, width: loose * (count - 1) + cardWidth, overlapped: false }
  }
  const needed = (width - cardWidth) / (count - 1)
  const step = Math.max(cardWidth * MIN_FAN_STEP_RATIO, needed)
  return { step, width: step * (count - 1) + cardWidth, overlapped: true }
}

/** What a zone is called in a label, a log line or a menu entry. */
export function zoneLabel(zone: PlayZone): string {
  switch (zone) {
    case 'library':
      return 'Library'
    case 'hand':
      return 'Hand'
    case 'battlefield':
      return 'Battlefield'
    case 'graveyard':
      return 'Graveyard'
    case 'exile':
      return 'Exile'
    case 'command':
      return 'Command zone'
  }
}

/**
 * A seat's colour, by its index at the table.
 *
 * Deliberately the life counter's `seatColor` rather than a second list: the chart palette is
 * CVD-validated **in token order** (docs/design-system.md), so a play table that picked its own
 * order would quietly re-create the adjacent-hue clashes that validation ruled out. One seam,
 * one order.
 */
export function seatColor(index: number): string {
  return chartSeatColor(index)
}

/** One card on a battlefield, with its attachments resolved into a paint order. */
export interface PlayBattlefieldItem {
  card: PlayCardView
  /** 0..1 fractions, already offset for an attachment. */
  x: number
  y: number
  /** Paint order: higher paints later (on top). */
  z: number
  /** How deep in an attachment chain this card is (0 = a free permanent). */
  depth: number
}

/** How far an attached card peeks out from under the card it is attached to. */
const ATTACH_OFFSET_X = 0.012
const ATTACH_OFFSET_Y = 0.03
/** A chain deeper than this is almost certainly a cycle; stop following it. */
const MAX_ATTACH_DEPTH = 6

/**
 * Resolve a battlefield's cards into positions and a paint order.
 *
 * An attached card (an aura, an equipment, a "goes under this" token) takes its **host's**
 * position, offset by a sliver, and paints *below* it — which is how the same thing is shown
 * on a real table: the enchanted creature is on top and readable, and the count of things
 * stuck to it is legible from the fanned edges. Attachments are followed transitively and
 * defensively: the server has no cycle check (nothing here is rules-enforced), so a chain that
 * loops back on itself is cut at {@link MAX_ATTACH_DEPTH} rather than hanging the renderer.
 */
export function battlefieldLayout(cards: PlayCardView[]): PlayBattlefieldItem[] {
  const byId = new Map(cards.map((card) => [card.id, card]))
  const items = cards.map((card) => {
    let x = card.x
    let y = card.y
    let depth = 0
    let host = card.attached_to === null ? undefined : byId.get(card.attached_to)
    const seen = new Set<number>([card.id])
    while (host && depth < MAX_ATTACH_DEPTH && !seen.has(host.id)) {
      seen.add(host.id)
      depth += 1
      x = host.x + ATTACH_OFFSET_X * depth
      y = host.y + ATTACH_OFFSET_Y * depth
      host = host.attached_to === null ? undefined : byId.get(host.attached_to)
    }
    return {
      card,
      x: clampFraction(x),
      y: clampFraction(y),
      z: MAX_ATTACH_DEPTH + 1 - depth,
      depth,
    }
  })
  // Deepest first so a host always paints over what is attached to it.
  return items.sort((a, b) => a.z - b.z)
}

/** Every menu entry the table can offer. The component maps these onto `PlayAction`s. */
export type PlayMenuActionId =
  | 'tap'
  | 'untap'
  | 'transform'
  | 'face_down'
  | 'face_up'
  | 'plus_one'
  | 'minus_one'
  | 'loyalty_up'
  | 'loyalty_down'
  | 'counter_custom'
  | 'reveal'
  | 'unreveal'
  | 'attach'
  | 'detach'
  | 'clone'
  | 'take_control'
  | 'play'
  | 'play_face_down'
  | 'to_hand'
  | 'to_library_top'
  | 'to_library_bottom'
  | 'to_graveyard'
  | 'to_exile'
  | 'to_command'
  | 'view'

/** A menu entry: what it says, and which run of the menu it belongs to. */
export interface PlayMenuAction {
  id: PlayMenuActionId
  label: string
  /** Entries are rendered in group order with a separator between groups. */
  group: 'state' | 'counters' | 'move' | 'view'
  /** A move that can't be undone by moving back (a token ceasing to exist). */
  destructive?: boolean
}

/** Whether this card's current face has a loyalty number worth stepping. */
function hasLoyalty(card: PlayCardView): boolean {
  const face = card.def?.faces[card.face_index] ?? card.def?.faces[0]
  return Boolean(face?.loyalty)
}

/**
 * The entries a card's context menu may offer, given where it is and whose it is.
 *
 * Three rules shape the list, and they are the reason this is a function and not markup:
 *
 * 1. **Zone decides.** "Tap" is meaningless in a graveyard; "reveal" only means something in a
 *    hand; "play" means *to the battlefield* from anywhere else. A card in the command zone is
 *    "cast", because that is the word at the table.
 * 2. **Ownership decides.** Someone else's permanent offers exactly one thing — take control of
 *    it — plus a look at it. Everything else would be a button the engine answers `NotYourCard`
 *    to. Their hidden zones offer nothing at all.
 * 3. **Format decides one thing.** Outside Commander the command zone exists but is never used,
 *    so it is not offered as a destination — an empty zone on the rail is fine, a "move to
 *    nowhere" menu entry is not.
 *
 * A token is called out where it matters: moving one off the battlefield destroys it, so those
 * entries are marked destructive rather than silently eating the card.
 */
export function menuActionsFor(
  card: PlayCardView,
  zone: PlayZone,
  isMine: boolean,
  format: string,
): PlayMenuAction[] {
  const out: PlayMenuAction[] = []
  const commander = format === 'commander'
  const token = card.def?.is_token ?? false

  if (!isMine) {
    // Someone else's card: the only legal reach across the table is taking control of a
    // permanent. Their hidden zones aren't even visible, so there is nothing to look at.
    if (zone === 'battlefield') {
      out.push({ id: 'take_control', label: 'Take control', group: 'state' })
      out.push({ id: 'view', label: 'View larger', group: 'view' })
    } else if (zone === 'graveyard' || zone === 'exile' || zone === 'command') {
      out.push({ id: 'view', label: 'View larger', group: 'view' })
    }
    return out
  }

  if (zone === 'battlefield') {
    out.push(
      card.tapped
        ? { id: 'untap', label: 'Untap', group: 'state' }
        : { id: 'tap', label: 'Tap', group: 'state' },
    )
    out.push(
      card.face_down
        ? { id: 'face_up', label: 'Turn face up', group: 'state' }
        : { id: 'face_down', label: 'Turn face down', group: 'state' },
    )
  }
  if (zone === 'hand') {
    out.push({ id: 'play', label: 'Play', group: 'state' })
    out.push({ id: 'play_face_down', label: 'Play face down', group: 'state' })
    out.push(
      card.revealed
        ? { id: 'unreveal', label: 'Stop revealing', group: 'state' }
        : { id: 'reveal', label: 'Reveal', group: 'state' },
    )
  }
  if (zone === 'graveyard' || zone === 'exile' || zone === 'command') {
    out.push({ id: 'play', label: zone === 'command' ? 'Cast' : 'Play', group: 'state' })
  }
  if ((card.def?.faces.length ?? 0) > 1) {
    out.push({ id: 'transform', label: 'Transform', group: 'state' })
  }

  if (zone === 'battlefield') {
    out.push({ id: 'plus_one', label: 'Add +1/+1', group: 'counters' })
    out.push({ id: 'minus_one', label: 'Add −1/−1', group: 'counters' })
    if (hasLoyalty(card)) {
      out.push({ id: 'loyalty_up', label: 'Loyalty +1', group: 'counters' })
      out.push({ id: 'loyalty_down', label: 'Loyalty −1', group: 'counters' })
    }
    out.push({ id: 'counter_custom', label: 'Add a counter…', group: 'counters' })
    if (card.attached_to === null) {
      out.push({ id: 'attach', label: 'Attach to…', group: 'counters' })
    } else {
      out.push({ id: 'detach', label: 'Detach', group: 'counters' })
    }
    out.push({ id: 'clone', label: 'Make a token copy', group: 'counters' })
  }

  const moves: { id: PlayMenuActionId; label: string; from: PlayZone[] }[] = [
    { id: 'to_hand', label: 'To hand', from: ['battlefield', 'graveyard', 'exile', 'command'] },
    {
      id: 'to_library_top',
      label: 'To top of library',
      from: ['battlefield', 'hand', 'graveyard', 'exile', 'command'],
    },
    {
      id: 'to_library_bottom',
      label: 'To bottom of library',
      from: ['battlefield', 'hand', 'graveyard', 'exile', 'command'],
    },
    {
      id: 'to_graveyard',
      label: 'To graveyard',
      from: ['battlefield', 'hand', 'exile', 'command'],
    },
    { id: 'to_exile', label: 'To exile', from: ['battlefield', 'hand', 'graveyard', 'command'] },
    {
      id: 'to_command',
      label: 'To command zone',
      from: ['battlefield', 'hand', 'graveyard', 'exile'],
    },
  ]
  for (const move of moves) {
    if (!move.from.includes(zone)) continue
    if (move.id === 'to_command' && !commander) continue
    // A token that leaves the battlefield stops existing — say so before it happens.
    out.push({ id: move.id, label: move.label, group: 'move', destructive: token })
  }

  out.push({ id: 'view', label: 'View larger', group: 'view' })
  return out
}

/** The menu's groups, in the order they render. */
export const PLAY_MENU_GROUPS = ['state', 'counters', 'move', 'view'] as const

/** The counters a card carries, as chips, in a stable order. */
export function counterChips(card: PlayCardView): { name: string; value: number }[] {
  return Object.entries(card.counters)
    .filter(([, value]) => value !== 0)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([name, value]) => ({ name, value }))
}

/** How many counters of every kind a card carries (for the accessible label). */
export function counterTotal(card: PlayCardView): number {
  return counterChips(card).reduce((sum, chip) => sum + Math.abs(chip.value), 0)
}

/** What a card is called when its identity is hidden from this viewer. */
export const PLAY_HIDDEN_CARD_NAME = 'Face-down card'

/** The name to show for a card — a hidden one has none to give. */
export function cardName(card: PlayCardView): string {
  if (!card.def) return PLAY_HIDDEN_CARD_NAME
  const face = card.def.faces[card.face_index] ?? card.def.faces[0]
  return face?.name ?? card.def.name
}

/**
 * What a screen reader hears for one card: name first, then the state that matters at a
 * glance. Never colour or position alone — "the tapped one on the left" is not a label.
 */
export function cardAriaLabel(card: PlayCardView): string {
  const parts = [card.face_down && !card.def ? PLAY_HIDDEN_CARD_NAME : cardName(card)]
  if (card.face_down) parts.push('face down')
  if (card.tapped) parts.push('tapped')
  const counters = counterChips(card)
  if (counters.length === 1 && counters[0]) {
    const chip = counters[0]
    parts.push(`${chip.value} ${chip.name} ${Math.abs(chip.value) === 1 ? 'counter' : 'counters'}`)
  } else if (counters.length > 1) {
    parts.push(counters.map((chip) => `${chip.value} ${chip.name}`).join(', '))
  }
  if (card.attached_to !== null) parts.push('attached')
  return parts.join(', ')
}

/**
 * One line of the log, as it reads.
 *
 * The server writes the whole narration for an action ("Ana drew a card" — it knows the seat
 * name at the moment it happened, and it is the one place hidden information is phrased
 * safely), so an action line is shown as-is. A chat line arrives as bare speech and is
 * punctuated with the speaker here ("Ana: nice draw") — the two must be tellable apart at a
 * glance in a scrolling column, which is also why they are coloured and weighted
 * differently in `PlayLog`.
 */
export function logLine(entry: PlayLogEntry, seats: PlaySeatSnapshot[]): string {
  if (entry.seat === null || entry.kind === 'system') return entry.text
  const seat = seats.find((s) => s.id === entry.seat)
  if (!seat) return entry.text
  return entry.kind === 'chat' ? `${seat.name}: ${entry.text}` : entry.text
}

/** The dice a table can roll, plus the coin (which is its own action). */
export const PLAY_DICE = [4, 6, 8, 10, 12, 20] as const

/** The phases, in turn order, with the short label the bar shows. */
export const PLAY_PHASES = [
  { phase: 'untap', label: 'Untap' },
  { phase: 'upkeep', label: 'Upkeep' },
  { phase: 'draw', label: 'Draw' },
  { phase: 'main1', label: 'Main 1' },
  { phase: 'combat', label: 'Combat' },
  { phase: 'main2', label: 'Main 2' },
  { phase: 'end', label: 'End' },
] as const

/** Bounds the table's own prompts honour, so a prompt can't ask for something 422-able. */
export const PLAY_LOOK_TOP_MAX = 10
export const PLAY_TOKEN_COUNT_MAX = 20
/**
 * The engine's `MAX_COUNTER_NAME`. A longer name is not truncated server-side — the whole
 * `counter` action is refused — so the prompt clamps rather than letting a wordy counter
 * ("commander tax this turn") come back as an error the player can't connect to what they
 * typed.
 */
export const PLAY_COUNTER_NAME_MAX = 24

/**
 * How long a touch/pen press has to sit still before it is a long press rather than a tap.
 *
 * Mirrors reka-ui's `ContextMenuRoot` `pressOpenDelay` default: past this the card's context
 * menu has opened under the finger, and the press that opened it must not *also* be read as a
 * tap when it lifts.
 */
export const PLAY_LONG_PRESS_MS = 700

/** Clamp an integer prompt answer into range, or null when it wasn't a number at all. */
export function parseCount(raw: string | null, min: number, max: number): number | null {
  if (raw === null) return null
  const value = Number.parseInt(raw.trim(), 10)
  if (!Number.isFinite(value)) return null
  return Math.min(max, Math.max(min, value))
}
