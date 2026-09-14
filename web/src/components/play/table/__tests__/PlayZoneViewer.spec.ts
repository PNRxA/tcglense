import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h, nextTick } from 'vue'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import PlayZoneViewer from '../PlayZoneViewer.vue'
import { usePlayTable } from '@/composables/usePlayTable'
import { usePlayRoomStore } from '@/stores/playRoom'
import type { PlayCardDef, PlayCardView, PlaySeatSnapshot, PlaySnapshot } from '@/lib/api/play'

// Looking at the top of your library is the one place on the table where the client holds a
// list the server does not: a peek is a photograph, and "put them back in this order" names
// every card in it. So the failure mode worth pinning is what happens when the photograph goes
// stale under the player's own hand — they pull one card out, then put the rest back.

const MINE = 1

function def(name: string): PlayCardDef {
  return {
    card_id: `id-${name.toLowerCase()}`,
    game: 'mtg',
    name,
    faces: [
      {
        name,
        mana_cost: '{1}',
        type_line: 'Instant',
        oracle_text: null,
        power: null,
        toughness: null,
        loyalty: null,
      },
    ],
    back_image: false,
    has_image: false,
    colors: [],
    cmc: 1,
    is_commander: false,
    is_token: false,
  }
}

function card(id: number, name: string): PlayCardView {
  return {
    id,
    def: def(name),
    owner: MINE,
    controller: MINE,
    zone: 'library',
    tapped: false,
    face_down: false,
    face_index: 0,
    revealed: false,
    counters: {},
    x: 0,
    y: 0,
    attached_to: null,
    power_toughness: null,
  }
}

function seat(): PlaySeatSnapshot {
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
    battlefield: [],
    graveyard: [],
    exile: [],
    command: [],
  }
}

function snapshot(): PlaySnapshot {
  return {
    version: 1,
    status: 'playing',
    format: 'commander',
    starting_life: 40,
    viewer_seat: MINE,
    seats: [seat()],
    cards: [],
    turn: { number: 1, active_seat: MINE, phase: 'main1' },
    log: [],
    winner: null,
  }
}

const PEEK = [card(1, 'Ponder'), card(2, 'Brainstorm'), card(3, 'Preordain')]

const mounted: Array<{ unmount: () => void }> = []

/** The viewer, inside a host that owns the table engine it injects. */
async function mountViewer() {
  const store = usePlayRoomStore()
  store.handleMessage({ type: 'snapshot', snapshot: snapshot() })
  const send = vi.spyOn(store, 'send').mockReturnValue(1)
  const Host = defineComponent({
    setup() {
      usePlayTable()
      return () => h(PlayZoneViewer)
    },
  })
  mounted.push(
    mount(Host, { global: { plugins: [[VueQueryPlugin, { queryClient: new QueryClient() }]] } }),
  )
  // The viewer opens itself when a peek lands — the server answers `look_top` privately, so
  // nothing on the click path knows when it will arrive.
  store.handleMessage({ type: 'peek', peek: { kind: 'look_top', cards: PEEK } })
  await nextTick()
  await nextTick()
  return { store, send }
}

/** The dialog portals to the body, so its buttons are found there rather than in the wrapper. */
function buttons(label: string): HTMLElement[] {
  return [...document.body.querySelectorAll('button')].filter(
    (button) => button.textContent?.trim() === label,
  )
}

/** An icon-only control, found by the name a screen reader would read. */
function byLabel(label: string): HTMLElement | null {
  return document.body.querySelector(`[aria-label="${label}"]`)
}

function click(el: HTMLElement | null | undefined) {
  el?.dispatchEvent(new Event('click', { bubbles: true }))
}

beforeEach(() => {
  setActivePinia(createPinia())
})

afterEach(() => {
  while (mounted.length) mounted.pop()?.unmount()
  document.body.innerHTML = ''
  vi.restoreAllMocks()
})

describe('PlayZoneViewer, looking at the top of the library', () => {
  it('puts back only the cards still on top after one is pulled out', async () => {
    const { send } = await mountViewer()
    expect(buttons('Hand')).toHaveLength(3)

    click(buttons('Hand')[1])
    await nextTick()

    expect(send).toHaveBeenCalledWith(
      expect.objectContaining({ type: 'move_library_card', card: 2, zone: 'hand' }),
    )
    // The card is in hand now, so it is gone from the dialog as well — a peek is a
    // photograph, and this one is out of date the moment anything moves.
    expect(buttons('Hand')).toHaveLength(2)

    click(buttons('Put back in this order')[0])

    // The engine refuses a `reorder_top` naming a card that is no longer in the library, so
    // sending the original three would throw away the ordering the player just did.
    expect(send).toHaveBeenCalledWith({ type: 'reorder_top', cards: [1, 3] })
  })

  it('keeps the order the player arranged, minus the card they took', async () => {
    const { send } = await mountViewer()

    // Order them Ponder → Preordain → Brainstorm, then take Ponder to hand: what goes back
    // has to be the arrangement they just made, minus the card that left.
    click(byLabel('Move Preordain up'))
    await nextTick()
    click(buttons('Hand')[0])
    await nextTick()
    click(buttons('Put back in this order')[0])

    expect(send).toHaveBeenLastCalledWith({ type: 'reorder_top', cards: [3, 2] })
  })

  it('says nothing at all when every card was pulled out', async () => {
    const { send } = await mountViewer()

    for (let i = 0; i < 3; i += 1) {
      click(buttons('Hand')[0])
      await nextTick()
    }
    click(buttons('Put back in this order')[0])

    // An empty `reorder_top` is not an instruction, it's a rejection waiting to happen.
    expect(send).not.toHaveBeenCalledWith(expect.objectContaining({ type: 'reorder_top' }))
  })
})
