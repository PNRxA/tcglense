import {
  computed,
  inject,
  onScopeDispose,
  provide,
  ref,
  shallowRef,
  watch,
  type ComputedRef,
  type InjectionKey,
  type Ref,
} from 'vue'
import { usePlayRoomStore } from '@/stores/playRoom'
import { useWakeLock, type WakeLock } from '@/composables/useWakeLock'
import {
  PLAY_COUNTER_NAME_MAX,
  PLAY_DEFAULT_DROP,
  PLAY_LONG_PRESS_MS,
  PLAY_LOOK_TOP_MAX,
  exceedsDragThreshold,
  menuActionsFor,
  parseCount,
  pointToFraction,
  type PlayMenuActionId,
  type PlayPoint,
} from '@/lib/playTable'
import type { PlayAction, PlayCardView, PlayPlacement, PlayZone } from '@/lib/api/play'

/**
 * The table's engine: everything about *using* the table that isn't markup.
 *
 * The table is one shared machine — a drag that starts in the hand ends on the battlefield, a
 * keyboard shortcut acts on whatever the pointer is over, a peek opens the same viewer a
 * graveyard click does — so the state behind it is created once by `PlayTable` and handed down
 * through provide/inject rather than threaded as props through eight components. There is
 * exactly one instance per table.
 *
 * Five things here are worth reading before changing them:
 *
 * 1. **A press is not yet a gesture.** Every card is both a button (click = tap) and a draggable
 *    object, and the two can only be told apart by what the pointer does next. So a pointerdown
 *    records an origin and captures the pointer, a pointermove past
 *    {@link PLAY_DRAG_THRESHOLD} promotes it to a drag, and a pointerup decides: no promotion
 *    means the press was a click. Without the threshold every tap on a touchscreen would also
 *    write a position. A press that opened the card's context menu (a long press on touch) is
 *    spent: it is that gesture, and not also a tap.
 * 2. **Drop targets are discovered, not registered.** A drop is resolved by asking the document
 *    what is under the pointer (`elementFromPoint` → the nearest `[data-play-drop]`), which
 *    means a new drop target is one attribute on a div and nothing here changes. The drag ghost
 *    must therefore stay `pointer-events-none`, or every drop would land on the ghost.
 * 3. **Positions are streamed, then committed.** Dragging a permanent sends `set_position` at
 *    most every {@link POSITION_THROTTLE_MS} so the table stays live for everyone else without
 *    flooding the socket, and always sends a final one on drop — the throttled stream is a
 *    nicety, the commit is the truth.
 * 4. **Shortcuts belong to the table, not to whatever is on top of it.** A single letter acts
 *    on the card under the pointer, so the keyboard stands down entirely while a dialog, sheet
 *    or menu is open (see {@link overlayOpen}) and ignores auto-repeat — the two ways a
 *    shortcut turns into something nobody asked for.
 * 5. **A finished game is a record, not a table.** Every verb that would change the game is
 *    gated on `canAct` here as well as disabled in the UI, so a keyboard shortcut, a stale
 *    click and a drag already in flight all stop at the same line. Chat, the log and looking
 *    through a pile keep working — that is what people do immediately after a game ends.
 */

/** How often a position update goes out while a card is still under the finger. */
const POSITION_THROTTLE_MS = 80

/** A press in progress, before or after it became a drag. */
export interface PlayDragState {
  cardId: number
  zone: PlayZone
  /** True once the pointer travelled past the drag threshold. */
  moved: boolean
  /** Where the press began, in viewport coordinates — the threshold measures from here. */
  originX: number
  originY: number
  /** Current pointer position, in viewport coordinates. */
  x: number
  y: number
  /** Where inside the card the pointer grabbed it, so the ghost doesn't jump. */
  grabX: number
  grabY: number
  /** The dragged card's on-screen size, so the ghost matches it. */
  width: number
  height: number
  /** `mouse` / `touch` / `pen` — only the latter two can become a long press. */
  pointerType: string
  /** When the press began (`Date.now()`), so its duration can be measured on the way up. */
  startedAt: number
}

/** What the zone viewer is currently showing. */
export type PlayViewerState =
  | { kind: 'zone'; seatId: number; zone: PlayZone }
  /** A private `look_top` / `search_library` answer — the cards live on `store.peek`. */
  | { kind: 'peek' }

/** A card pulled up large, either by hover/focus or pinned by "view larger". */
export interface PlayPreviewState {
  card: PlayCardView
  /** Where on screen the card sits, so the preview can be placed beside it. */
  anchor: { left: number; top: number; width: number; height: number }
  /** A pinned preview stays until dismissed; a hover preview follows the pointer away. */
  pinned: boolean
}

export interface PlayTableApi {
  store: ReturnType<typeof usePlayRoomStore>
  /** The card the pointer or keyboard focus is on — what a bare `T`/`F` acts upon. */
  hoveredId: Ref<number | null>
  selectedId: Ref<number | null>
  drag: Ref<PlayDragState | null>
  preview: Ref<PlayPreviewState | null>
  viewer: Ref<PlayViewerState | null>
  tokenOpen: Ref<boolean>
  /** Whether the game is still on. False once it is finished: every verb below stands down. */
  canAct: ComputedRef<boolean>
  /** The battlefield card an "attach to…" is being picked for. */
  attachFor: Ref<number | null>
  logOpen: Ref<boolean>
  wakeLock: WakeLock
  /** Register my battlefield element — the coordinate space every drop resolves against. */
  setBattlefield: (el: HTMLElement | null) => void
  startCardDrag: (event: PointerEvent, card: PlayCardView, zone: PlayZone) => void
  hoverCard: (card: PlayCardView | null, el: HTMLElement | null) => void
  pinPreview: (card: PlayCardView, el: HTMLElement | null) => void
  closePreview: () => void
  openViewer: (state: PlayViewerState) => void
  closeViewer: () => void
  send: (action: PlayAction) => void
  tapCard: (card: PlayCardView) => void
  playCard: (card: PlayCardView, at?: PlayPoint) => void
  moveCard: (card: PlayCardView, zone: PlayZone, opts?: MoveOptions) => void
  moveLibraryCard: (cardId: number, zone: PlayZone, opts?: MoveOptions) => void
  runMenuAction: (id: PlayMenuActionId, card: PlayCardView) => void
  menuFor: (card: PlayCardView, zone: PlayZone) => ReturnType<typeof menuActionsFor>
  draw: (n: number) => void
  untapAll: () => void
  shuffle: () => void
  mulligan: () => void
  lookTop: () => void
  searchLibrary: () => void
  passTurn: () => void
  roll: (sides: number) => void
  flipCoin: () => void
  chat: (text: string) => void
  /** Whether a card belongs to the viewer's own seat (so it may be acted on). */
  isMine: (card: PlayCardView) => boolean
}

export interface MoveOptions {
  placement?: PlayPlacement | null
  x?: number | null
  y?: number | null
  faceDown?: boolean | null
}

const PLAY_TABLE_KEY: InjectionKey<PlayTableApi> = Symbol('play-table')

/** Read the table engine `PlayTable` provided. Throws rather than silently doing nothing. */
export function usePlayTableContext(): PlayTableApi {
  const api = inject(PLAY_TABLE_KEY, null)
  if (!api) throw new Error('usePlayTableContext() outside of <PlayTable>')
  return api
}

/**
 * True for an element that Space/Enter already means something to — a button, a link, a menu
 * entry. Those two keys pass the turn, and a player whose focus is on "Draw" pressing Enter
 * means *draw*, not *draw and then hand the turn over*. The letter shortcuts have no such
 * clash, so they are not gated on this — a card is a button, and `T` on a focused card has to
 * keep working.
 */
function isActivatableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  return target.closest('a[href],button,summary,[role="menuitem"],[role="menuitemradio"]') !== null
}

/** True for a keystroke aimed at a text field — chat must never eat a shortcut key. */
function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  if (target.isContentEditable) return true
  const tag = target.tagName
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT'
}

/**
 * True while something modal owns the screen: a dialog (the zone viewer, the token and attach
 * dialogs, the concede confirm), the log sheet, or an open card/table menu.
 *
 * Single-letter shortcuts act on whatever the pointer last touched, which is exactly the wrong
 * thing to do while a player is reading their library or picking a token: moving the pointer
 * across an open dialog leaves `hoveredId` on a card behind it, so `S` would shuffle the very
 * library they are in the middle of searching. Keys that belong to an overlay (Escape, arrows,
 * Enter on a menu item) are the overlay's own, and reka handles them.
 *
 * The preview is the one exception, and carries `data-play-preview` to say so: it is a picture
 * that follows the pointer around the table rather than something that takes the screen, and
 * gating shortcuts on a hover would turn every shortcut off whenever a card is under the
 * pointer — which is to say, always.
 */
function overlayOpen(): boolean {
  if (typeof document === 'undefined') return false
  return (
    document.querySelector(
      '[role="dialog"]:not([data-play-preview]),[role="alertdialog"],[role="menu"]',
    ) !== null
  )
}

/** Create the table engine and provide it to the subtree. Called once, by `PlayTable`. */
export function usePlayTable(): PlayTableApi {
  const store = usePlayRoomStore()

  const hoveredId = ref<number | null>(null)
  const selectedId = ref<number | null>(null)
  const drag = shallowRef<PlayDragState | null>(null)
  const preview = shallowRef<PlayPreviewState | null>(null)
  const viewer = ref<PlayViewerState | null>(null)
  const tokenOpen = ref(false)
  const attachFor = ref<number | null>(null)
  const logOpen = ref(true)

  // A screen that dims mid-game is a real problem at a table where a turn can take minutes
  // without a touch; the lock is dropped the moment the game is over.
  const wakeLock = useWakeLock(() => store.status === 'playing')

  /**
   * The one gate on every verb. A room is only playable while it says `playing` — a lobby has
   * no table yet and a finished one is a result, which people keep looking at and talking
   * about long after the last action.
   */
  const canAct = computed(() => store.status === 'playing')

  let battlefield: HTMLElement | null = null
  function setBattlefield(el: HTMLElement | null) {
    battlefield = el
  }

  function send(action: PlayAction) {
    store.send(action)
  }

  function isMine(card: PlayCardView): boolean {
    return store.mySeatId !== null && card.controller === store.mySeatId
  }

  // ---- moves -------------------------------------------------------------------------

  function moveCard(card: PlayCardView, zone: PlayZone, opts: MoveOptions = {}) {
    if (!canAct.value) return
    send({
      type: 'move_card',
      card: card.id,
      zone,
      placement: opts.placement ?? null,
      x: opts.x ?? null,
      y: opts.y ?? null,
      face_down: opts.faceDown ?? null,
    })
  }

  /** The same move for a card the viewer only knows about through a peek. */
  function moveLibraryCard(cardId: number, zone: PlayZone, opts: MoveOptions = {}) {
    if (!canAct.value) return
    send({
      type: 'move_library_card',
      card: cardId,
      zone,
      placement: opts.placement ?? null,
      x: opts.x ?? null,
      y: opts.y ?? null,
      face_down: opts.faceDown ?? null,
    })
  }

  function playCard(card: PlayCardView, at: PlayPoint = PLAY_DEFAULT_DROP, faceDown = false) {
    moveCard(card, 'battlefield', { x: at.x, y: at.y, faceDown: faceDown ? true : null })
  }

  function tapCard(card: PlayCardView) {
    if (!canAct.value || !isMine(card)) return
    send({ type: 'tap', card: card.id, tapped: !card.tapped })
  }

  // ---- pointer gestures --------------------------------------------------------------

  let lastPositionAt = 0
  /** The card a context menu opened on while its press was still in flight. */
  let menuCardId: number | null = null

  /** What is under this point that accepts a drop, if anything. */
  function dropTargetAt(x: number, y: number): { zone: PlayZone; seatId: number | null } | null {
    if (typeof document === 'undefined' || !document.elementFromPoint) return null
    // jsdom (and a document mid-teardown) can throw here; a drag that can't find a target
    // is a drop on nothing, which is already a case this handles.
    let el: Element | null = null
    try {
      el = document.elementFromPoint(x, y)
    } catch {
      return null
    }
    const target = el instanceof Element ? el.closest('[data-play-drop]') : null
    if (!(target instanceof HTMLElement)) return null
    const zone = target.dataset.playDrop as PlayZone | undefined
    if (!zone) return null
    const seat = target.dataset.playSeat
    return { zone, seatId: seat ? Number(seat) : null }
  }

  function battlefieldPoint(x: number, y: number): PlayPoint {
    if (!battlefield) return PLAY_DEFAULT_DROP
    return pointToFraction(battlefield.getBoundingClientRect(), x, y)
  }

  function onPointerMove(event: PointerEvent) {
    const state = drag.value
    if (!state) return
    const moved =
      state.moved ||
      exceedsDragThreshold(event.clientX - state.originX, event.clientY - state.originY)
    const next: PlayDragState = { ...state, x: event.clientX, y: event.clientY, moved }
    drag.value = next
    if (!next.moved) return
    // Hide any hover preview the moment a drag starts — a large image under the finger is
    // exactly what you don't want while placing a card.
    if (preview.value && !preview.value.pinned) preview.value = null
    if (state.zone !== 'battlefield') return
    const now = Date.now()
    if (now - lastPositionAt < POSITION_THROTTLE_MS) return
    lastPositionAt = now
    const point = battlefieldPoint(event.clientX, event.clientY)
    send({ type: 'set_position', card: state.cardId, x: point.x, y: point.y })
  }

  /**
   * Whether this press has already been spent opening the card's context menu.
   *
   * A long press on touch is *one* gesture that produces two events: the menu opens under the
   * finger (a `contextmenu` on Android, reka's own `pressOpenDelay` timer everywhere else) and
   * then the finger lifts, which the drag machine would otherwise read as a press that never
   * travelled — i.e. a tap. Tapping a permanent taps it, so a long press used to open the menu
   * *and* tap the card behind it, with the menu's own "Tap" entry then untapping it.
   */
  function pressWasLongPress(state: PlayDragState): boolean {
    if (menuCardId === state.cardId) return true
    const touchish = state.pointerType === 'touch' || state.pointerType === 'pen'
    return touchish && Date.now() - state.startedAt >= PLAY_LONG_PRESS_MS
  }

  function finishDrag(event: PointerEvent) {
    const state = drag.value
    drag.value = null
    detachPointer()
    if (!state) return
    const card = store.card(state.cardId)
    if (!card) return
    if (!state.moved) {
      // A press that never travelled: a click, unless it was long enough to be the gesture
      // that opened the menu — that one is already spent.
      if (pressWasLongPress(state)) return
      // On the battlefield a click is tap/untap; anywhere else it is just a selection (the
      // menu and double-click carry the verbs there).
      selectedId.value = card.id
      if (state.zone === 'battlefield' && isMine(card)) tapCard(card)
      return
    }
    const target = dropTargetAt(event.clientX, event.clientY)
    if (!target) {
      // Dropped on nothing. A battlefield card keeps wherever the stream last put it; a card
      // from elsewhere goes back where it was, which needs no action at all.
      if (state.zone === 'battlefield' && isMine(card)) {
        const point = battlefieldPoint(event.clientX, event.clientY)
        send({ type: 'set_position', card: card.id, x: point.x, y: point.y })
      }
      return
    }
    if (target.zone === 'battlefield') {
      const point = battlefieldPoint(event.clientX, event.clientY)
      if (state.zone === 'battlefield' && isMine(card)) {
        send({ type: 'set_position', card: card.id, x: point.x, y: point.y })
      } else if (isMine(card)) {
        moveCard(card, 'battlefield', { x: point.x, y: point.y })
      } else {
        // Someone else's permanent dragged onto my board: that's taking control of it.
        send({ type: 'take_control', card: card.id, x: point.x, y: point.y })
      }
      return
    }
    if (!isMine(card)) return
    if (target.zone === state.zone) return
    moveCard(card, target.zone, { placement: 'top' })
  }

  function onPointerCancel() {
    drag.value = null
    detachPointer()
  }

  function onContextMenu() {
    const state = drag.value
    if (state) menuCardId = state.cardId
  }

  function attachPointer() {
    window.addEventListener('pointermove', onPointerMove)
    window.addEventListener('pointerup', finishDrag)
    window.addEventListener('pointercancel', onPointerCancel)
    // Capture: the trigger calls `preventDefault()` on its own handler, and a listener that
    // waited for the bubble phase would still see it — but capture keeps this independent of
    // whether anything downstream stops propagation.
    window.addEventListener('contextmenu', onContextMenu, true)
  }

  function detachPointer() {
    window.removeEventListener('pointermove', onPointerMove)
    window.removeEventListener('pointerup', finishDrag)
    window.removeEventListener('pointercancel', onPointerCancel)
    window.removeEventListener('contextmenu', onContextMenu, true)
  }

  function startCardDrag(event: PointerEvent, card: PlayCardView, zone: PlayZone) {
    // Left button / touch / pen only: the right button belongs to the context menu.
    if (event.button !== 0 || !canAct.value) return
    const el = event.currentTarget
    const rect = el instanceof HTMLElement ? el.getBoundingClientRect() : null
    if (el instanceof HTMLElement && el.setPointerCapture) {
      // Capture keeps the gesture alive when the pointer leaves the card (which it does
      // immediately — that is the whole point of a drag).
      try {
        el.setPointerCapture(event.pointerId)
      } catch {
        // Some browsers refuse capture for a pointer that is already gone; the window
        // listeners below still see the rest of the gesture.
      }
    }
    hoveredId.value = card.id
    menuCardId = null
    drag.value = {
      cardId: card.id,
      zone,
      moved: false,
      pointerType: event.pointerType || 'mouse',
      startedAt: Date.now(),
      originX: event.clientX,
      originY: event.clientY,
      x: event.clientX,
      y: event.clientY,
      grabX: rect ? event.clientX - rect.left : 0,
      grabY: rect ? event.clientY - rect.top : 0,
      width: rect?.width ?? 90,
      height: rect?.height ?? 125,
    }
    attachPointer()
  }

  // ---- preview -----------------------------------------------------------------------

  function anchorOf(el: HTMLElement | null) {
    const rect = el?.getBoundingClientRect()
    return {
      left: rect?.left ?? 0,
      top: rect?.top ?? 0,
      width: rect?.width ?? 0,
      height: rect?.height ?? 0,
    }
  }

  function hoverCard(card: PlayCardView | null, el: HTMLElement | null) {
    hoveredId.value = card?.id ?? null
    if (preview.value?.pinned) return
    if (!card || drag.value) {
      preview.value = null
      return
    }
    preview.value = { card, anchor: anchorOf(el), pinned: false }
  }

  function pinPreview(card: PlayCardView, el: HTMLElement | null) {
    preview.value = { card, anchor: anchorOf(el), pinned: true }
  }

  function closePreview() {
    preview.value = null
  }

  // ---- zone viewer -------------------------------------------------------------------

  function openViewer(state: PlayViewerState) {
    viewer.value = state
  }

  function closeViewer() {
    viewer.value = null
    // A peek is a one-shot private answer; closing the viewer is what consumes it.
    if (store.peek) store.clearPeek()
  }

  // A peek arrives asynchronously (the server answers `look_top` / `search_library` privately),
  // so the viewer opens itself when one lands rather than the caller guessing at a delay.
  watch(
    () => store.peek,
    (peek) => {
      if (peek) viewer.value = { kind: 'peek' }
    },
  )

  // ---- table verbs -------------------------------------------------------------------

  function draw(n: number) {
    if (!canAct.value) return
    send({ type: 'draw', n })
  }

  function untapAll() {
    if (!canAct.value) return
    send({ type: 'untap_all' })
  }

  function shuffle() {
    if (!canAct.value) return
    send({ type: 'shuffle' })
  }

  function mulligan() {
    if (!canAct.value) return
    const size = parseCount(window.prompt('Mulligan to how many cards?', '7'), 0, 20)
    if (size === null) return
    send({ type: 'mulligan', hand_size: size })
  }

  function lookTop() {
    if (!canAct.value) return
    const n = parseCount(window.prompt('Look at how many cards?', '1'), 1, PLAY_LOOK_TOP_MAX)
    if (n === null) return
    send({ type: 'look_top', n })
  }

  function searchLibrary() {
    if (!canAct.value) return
    send({ type: 'search_library' })
  }

  function passTurn() {
    if (!canAct.value || !store.isMyTurn) return
    send({ type: 'pass_turn' })
  }

  function roll(sides: number) {
    if (!canAct.value) return
    send({ type: 'roll', sides })
  }

  function flipCoin() {
    if (!canAct.value) return
    send({ type: 'flip_coin' })
  }

  function chat(text: string) {
    const trimmed = text.trim()
    if (!trimmed) return
    send({ type: 'chat', text: trimmed.slice(0, 500) })
  }

  // ---- the menu ----------------------------------------------------------------------

  function menuFor(card: PlayCardView, zone: PlayZone) {
    const actions = menuActionsFor(card, zone, isMine(card), store.format)
    // Once the game is over the only thing left to do with a card is look at it.
    return canAct.value ? actions : actions.filter((action) => action.id === 'view')
  }

  function addCounter(card: PlayCardView, name: string, delta: number) {
    send({ type: 'counter', card: card.id, name, delta })
  }

  /**
   * Run one menu entry. `menuActionsFor` decides what may be offered; this does it.
   *
   * There is deliberately no `zone` argument: the card carries its own (`card.zone`), and a
   * second source of truth for "where is this" is exactly the kind of thing that drifts one
   * refactor later into a move from a zone the card already left.
   */
  function runMenuAction(id: PlayMenuActionId, card: PlayCardView) {
    // "View larger" is not a game action, so it survives the end of the game.
    if (!canAct.value && id !== 'view') return
    switch (id) {
      case 'tap':
      case 'untap':
        send({ type: 'tap', card: card.id, tapped: id === 'tap' })
        return
      case 'transform':
        send({ type: 'toggle_face', card: card.id })
        return
      case 'face_down':
      case 'face_up':
        send({ type: 'set_face_down', card: card.id, face_down: id === 'face_down' })
        return
      case 'plus_one':
        addCounter(card, '+1/+1', 1)
        return
      case 'minus_one':
        addCounter(card, '-1/-1', 1)
        return
      case 'loyalty_up':
        addCounter(card, 'loyalty', 1)
        return
      case 'loyalty_down':
        addCounter(card, 'loyalty', -1)
        return
      case 'counter_custom': {
        const answer = window.prompt('Counter name', 'charge')?.trim()
        if (!answer) return
        // Past `PLAY_COUNTER_NAME_MAX` the engine refuses the action rather than truncating
        // it, so clamp here; `trimEnd` because a cut mid-phrase leaves a trailing space, and
        // "poison " is a second counter as far as the map on the card is concerned.
        const name = answer.slice(0, PLAY_COUNTER_NAME_MAX).trimEnd()
        if (!name) return
        addCounter(card, name, 1)
        return
      }
      case 'reveal':
      case 'unreveal':
        send({ type: 'reveal', card: card.id, revealed: id === 'reveal' })
        return
      case 'attach':
        attachFor.value = card.id
        return
      case 'detach':
        send({ type: 'attach', card: card.id, to: null })
        return
      case 'clone':
        send({ type: 'clone_card', card: card.id })
        return
      case 'take_control':
        send({ type: 'take_control', card: card.id, x: card.x, y: card.y })
        return
      case 'play':
        playCard(card)
        return
      case 'play_face_down':
        playCard(card, PLAY_DEFAULT_DROP, true)
        return
      case 'to_hand':
        moveCard(card, 'hand')
        return
      case 'to_library_top':
        moveCard(card, 'library', { placement: 'top' })
        return
      case 'to_library_bottom':
        moveCard(card, 'library', { placement: 'bottom' })
        return
      case 'to_graveyard':
        moveCard(card, 'graveyard', { placement: 'top' })
        return
      case 'to_exile':
        moveCard(card, 'exile', { placement: 'top' })
        return
      case 'to_command':
        moveCard(card, 'command')
        return
      case 'view':
        pinPreview(card, null)
        return
    }
  }

  // ---- keyboard ----------------------------------------------------------------------

  /** The card a bare shortcut acts on: what the pointer is over, else what was last clicked. */
  const focusCard = computed(() => {
    const id = hoveredId.value ?? selectedId.value
    return id === null ? null : (store.card(id) ?? null)
  })

  function onKeyDown(event: KeyboardEvent) {
    if (event.defaultPrevented || event.metaKey || event.ctrlKey || event.altKey) return
    // A held key repeats ~30×/s. Every verb here is a socket frame, so an `S` leant on for a
    // second is thirty shuffles, `too_fast`, and a 4008 close — and none of them is a verb
    // anyone means more than once.
    if (event.repeat) return
    // Chat is a text field on the same screen as every single-letter shortcut, so typing
    // must never be interpreted as a table command.
    if (isTypingTarget(event.target)) return
    // Escape dismisses the preview whatever else is up, and whether or not the game is still
    // on — a pinned card is readable long after the last action, and must stay closable.
    if (event.key === 'Escape') {
      if (preview.value) {
        event.preventDefault()
        closePreview()
      }
      return
    }
    if (overlayOpen()) return
    if (store.status !== 'playing') return
    const card = focusCard.value
    switch (event.key.toLowerCase()) {
      case 't':
        if (card && isMine(card) && card.zone === 'battlefield') {
          event.preventDefault()
          tapCard(card)
        }
        return
      case 'f':
        if (card && isMine(card) && card.zone === 'battlefield') {
          event.preventDefault()
          send({ type: 'set_face_down', card: card.id, face_down: !card.face_down })
        }
        return
      case 'd':
        event.preventDefault()
        draw(1)
        return
      case 'u':
        event.preventDefault()
        untapAll()
        return
      case 's':
        event.preventDefault()
        shuffle()
        return
      case ' ':
      case 'enter':
        if (!store.isMyTurn || isActivatableTarget(event.target)) return
        event.preventDefault()
        passTurn()
        return
    }
  }

  if (typeof window !== 'undefined') {
    window.addEventListener('keydown', onKeyDown)
    onScopeDispose(() => window.removeEventListener('keydown', onKeyDown))
  }
  onScopeDispose(detachPointer)

  const api: PlayTableApi = {
    store,
    hoveredId,
    selectedId,
    drag,
    preview,
    viewer,
    tokenOpen,
    canAct,
    attachFor,
    logOpen,
    wakeLock,
    setBattlefield,
    startCardDrag,
    hoverCard,
    pinPreview,
    closePreview,
    openViewer,
    closeViewer,
    send,
    tapCard,
    playCard,
    moveCard,
    moveLibraryCard,
    runMenuAction,
    menuFor,
    draw,
    untapAll,
    shuffle,
    mulligan,
    lookTop,
    searchLibrary,
    passTurn,
    roll,
    flipCoin,
    chat,
    isMine,
  }
  provide(PLAY_TABLE_KEY, api)
  return api
}
