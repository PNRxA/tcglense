import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent } from 'vue'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { usePlayTable, type PlayTableApi } from '@/composables/usePlayTable'
import { PLAY_COUNTER_NAME_MAX, PLAY_LONG_PRESS_MS } from '@/lib/playTable'
import { usePlayRoomStore } from '@/stores/playRoom'
import type { PlayCardView, PlaySeatSnapshot, PlaySnapshot } from '@/lib/api/play'

// The engine on its own, without the markup — because the two things worth pinning here are
// both about what the table does when something ELSE is in charge of the screen or the finger:
// a shortcut that fires while a dialog is open acts on a card nobody is looking at, and a long
// press that opens the card menu must not also be read as the tap that lifted off it.

const MINE = 1

function seat(over: Partial<PlaySeatSnapshot> = {}): PlaySeatSnapshot {
  return {
    id: MINE,
    seat_index: 0,
    name: 'Ana',
    is_host: true,
    deck_name: null,
    life: 40,
    counters: {},
    commander_damage: {},
    out: false,
    connected: true,
    library_count: 92,
    hand_count: 0,
    hand: [],
    battlefield: [111],
    graveyard: [],
    exile: [],
    command: [],
    ...over,
  }
}

function card(over: Partial<PlayCardView> = {}): PlayCardView {
  return {
    id: 111,
    def: null,
    owner: MINE,
    controller: MINE,
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
    ...over,
  }
}

function snapshot(over: Partial<PlaySnapshot> = {}): PlaySnapshot {
  return {
    version: 1,
    status: 'playing',
    format: 'commander',
    starting_life: 40,
    viewer_seat: MINE,
    seats: [seat()],
    cards: [card()],
    turn: { number: 1, active_seat: MINE, phase: 'main1' },
    log: [],
    winner: null,
    ...over,
  }
}

const mounted: Array<{ unmount: () => void }> = []

function mountEngine(state: PlaySnapshot = snapshot()) {
  const store = usePlayRoomStore()
  store.handleMessage({ type: 'snapshot', snapshot: state })
  const send = vi.spyOn(store, 'send').mockReturnValue(1)
  let table!: PlayTableApi
  const Host = defineComponent({
    setup() {
      table = usePlayTable()
      return () => null
    },
  })
  mounted.push(mount(Host))
  return { table, store, send }
}

/** A pointer event built by hand: `button` and `pointerType` are read-only on the real ones. */
function pointer(type: string, over: Record<string, unknown> = {}): PointerEvent {
  const event = new Event(type, { bubbles: true })
  Object.assign(event, {
    button: 0,
    clientX: 40,
    clientY: 40,
    pointerId: 1,
    pointerType: 'mouse',
    ...over,
  })
  return event as PointerEvent
}

function press(table: PlayTableApi, over: Record<string, unknown> = {}) {
  table.startCardDrag(pointer('pointerdown', over), card(), 'battlefield')
}

function key(k: string, over: Record<string, unknown> = {}) {
  window.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, ...over }))
}

/** Something modal, of the kind reka portals to the body when a dialog or menu opens. */
function openOverlay(role: string, attrs: Record<string, string> = {}) {
  const el = document.createElement('div')
  el.setAttribute('role', role)
  for (const [name, value] of Object.entries(attrs)) el.setAttribute(name, value)
  document.body.append(el)
  return el
}

beforeEach(() => {
  setActivePinia(createPinia())
})

afterEach(() => {
  while (mounted.length) mounted.pop()?.unmount()
  document.body.innerHTML = ''
  vi.restoreAllMocks()
})

describe('keyboard shortcuts', () => {
  it('acts on a single press and ignores the auto-repeat that follows', () => {
    const { send } = mountEngine()

    key('s')
    expect(send).toHaveBeenCalledTimes(1)

    // A key leant on repeats ~30×/s, and every one of these is a socket frame: thirty
    // shuffles nobody asked for, `too_fast`, and then a 4008 close.
    key('s', { repeat: true })
    key('s', { repeat: true })
    expect(send).toHaveBeenCalledTimes(1)
  })

  it('stands down while a dialog owns the screen', () => {
    const { send } = mountEngine()
    const dialog = openOverlay('dialog')

    // The zone viewer is open — the player is READING their library. `S` here would shuffle
    // the very pile they are searching, because `hoveredId` is still on whatever the pointer
    // crossed on the way to the dialog.
    key('s')
    key('d')
    expect(send).not.toHaveBeenCalled()

    dialog.remove()
    key('s')
    expect(send).toHaveBeenCalledWith({ type: 'shuffle' })
  })

  it('stands down while a card menu is open', () => {
    const { send } = mountEngine()
    openOverlay('menu')

    key('u')

    expect(send).not.toHaveBeenCalled()
  })

  it('keeps working while the card preview is up — it is a picture, not an overlay', () => {
    const { send } = mountEngine()
    // The preview carries `role="dialog"` and follows the pointer around the table, so
    // treating it as modal would turn every shortcut off whenever a card is hovered.
    openOverlay('dialog', { 'data-play-preview': '' })

    key('s')

    expect(send).toHaveBeenCalledWith({ type: 'shuffle' })
  })

  it('closes a pinned preview with Escape, game over or not', () => {
    const { table } = mountEngine(snapshot({ status: 'finished', winner: MINE }))
    table.pinPreview(card(), null)
    expect(table.preview.value).not.toBeNull()

    // "View larger" survives the end of the game, so the key that dismisses it has to as
    // well — otherwise the last thing a pod sees is a card they can't put down.
    key('Escape')

    expect(table.preview.value).toBeNull()
  })
})

describe('press, long press and tap', () => {
  it('taps on a press that neither travelled nor lingered', () => {
    const { table, send } = mountEngine()

    press(table)
    window.dispatchEvent(pointer('pointerup'))

    expect(send).toHaveBeenCalledWith({ type: 'tap', card: 111, tapped: true })
  })

  it('does not also tap the card the context menu just opened on', () => {
    const { table, send } = mountEngine()

    press(table, { pointerType: 'touch' })
    // The long press opened the menu; the finger lifting is the END of that gesture, not a
    // second one. Tapping here taps the permanent behind the menu the player is reading.
    window.dispatchEvent(new Event('contextmenu', { bubbles: true }))
    window.dispatchEvent(pointer('pointerup', { pointerType: 'touch' }))

    expect(send).not.toHaveBeenCalled()
  })

  it('does not tap after a touch press held past the long-press delay', () => {
    vi.useFakeTimers()
    try {
      const { table, send } = mountEngine()

      press(table, { pointerType: 'touch' })
      // reka opens the card menu on its own timer rather than on a `contextmenu` event, so
      // the duration of the press is the other half of the same signal.
      vi.advanceTimersByTime(PLAY_LONG_PRESS_MS + 100)
      window.dispatchEvent(pointer('pointerup', { pointerType: 'touch' }))

      expect(send).not.toHaveBeenCalled()
    } finally {
      vi.useRealTimers()
    }
  })

  it('still taps a slow press with a mouse, which has no long press', () => {
    vi.useFakeTimers()
    try {
      const { table, send } = mountEngine()

      press(table)
      vi.advanceTimersByTime(PLAY_LONG_PRESS_MS + 100)
      window.dispatchEvent(pointer('pointerup'))

      // A mouse opens the menu with the right button, which never starts a drag at all —
      // so a thoughtful left click is just a click, however long it took.
      expect(send).toHaveBeenCalledWith({ type: 'tap', card: 111, tapped: true })
    } finally {
      vi.useRealTimers()
    }
  })
})

describe('custom counters', () => {
  it('clamps the name to what the engine accepts', () => {
    const { table, send } = mountEngine()
    vi.spyOn(window, 'prompt').mockReturnValue('commander tax paid this turn')

    table.runMenuAction('counter_custom', card())

    // Past `MAX_COUNTER_NAME` the engine refuses the whole action rather than truncating, so
    // a wordy counter would come back as an error with nothing to connect it to. The cut is
    // trimmed too: "poison " and "poison" would be two counters on the same card.
    expect(send).toHaveBeenCalledWith({
      type: 'counter',
      card: 111,
      name: 'commander tax paid this',
      delta: 1,
    })
    const sent = send.mock.calls[0]?.[0] as { name?: string } | undefined
    expect(sent?.name?.length ?? 0).toBeLessThanOrEqual(PLAY_COUNTER_NAME_MAX)
  })

  it('sends nothing when the prompt is dismissed', () => {
    const { table, send } = mountEngine()
    vi.spyOn(window, 'prompt').mockReturnValue(null)

    table.runMenuAction('counter_custom', card())

    expect(send).not.toHaveBeenCalled()
  })
})
