import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import PlayTable from '../PlayTable.vue'
import PlayCardMenu from '../PlayCardMenu.vue'
import PlayTableMenu from '../PlayTableMenu.vue'
import { usePlayRoomStore } from '@/stores/playRoom'
import type {
  PlayCardDef,
  PlayCardView,
  PlaySeatSnapshot,
  PlaySnapshot,
  PlayZone,
} from '@/lib/api/play'

// The table, mounted against a fixture snapshot — the same shape the socket delivers.
//
// The backend isn't runnable yet, so the contract *is* the test: `handleMessage` folds a
// `snapshot` frame into the store exactly as a live server would, and everything asserted here
// is a rule that has to survive the wiring — most of all the privacy one, that an opponent's
// board shows counts and backs rather than cards.

function def(name: string, overrides: Partial<PlayCardDef> = {}): PlayCardDef {
  return {
    card_id: `id-${name.toLowerCase().replace(/\s+/g, '-')}`,
    game: 'mtg',
    name,
    faces: [
      {
        name,
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
    ...overrides,
  }
}

function view(
  id: number,
  zone: PlayZone,
  seat: number,
  cardDef: PlayCardDef | null,
  overrides: Partial<PlayCardView> = {},
): PlayCardView {
  return {
    id,
    def: cardDef,
    owner: seat,
    controller: seat,
    zone,
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

function seat(overrides: Partial<PlaySeatSnapshot> & { id: number }): PlaySeatSnapshot {
  return {
    seat_index: 0,
    name: 'Ana',
    is_host: false,
    deck_name: null,
    life: 40,
    counters: {},
    commander_damage: {},
    out: false,
    connected: true,
    library_count: 92,
    hand_count: 0,
    hand: null,
    battlefield: [],
    graveyard: [],
    exile: [],
    command: [],
    ...overrides,
  }
}

const MINE = 1
const THEIRS = 2

function snapshot(overrides: Partial<PlaySnapshot> = {}): PlaySnapshot {
  return {
    version: 1,
    status: 'playing',
    format: 'commander',
    starting_life: 40,
    viewer_seat: MINE,
    seats: [
      seat({
        id: MINE,
        seat_index: 0,
        name: 'Ana',
        is_host: true,
        hand_count: 2,
        hand: [101, 102],
        battlefield: [111],
        graveyard: [121],
        command: [131],
      }),
      seat({
        id: THEIRS,
        seat_index: 1,
        name: 'Bo',
        life: 37,
        hand_count: 5,
        library_count: 88,
        battlefield: [211, 212],
        counters: { poison: 3 },
      }),
    ],
    cards: [
      view(101, 'hand', MINE, def('Sol Ring')),
      view(102, 'hand', MINE, def('Swords to Plowshares')),
      view(111, 'battlefield', MINE, def('Llanowar Elves'), { x: 0.3, y: 0.4 }),
      view(121, 'graveyard', MINE, def('Lightning Bolt')),
      view(131, 'command', MINE, def('Atraxa', { is_commander: true })),
      view(211, 'battlefield', THEIRS, def('Birds of Paradise'), { x: 0.2, y: 0.2 }),
      // Their face-down permanent: the server sends the *existence* and nothing else.
      view(212, 'battlefield', THEIRS, null, { face_down: true, x: 0.7, y: 0.6 }),
    ],
    turn: { number: 3, active_seat: THEIRS, phase: 'main1' },
    // The server narrates the whole sentence for an action (`logLine` only punctuates chat),
    // so the fixture carries the subject the way the wire does.
    log: [
      { id: 1, at: '2026-01-01T00:00:00Z', kind: 'action', seat: THEIRS, text: 'Bo drew a card' },
      { id: 2, at: '2026-01-01T00:00:01Z', kind: 'chat', seat: MINE, text: 'nice' },
    ],
    winner: null,
    ...overrides,
  }
}

/** Every table mounted in a test, so each one is torn down before the next pinia. */
const mounted: { unmount: () => void }[] = []

function mountTable(state: PlaySnapshot = snapshot()) {
  const store = usePlayRoomStore()
  store.game = 'mtg'
  store.code = 'ABC123'
  store.handleMessage({ type: 'snapshot', snapshot: state })
  const send = vi.spyOn(store, 'send').mockReturnValue(1)
  const wrapper = mount(PlayTable, {
    global: { plugins: [[VueQueryPlugin, { queryClient: new QueryClient() }]] },
  })
  mounted.push(wrapper)
  return { wrapper, store, send }
}

/**
 * Dispatch a pointer gesture by hand: `trigger` builds a real `MouseEvent`, whose `button` is
 * read-only, and a press with no `button: 0` is not a left-click as far as the drag machine is
 * concerned.
 */
function pointer(target: EventTarget, type: string, x: number, y: number) {
  const event = new Event(type, { bubbles: true })
  Object.assign(event, { button: 0, clientX: x, clientY: y, pointerId: 1 })
  target.dispatchEvent(event)
}

/** Every card button on the table, by the accessible name a screen reader would read. */
function cardLabels(wrapper: ReturnType<typeof mount>): string[] {
  return wrapper
    .findAll('[data-play-card]')
    .map((el) => el.attributes('aria-label') ?? '')
    .filter(Boolean)
}

describe('PlayTable', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  afterEach(() => {
    // The table holds window-level listeners (the drag machine, the shortcuts) and portals;
    // leaving one mounted would let it react to the next test's store.
    while (mounted.length) mounted.pop()?.unmount()
  })

  it('renders my own hand as cards', () => {
    const { wrapper } = mountTable()
    const labels = cardLabels(wrapper)
    expect(labels).toContain('Sol Ring')
    expect(labels).toContain('Swords to Plowshares')
  })

  it("shows an opponent's hand and library as counts, never as cards", () => {
    const { wrapper } = mountTable()
    const board = wrapper.find('[aria-label="Bo, 37 life"]')
    expect(board.exists()).toBe(true)
    expect(board.text()).toContain('Hand 5')
    expect(board.text()).toContain('Library 88')
    expect(board.text()).toContain('3 poison')
    // Their hand cards have no ids in the snapshot at all, so there is nothing to render.
    expect(board.findAll('[data-play-card]')).toHaveLength(2)
  })

  it("renders an opponent's face-down permanent as a back with no name", () => {
    const { wrapper } = mountTable()
    const labels = cardLabels(wrapper)
    expect(labels).toContain('Face-down card, face down')
    const back = wrapper.find('[aria-label="Face-down card, face down"]')
    expect(back.find('img').exists()).toBe(false)
    expect(back.text()).toContain('TCGLense')
  })

  it('taps a battlefield card on a press that did not travel', async () => {
    const { wrapper, send } = mountTable()
    const card = wrapper.find('[aria-label="Llanowar Elves"]')
    expect(card.exists()).toBe(true)
    pointer(card.element, 'pointerdown', 40, 40)
    pointer(window, 'pointerup', 40, 40)
    expect(send).toHaveBeenCalledWith({ type: 'tap', card: 111, tapped: true })
  })

  it('does not let me pass a turn that is not mine', () => {
    const { wrapper } = mountTable()
    const pass = wrapper.findAll('button').find((b) => b.text().includes('Pass turn'))
    expect(pass?.attributes('disabled')).toBeDefined()
  })

  it('passes the turn once it comes round', async () => {
    const { wrapper, send } = mountTable(
      snapshot({ turn: { number: 4, active_seat: MINE, phase: 'untap' } }),
    )
    const pass = wrapper.findAll('button').find((b) => b.text().includes('Pass turn'))
    expect(pass?.attributes('disabled')).toBeUndefined()
    await pass?.trigger('click')
    expect(send).toHaveBeenCalledWith({ type: 'pass_turn' })
  })

  it('offers each card the actions its zone and owner allow', () => {
    const { wrapper } = mountTable()
    const menus = wrapper.findAllComponents(PlayCardMenu)
    const byCard = new Map(
      menus.map((menu) => [
        (menu.props('card') as PlayCardView).id,
        (menu.vm as unknown as { actions: { id: string }[] }).actions.map((a) => a.id),
      ]),
    )

    // My permanent: the full battlefield vocabulary.
    expect(byCard.get(111)).toEqual(expect.arrayContaining(['tap', 'plus_one', 'to_graveyard']))
    // My hand: playing and revealing, never tapping.
    expect(byCard.get(101)).toEqual(expect.arrayContaining(['play', 'reveal']))
    expect(byCard.get(101)).not.toContain('tap')
    // Their permanent: the one reach across the table, and a closer look.
    expect(byCard.get(211)).toEqual(['take_control', 'view'])
  })

  it('reads the log with the seat that did it, and chat as speech', () => {
    const { wrapper } = mountTable()
    expect(wrapper.text()).toContain('Bo drew a card')
    // Chat is the half the client punctuates, so it proves `logLine` is actually in the path.
    expect(wrapper.text()).toContain('Ana: nice')
  })

  it('keeps all four of my zones on the rail, in reach and in order', () => {
    // A regression with teeth: the rail used to stack four full-size card boxes, which pushed
    // the exile's label and the entire command zone below the fold of its own scroller at
    // 1440×900 — with nothing on screen to say they existed. Command leads because in
    // Commander it is the pile you touch most.
    const { wrapper } = mountTable()
    const rail = wrapper.find('aside[aria-label="Your zones"]')
    expect(rail.exists()).toBe(true)
    const drops = [...rail.element.querySelectorAll('[data-play-drop]')].map(
      (el) => (el as HTMLElement).dataset.playDrop,
    )
    expect(drops).toEqual(['command', 'library', 'graveyard', 'exile'])
  })

  it('announces the game being over', () => {
    const { wrapper } = mountTable(snapshot({ status: 'finished', winner: THEIRS }))
    expect(wrapper.text()).toContain('Game over — Bo won')
  })

  it('confirms before conceding, and only then sends it', async () => {
    const { wrapper, send } = mountTable()
    const menu = wrapper.findComponent(PlayTableMenu)
    expect((menu.vm as unknown as { items: { id: string }[] }).items.map((i) => i.id)).toContain(
      'concede',
    )

    // Choosing the entry opens the confirm and sends nothing — scooping has no undo.
    ;(menu.vm as unknown as { select: (id: string) => void }).select('concede')
    await wrapper.vm.$nextTick()
    expect(send).not.toHaveBeenCalledWith({ type: 'concede' })

    // The dialog portals to the body, so it is found there rather than in the wrapper.
    const confirm = document.body.querySelector('[data-play-confirm="concede"]')
    expect(confirm).not.toBeNull()
    confirm?.dispatchEvent(new Event('click', { bubbles: true }))
    expect(send).toHaveBeenCalledWith({ type: 'concede' })
  })

  it("keeps the host's game-ending verbs out of everyone else's menu", () => {
    // Seat 1 hosts in the default fixture, so the host sees all three...
    const host = mountTable()
    const hostItems = (
      host.wrapper.findComponent(PlayTableMenu).vm as unknown as { items: { id: string }[] }
    ).items.map((item) => item.id)
    expect(hostItems).toEqual(['concede', 'end_game', 'set_active'])
  })

  it('offers a non-host only the verb that is theirs to use', () => {
    // ...and a guest sees only their own scoop: the engine answers `HostOnly` to the rest.
    const guest = mountTable(
      snapshot({
        viewer_seat: THEIRS,
        seats: [
          seat({ id: MINE, seat_index: 0, name: 'Ana', is_host: true }),
          seat({ id: THEIRS, seat_index: 1, name: 'Bo' }),
        ],
        cards: [],
      }),
    )
    const items = (
      guest.wrapper.findComponent(PlayTableMenu).vm as unknown as { items: { id: string }[] }
    ).items.map((item) => item.id)
    expect(items).toEqual(['concede'])
    expect(items).not.toContain('end_game')
    expect(items).not.toContain('set_active')
  })

  it('stands the table verbs down once the game is finished, but not the chat', async () => {
    const { wrapper, send } = mountTable(snapshot({ status: 'finished', winner: THEIRS }))
    const button = (text: string) => wrapper.findAll('button').find((b) => b.text().includes(text))
    expect(button('Draw')?.attributes('disabled')).toBeDefined()
    expect(button('Pass turn')?.attributes('disabled')).toBeDefined()
    expect(wrapper.find('[aria-label="Gain a life"]').attributes('disabled')).toBeDefined()

    // And a press that slips past the disabled chrome is refused by the engine as well —
    // the gate is in one place, not only on the buttons.
    const card = wrapper.find('[aria-label="Llanowar Elves"]')
    pointer(card.element, 'pointerdown', 40, 40)
    pointer(window, 'pointerup', 40, 40)
    expect(send).not.toHaveBeenCalledWith({ type: 'tap', card: 111, tapped: true })

    // Chat, though, is exactly what a pod does in the minute after a game ends.
    const input = wrapper.find('input[aria-label="Chat message"]')
    expect(input.exists()).toBe(true)
    await input.setValue('gg')
    input.element.closest('form')?.dispatchEvent(new Event('submit', { bubbles: true }))
    expect(send).toHaveBeenCalledWith({ type: 'chat', text: 'gg' })
  })

  it('leaves navigation to the view it is mounted in', async () => {
    const { wrapper } = mountTable()
    await wrapper.find('[aria-label="Leave the table"]').trigger('click')
    expect(wrapper.emitted('leave')).toHaveLength(1)
  })
})
