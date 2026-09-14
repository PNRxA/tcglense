import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h } from 'vue'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { usePlayTable, type PlayTableApi } from '@/composables/usePlayTable'
import { usePlayRoomStore } from '@/stores/playRoom'
import type { PlayCardView, PlaySeatSnapshot, PlaySnapshot } from '@/lib/api/play'

// The table engine on its own — the rules that are about *input*, not about markup.
//
// Two of them only ever show up on a device the unit suite doesn't have: a finger produces a
// `pointerenter` on its way into a tap (so honouring hover covered a phone in a full-screen
// card), and a `focus` carries no pointer type at all (so the tap that follows would have
// re-opened it). Both are asserted here rather than in a component, because the decision lives
// in one function and a component test would only be testing that it was called.

function seat(overrides: Partial<PlaySeatSnapshot> & { id: number }): PlaySeatSnapshot {
  return {
    seat_index: 0,
    name: 'Ana',
    is_host: true,
    deck_name: null,
    life: 40,
    counters: {},
    commander_damage: {},
    out: false,
    connected: true,
    library_count: 90,
    hand_count: 1,
    hand: [101],
    battlefield: [111],
    graveyard: [],
    exile: [],
    command: [],
    ...overrides,
  }
}

function card(id: number, zone: PlayCardView['zone'], owner = 1): PlayCardView {
  return {
    id,
    def: {
      card_id: `c${id}`,
      game: 'mtg',
      name: `Card ${id}`,
      faces: [
        {
          name: `Card ${id}`,
          mana_cost: null,
          type_line: null,
          oracle_text: null,
          power: null,
          toughness: null,
          loyalty: null,
        },
      ],
      back_image: false,
      has_image: true,
      colors: [],
      cmc: null,
      is_commander: false,
      is_token: false,
    },
    owner,
    controller: owner,
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
  }
}

function snapshot(overrides: Partial<PlaySnapshot> = {}): PlaySnapshot {
  return {
    version: 1,
    status: 'playing',
    format: 'commander',
    starting_life: 40,
    viewer_seat: 1,
    seats: [seat({ id: 1 })],
    cards: [card(101, 'hand'), card(111, 'battlefield')],
    turn: { number: 1, active_seat: 1, phase: 'main1' },
    log: [],
    winner: null,
    ...overrides,
  }
}

/** jsdom has no media engine at all, so the layout queries are answered by hand. */
function stubMatchMedia(answer: (query: string) => boolean) {
  Object.defineProperty(window, 'matchMedia', {
    configurable: true,
    writable: true,
    value: (query: string) => ({
      media: query,
      matches: answer(query),
      addEventListener: () => {},
      removeEventListener: () => {},
    }),
  })
}

const mounted: { unmount: () => void }[] = []

function makeTable(state: PlaySnapshot = snapshot()) {
  const store = usePlayRoomStore()
  store.game = 'mtg'
  store.handleMessage({ type: 'snapshot', snapshot: state })
  const send = vi.spyOn(store, 'send').mockReturnValue(1)
  let api!: PlayTableApi
  const wrapper = mount(
    defineComponent({
      setup() {
        api = usePlayTable()
        return () => h('div')
      },
    }),
  )
  mounted.push(wrapper)
  return { api, store, send }
}

/** A stand-in for the card element a hover would be anchored to. */
const anchor = null

describe('usePlayTable', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  afterEach(() => {
    while (mounted.length) mounted.pop()?.unmount()
    Reflect.deleteProperty(window, 'matchMedia')
  })

  it('opens the hover preview for a mouse', () => {
    const { api } = makeTable()
    api.hoverCard(api.store.card(101)!, anchor, 'mouse')
    expect(api.preview.value?.card.id).toBe(101)
  })

  it('opens no preview for a finger', () => {
    // A touch `pointerenter` is a tap arriving, not a request to see the card full size.
    const { api } = makeTable()
    api.hoverCard(api.store.card(101)!, anchor, 'touch')
    expect(api.preview.value).toBeNull()
    expect(api.coarsePointer.value).toBe(true)
    // ...and it still records what is under the finger, which the shortcuts read.
    expect(api.hoveredId.value).toBe(101)
  })

  it('opens no preview for a pen either', () => {
    const { api } = makeTable()
    api.hoverCard(api.store.card(101)!, anchor, 'pen')
    expect(api.preview.value).toBeNull()
  })

  it('keeps a focus from re-opening what a tap just suppressed', () => {
    // `focus` has no pointer type, and on touch the tap focuses the card it hit — so without
    // remembering the last pointer, every tap would still end in a full-screen preview.
    const { api } = makeTable()
    const event = new Event('pointerdown', { bubbles: true })
    Object.assign(event, { pointerType: 'touch' })
    window.dispatchEvent(event)
    api.hoverCard(api.store.card(101)!, anchor)
    expect(api.preview.value).toBeNull()
  })

  it('goes back to hovering when a mouse takes over', () => {
    const { api } = makeTable()
    api.hoverCard(api.store.card(101)!, anchor, 'touch')
    expect(api.preview.value).toBeNull()
    api.hoverCard(api.store.card(101)!, anchor, 'mouse')
    expect(api.preview.value?.card.id).toBe(101)
  })

  it('reads the layout off media queries', () => {
    stubMatchMedia((query) => query.includes('max-width') || query.includes('coarse'))
    const { api } = makeTable()
    expect(api.isPhone.value).toBe(true)
    expect(api.isShort.value).toBe(false)
    // A coarse-pointer device starts out not hovering, before any event has arrived.
    expect(api.coarsePointer.value).toBe(true)
  })

  it('stays on the desktop layout where there is no media engine at all', () => {
    const { api } = makeTable()
    expect(api.isPhone.value).toBe(false)
    expect(api.isShort.value).toBe(false)
  })

  it('only calls a card of mine in my hand a hand selection', () => {
    const { api } = makeTable()
    api.selectedId.value = 111 // on the battlefield
    expect(api.selectedHandCard.value).toBeNull()
    api.selectedId.value = 101
    expect(api.selectedHandCard.value?.id).toBe(101)
    api.clearSelection()
    expect(api.selectedHandCard.value).toBeNull()
  })

  it('stands every verb down once the game is finished', () => {
    const { api, send } = makeTable(snapshot({ status: 'finished', winner: 1 }))
    api.draw(1)
    api.shuffle()
    api.untapAll()
    api.passTurn()
    api.tapCard(api.store.card(111)!)
    expect(send).not.toHaveBeenCalled()
    // Chat is not a table verb — a pod talks after the game.
    api.chat('gg')
    expect(send).toHaveBeenCalledWith({ type: 'chat', text: 'gg' })
  })
})
