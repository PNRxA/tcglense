import { beforeEach, describe, expect, it, vi } from 'vitest'
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
/** What the shared detail-modal seam was asked to do: every plain click on a card in hand,
 * as `[kind, game, id]`, and every chunk warm. The seam itself is covered by its own spec. */
const modal = vi.hoisted(() => ({
  activations: [] as [string, string, string][],
  warms: [] as string[],
}))
/** Flip to make the next mounted panel's query report a failure. */
const failNext = vi.hoisted(() => ({ value: false }))
/** The query's in-flight state for the next mounted panel: `fetching` alone is a hand being
 * replaced (keepPreviousData holds the last one), `fetching` + `blank` is the opening deal
 * with nothing yet to show. */
const inFlight = vi.hoisted(() => ({ fetching: false, blank: false }))

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
      const failing = failNext.value
      const { fetching, blank } = inFlight
      return {
        data: computed(() =>
          failing || blank || params.value.seed === undefined ? undefined : hand(params.value),
        ),
        error: computed(() => (failing ? new Error('That hand could not be dealt.') : null)),
        isFetching: computed(() => fetching),
      }
    },
    usePublicDeckGoldfishQuery: () => ({
      data: computed(() => undefined),
      error: computed(() => null),
      isFetching: computed(() => false),
    }),
  }
})

vi.mock('@/composables/useDetailModalLink', () => ({
  useDetailModalLink: () => ({
    hrefFor: (_kind: string, game: string, id: string) => `/cards/${game}/cards/${id}`,
    onActivate: (event: MouseEvent, kind: string, game: string, id: string) => {
      event.preventDefault()
      modal.activations.push([kind, game, id])
    },
    warm: (kind: string) => {
      modal.warms.push(kind)
    },
  }),
}))

beforeEach(() => {
  failNext.value = false
  inFlight.fetching = false
  inFlight.blank = false
  modal.activations = []
  modal.warms = []
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

    // While a card is owed, the hand is buttons that bottom, not links that open: the prompt
    // says "click one in your hand", and that click must not open a modal instead.
    expect(wrapper.findAll('li button')).toHaveLength(7)
    expect(wrapper.findAll('li a')).toHaveLength(0)
    // The tile still names its card on hover — this is the one state where a card can't be
    // opened to check, and the moment a player has to tell one piece of art from another.
    expect(wrapper.findAll('li button')[2]!.attributes('title')).toBe('Card 2')
    expect(wrapper.findAll('li button')[2]!.attributes('aria-label')).toBe(
      'Put Card 2 on the bottom',
    )

    // Clicking a card in hand bottoms it.
    await wrapper.findAll('li button')[2]!.trigger('click')
    expect(captured.params!.value.bottom).toHaveLength(1)
    expect(modal.activations).toEqual([])
    expect(wrapper.findAll('li')).toHaveLength(6)
    expect(wrapper.text()).toContain('On the bottom:')
    expect((buttonNamed(wrapper, 'Draw').element as HTMLButtonElement).disabled).toBe(false)

    // The bottom paid, the hand goes back to being cards you can open.
    expect(wrapper.findAll('li button')).toHaveLength(0)
    expect(wrapper.findAll('li a')).toHaveLength(6)
  })

  it('opens a card in hand in the detail modal, keeping the real card page as its href', async () => {
    const wrapper = mountPanel()
    await buttonNamed(wrapper, 'Draw opening hand').trigger('click')

    // Every card in hand is a link to its own page — the href modifier/middle clicks and
    // "open in new tab" follow — and none is a bottoming button, since nothing is owed.
    const links = wrapper.findAll('li a')
    expect(links).toHaveLength(7)
    expect(wrapper.findAll('li button')).toHaveLength(0)
    expect(links[2]!.attributes('href')).toBe('/cards/mtg/cards/c2')
    // The grid shows art only, so the name rides the link for hover and assistive tech.
    expect(links[2]!.attributes('title')).toBe('Card 2')
    expect(links[2]!.find('img').attributes('alt')).toBe('Card 2')

    // A plain click goes to the shared seam, which opens the modal over this page.
    await links[2]!.trigger('click')
    expect(modal.activations).toEqual([['card', 'mtg', 'c2']])
    // And the click was a modal open, not a bottom: the hand is untouched.
    expect(captured.params!.value.bottom).toEqual([])
    expect(wrapper.findAll('li')).toHaveLength(7)

    // Hovering a card warms the dialog chunk, as every other card link on the page does.
    await links[4]!.trigger('pointerenter')
    expect(modal.warms).toEqual(['card'])
  })

  it('opens a drawn card too — the badge over its corner never swallows the tap', async () => {
    const wrapper = mountPanel()
    await buttonNamed(wrapper, 'Draw opening hand').trigger('click')
    await buttonNamed(wrapper, 'Draw').trigger('click')

    const drawn = wrapper.findAll('li').find((item) => item.text().includes('drawn'))!
    expect(drawn.find('a').attributes('href')).toBe('/cards/mtg/cards/c7')
    // The badge is a sibling laid over the link, so it has to let pointer events through.
    expect(drawn.find('span').classes()).toContain('pointer-events-none')

    await drawn.find('a').trigger('click')
    expect(modal.activations).toEqual([['card', 'mtg', 'c7']])
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
    expect(field.element.value).toBe('4242')
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

  it('rejects a seed it cannot use and snaps the field back', async () => {
    const wrapper = mountPanel()
    await buttonNamed(wrapper, 'Draw opening hand').trigger('click')
    const seed = captured.params!.value.seed!
    const field = wrapper.find<HTMLInputElement>('input[type="text"]')

    for (const bad of ['not a seed', '-1', '1.5', '99999999999']) {
      await field.setValue(bad)
      await field.trigger('change')
      expect(captured.params!.value.seed).toBe(seed)
      // The box must not keep showing text that isn't the seed on screen.
      expect(field.element.value).toBe(String(seed))
    }

    // Clearing the box (to paste a shared seed) is not "replay seed 0".
    await field.setValue('')
    await field.trigger('change')
    expect(captured.params!.value.seed).toBe(seed)
    expect(field.element.value).toBe(String(seed))
  })

  it('says so when a hand cannot be dealt', async () => {
    failNext.value = true
    const wrapper = mountPanel()
    await buttonNamed(wrapper, 'Draw opening hand').trigger('click')
    expect(wrapper.text()).toContain('could not be dealt')
    expect(wrapper.findAll('li')).toHaveLength(0)
    // The way out is still on screen — with no hand, that's the same call to action as a
    // fresh panel.
    expect(buttonNamed(wrapper, 'Draw opening hand')).toBeTruthy()
  })
})

describe('DeckGoldfish loading states', () => {
  it('draws the frames it is about to fill while the opening hand is in the air', async () => {
    inFlight.fetching = true
    inFlight.blank = true
    const wrapper = mountPanel()

    await buttonNamed(wrapper, 'Draw opening hand').trigger('click')

    // The card used to collapse to a bare header for the whole round trip.
    expect(wrapper.text()).toContain('Shuffling up…')
    expect(wrapper.findAll('[data-slot="skeleton"]')).toHaveLength(7)
    // The spinner is in the button that was clicked, and it can't be clicked again.
    const button = buttonNamed(wrapper, 'Draw opening hand').element as HTMLButtonElement
    expect(button.disabled).toBe(true)
    expect(button.querySelector('.animate-spin')).not.toBeNull()
  })

  it('holds the hand on screen and says it is dealing the next one', async () => {
    inFlight.fetching = true
    const wrapper = mountPanel()

    await buttonNamed(wrapper, 'Draw opening hand').trigger('click')

    // keepPreviousData means the hand stays put rather than blanking — so the cue is what
    // separates "the click did nothing" from "the click is in the air".
    expect(wrapper.findAll('li')).toHaveLength(7)
    expect(wrapper.text()).toContain('Dealing…')
    // The counts describe the hand *before* the click, so they don't get to claim otherwise.
    expect(wrapper.text()).not.toContain('7 in hand · 33 in library')
    // And the hand is inert: clicking a card mid-flight would bottom one twice.
    expect(wrapper.find('[inert]').exists()).toBe(true)
  })

  it('leaves no cue behind once the hand has landed', async () => {
    const wrapper = mountPanel()
    await buttonNamed(wrapper, 'Draw opening hand').trigger('click')

    expect(wrapper.text()).toContain('7 in hand · 33 in library')
    expect(wrapper.text()).not.toContain('Dealing…')
    expect(wrapper.find('[inert]').exists()).toBe(false)
    expect((buttonNamed(wrapper, 'New hand').element as HTMLButtonElement).disabled).toBe(false)
  })
})
