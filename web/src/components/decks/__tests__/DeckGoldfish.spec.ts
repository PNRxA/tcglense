import { describe, expect, it, vi } from 'vitest'
import type { Ref } from 'vue'
import { mount } from '@vue/test-utils'
import DeckGoldfish from '@/components/decks/DeckGoldfish.vue'
import type { Card, GoldfishHand } from '@/lib/api'

// The shuffle, the mulligan and the draw are all the server's (issue #596). What this
// component owns is the four values that address a hand — seed, mulligans, what was
// bottomed, how many drawn — so these tests drive the buttons and assert on the request
// those four produce.

interface Params {
  seed?: number
  mulligans: number
  bottom: string[]
  draws: number
  opening: number
}

const captured = vi.hoisted(() => ({ params: null as Ref<Params> | null }))

function card(id: string, name: string): Card {
  return { id, name, has_image: true } as Card
}

/** A hand derived from the request, so a click's effect is visible in what renders. */
function hand(params: Params): GoldfishHand {
  const library = Array.from({ length: 40 }, (_, index) => card(`c${index}`, `Card ${index}`))
  const opening = library.slice(0, params.opening)
  const kept = opening.filter((entry) => !params.bottom.includes(entry.id))
  const drawn = library.slice(params.opening, params.opening + params.draws)
  return {
    seed: params.seed ?? 0,
    mulligans: params.mulligans,
    opening: params.opening,
    draws: drawn.length,
    to_bottom: Math.max(0, params.mulligans - params.bottom.length),
    hand: [...kept, ...drawn],
    bottomed: opening.filter((entry) => params.bottom.includes(entry.id)),
    library_size: library.length - params.opening - drawn.length,
    library_total: library.length,
    section_ids: [1],
  }
}

vi.mock('@/composables/useDeckAnalysis', async () => {
  const { computed } = await import('vue')
  return {
    useDeckGoldfishQuery: (_game: unknown, _deckId: unknown, params: Ref<Params>) => {
      captured.params = params
      return {
        data: computed(() => (params.value.seed === undefined ? undefined : hand(params.value))),
      }
    },
    usePublicDeckGoldfishQuery: () => ({ data: computed(() => undefined) }),
  }
})

function mountPanel() {
  return mount(DeckGoldfish, { props: { game: 'mtg', deckId: 7 } })
}

function buttonNamed(wrapper: ReturnType<typeof mountPanel>, text: string) {
  return wrapper.findAll('button').find((button) => button.text().includes(text))!
}

describe('DeckGoldfish', () => {
  it('deals nothing until asked, then an opening seven', async () => {
    const wrapper = mountPanel()
    expect(wrapper.text()).toContain('Draw opening hand')
    expect(wrapper.findAll('li')).toHaveLength(0)

    await buttonNamed(wrapper, 'Draw opening hand').trigger('click')
    expect(captured.params!.value.seed).toBeTypeOf('number')
    expect(captured.params!.value.opening).toBe(7)
    expect(wrapper.findAll('li')).toHaveLength(7)
    expect(wrapper.text()).toContain('7 in hand · 33 in library')
  })

  it('mulligans by reshuffling and owing a card to the bottom', async () => {
    const wrapper = mountPanel()
    await buttonNamed(wrapper, 'Draw opening hand').trigger('click')
    const seed = captured.params!.value.seed

    await buttonNamed(wrapper, 'Mulligan to 6').trigger('click')
    // Same seed: a mulligan is a different shuffle of the same deck, not a different deck.
    expect(captured.params!.value.seed).toBe(seed)
    expect(captured.params!.value.mulligans).toBe(1)
    expect(wrapper.text()).toContain('Put 1 card on the bottom')
    // Drawing is blocked until the bottoming is done.
    expect((buttonNamed(wrapper, 'Draw').element as HTMLButtonElement).disabled).toBe(true)

    // Clicking a card in hand bottoms it.
    await wrapper.findAll('li button')[2]!.trigger('click')
    expect(captured.params!.value.bottom).toHaveLength(1)
    expect(wrapper.findAll('li')).toHaveLength(6)
    expect(wrapper.text()).toContain('On the bottom:')
    expect((buttonNamed(wrapper, 'Draw').element as HTMLButtonElement).disabled).toBe(false)
  })

  it('draws one card at a time and marks what was drawn', async () => {
    const wrapper = mountPanel()
    await buttonNamed(wrapper, 'Draw opening hand').trigger('click')

    await buttonNamed(wrapper, 'Draw').trigger('click')
    expect(captured.params!.value.draws).toBe(1)
    expect(wrapper.findAll('li')).toHaveLength(8)
    expect(wrapper.text()).toContain('1 drawn')
    expect(wrapper.findAll('li').filter((item) => item.text().includes('drawn'))).toHaveLength(1)
  })

  it('replays a hand from a typed seed, from the top', async () => {
    const wrapper = mountPanel()
    await buttonNamed(wrapper, 'Draw opening hand').trigger('click')
    await buttonNamed(wrapper, 'Draw').trigger('click')
    expect(captured.params!.value.draws).toBe(1)

    const field = wrapper.find<HTMLInputElement>('input[type="text"]')
    await field.setValue('4242')
    await field.trigger('change')
    expect(captured.params!.value.seed).toBe(4242)
    // A typed seed starts that hand over — carrying a draw step across would be applying
    // an old decision to a different shuffle.
    expect(captured.params!.value.draws).toBe(0)
    expect(captured.params!.value.mulligans).toBe(0)
    expect(captured.params!.value.bottom).toEqual([])
  })

  it('starts a whole new hand on a new seed', async () => {
    const wrapper = mountPanel()
    await buttonNamed(wrapper, 'Draw opening hand').trigger('click')
    const first = captured.params!.value.seed
    await buttonNamed(wrapper, 'Mulligan to 6').trigger('click')

    await buttonNamed(wrapper, 'New hand').trigger('click')
    expect(captured.params!.value.seed).not.toBe(first)
    expect(captured.params!.value.mulligans).toBe(0)
    expect(captured.params!.value.bottom).toEqual([])
  })
})
