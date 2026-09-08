import { beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import DeckCompare from '@/components/decks/DeckCompare.vue'
import { makeCard } from '@/test/fixtures'
import type { Deck, DeckDiff } from '@/lib/api'

// The fold is the server's, so what's under test is the panel's contract around it: the
// picked deck rides `?compare=` (and is read back from it), the diff is only asked for once
// a deck is picked, the two layouts render one response two ways (a section move shows per
// section and nets out whole-deck), a finish-only change is worded as such, and the empty
// states say which "nothing" they are.

const state = vi.hoisted(() => ({
  decks: [] as Deck[],
  decksLoaded: true,
  diff: null as DeckDiff | null,
  pending: false,
  fetching: false,
  loadingError: false,
  refetchError: false,
  enabledRef: null as { value: boolean } | null,
  otherIdRef: null as { value: number } | null,
}))

vi.mock('@/composables/useDecks', async () => {
  const { computed } = await import('vue')
  return {
    useDecksQuery: () => ({
      data: computed(() => (state.decksLoaded ? { data: state.decks } : undefined)),
      isPending: computed(() => !state.decksLoaded),
      isSuccess: computed(() => state.decksLoaded),
    }),
    useDeckDiffQuery: (
      _game: unknown,
      _deckId: unknown,
      otherId: { value: number },
      enabled: { value: boolean },
    ) => {
      state.enabledRef = enabled
      state.otherIdRef = otherId
      return {
        data: computed(() => (state.pending ? undefined : (state.diff ?? undefined))),
        isPending: computed(() => state.pending),
        isFetching: computed(() => state.fetching || state.pending),
        isLoadingError: computed(() => state.loadingError),
        isRefetchError: computed(() => state.refetchError),
      }
    },
  }
})

vi.mock('@/composables/useDetailModalLink', () => ({
  useDetailModalLink: () => ({
    hrefFor: (_kind: string, game: string, id: string) => `/cards/${game}/cards/${id}`,
    onActivate: () => {},
    warm: () => {},
  }),
}))

// A plain `<select>` in place of the shadcn Select, so a pick is one DOM event.
const SelectStub = defineComponent({
  props: { modelValue: { type: String, required: true } },
  emits: ['update:modelValue'],
  setup(props, { emit, slots }) {
    return () =>
      h(
        'select',
        {
          'data-test': 'picker',
          value: props.modelValue,
          onChange: (e: Event) => emit('update:modelValue', (e.target as HTMLSelectElement).value),
        },
        slots.default?.(),
      )
  },
})
const SelectItemStub = defineComponent({
  props: { value: { type: String, required: true } },
  setup(props, { slots }) {
    return () => h('option', { value: props.value }, slots.default?.())
  },
})
// Fragments, not wrappers: an `<option>` inside a `<div>` inside a `<select>` isn't one the
// select can take a value from, and `setValue` would silently land on `''`.
const Passthrough = defineComponent({
  setup(_, { slots }) {
    return () => slots.default?.()
  },
})

function deck(id: number, name: string, card_count = 60): Deck {
  return {
    id,
    game: 'mtg',
    name,
    description: null,
    format: null,
    folder_id: null,
    is_public: false,
    card_count,
    color_identity: null,
    commanders: [],
    value_usd: null,
    created_at: '',
    updated_at: '',
  }
}

function diff(over: Partial<DeckDiff> = {}): DeckDiff {
  return {
    base: { id: 1, name: 'Krenko v1', format: null, total_cards: 60 },
    other: { id: 2, name: 'Krenko v2', format: null, total_cards: 60 },
    summary: { added: 0, removed: 0, changed: 0, finish_changed: 0, unchanged: 60 },
    cards: [],
    sections: [],
    ...over,
  }
}

async function mountAt(path: string) {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/decks/:game/:id', component: { template: '<div />' } }],
  })
  await router.push(path)
  await router.isReady()
  const wrapper = mount(DeckCompare, {
    props: { game: 'mtg', deckId: 1 },
    global: {
      plugins: [router],
      stubs: {
        Select: SelectStub,
        SelectItem: SelectItemStub,
        SelectTrigger: Passthrough,
        SelectContent: Passthrough,
        SelectValue: Passthrough,
        RouterLink: { props: ['to'], template: '<a :href="to"><slot /></a>' },
      },
    },
  })
  await flushPromises()
  return { wrapper, router }
}

beforeEach(() => {
  state.decks = [deck(1, 'Krenko v1'), deck(2, 'Krenko v2'), deck(3, 'Mono blue', 40)]
  state.decksLoaded = true
  state.diff = null
  state.pending = false
  state.fetching = false
  state.loadingError = false
  state.refetchError = false
  state.enabledRef = null
  state.otherIdRef = null
})

describe('DeckCompare', () => {
  it('lists the other decks, never the one on screen, and asks nothing until one is picked', async () => {
    const { wrapper } = await mountAt('/decks/mtg/1')
    const options = wrapper.findAll('option').map((o) => o.text())
    expect(options.some((t) => t.startsWith('Krenko v2'))).toBe(true)
    expect(options.some((t) => t.startsWith('Mono blue'))).toBe(true)
    expect(options.some((t) => t.startsWith('Krenko v1'))).toBe(false)
    expect(state.enabledRef?.value).toBe(false)
    expect(wrapper.text()).toContain('What changed between this deck and another of yours')
  })

  it('writes the pick to ?compare= and reads it back from the URL', async () => {
    state.diff = diff()
    const { wrapper, router } = await mountAt('/decks/mtg/1')
    await wrapper.find('[data-test="picker"]').setValue('2')
    await flushPromises()
    expect(router.currentRoute.value.query.compare).toBe('2')
    expect(state.enabledRef?.value).toBe(true)
    expect(state.otherIdRef?.value).toBe(2)

    // A fresh mount at that URL starts compared.
    const fresh = await mountAt('/decks/mtg/1?compare=3')
    expect(state.otherIdRef?.value).toBe(3)
    expect(state.enabledRef?.value).toBe(true)
    fresh.wrapper.unmount()
  })

  it('ignores a ?compare= that is not another deck id', async () => {
    await mountAt('/decks/mtg/1?compare=1')
    expect(state.enabledRef?.value).toBe(false)
    await mountAt('/decks/mtg/1?compare=abc')
    expect(state.enabledRef?.value).toBe(false)
  })

  it('treats an id the deck list does not offer as no pick — but only once the list has loaded', async () => {
    // A deleted (or never-yours) deck: the picker would show its placeholder while the panel
    // reported "couldn't compare", so the pick is dropped instead.
    const stale = await mountAt('/decks/mtg/1?compare=99')
    expect(state.enabledRef?.value).toBe(false)
    expect(stale.wrapper.text()).not.toContain("Couldn't compare")

    // While the list is still loading, the URL's pick is taken at its word.
    state.decksLoaded = false
    state.diff = diff()
    await mountAt('/decks/mtg/1?compare=99')
    expect(state.enabledRef?.value).toBe(true)
  })

  it('pushes history on the first pick, replaces on a re-pick, and clears via "No comparison"', async () => {
    state.diff = diff()
    const { wrapper, router } = await mountAt('/decks/mtg/1')
    const picker = wrapper.find('[data-test="picker"]')
    expect(picker.findAll('option')[0]!.text()).toBe('No comparison')

    await picker.setValue('2')
    await flushPromises()
    expect(router.currentRoute.value.query.compare).toBe('2')
    await picker.setValue('3')
    await flushPromises()
    expect(router.currentRoute.value.query.compare).toBe('3')

    // One history entry for the comparison as a whole: Back lands on the plain deck page,
    // not on the previous pick.
    router.back()
    await flushPromises()
    expect(router.currentRoute.value.query.compare).toBeUndefined()
    expect(state.enabledRef?.value).toBe(false)

    await picker.setValue('2')
    await flushPromises()
    await picker.setValue('none')
    await flushPromises()
    expect(router.currentRoute.value.query.compare).toBeUndefined()
    expect(state.enabledRef?.value).toBe(false)
  })

  it('renders a section move per section and nets it out whole-deck', async () => {
    const solRing = makeCard('sol', { name: 'Sol Ring' })
    state.diff = diff({
      summary: { added: 0, removed: 0, changed: 0, finish_changed: 0, unchanged: 60 },
      cards: [],
      sections: [
        {
          name: 'Ramp',
          base_section_id: 10,
          other_section_id: 20,
          is_maybeboard: false,
          unchanged: 3,
          entries: [
            {
              card: solRing,
              name: 'Sol Ring',
              change: 'removed',
              base_quantity: 1,
              other_quantity: 0,
              delta: -1,
              base_foil_quantity: 0,
              other_foil_quantity: 0,
            },
          ],
        },
        {
          name: 'Artifacts',
          base_section_id: 11,
          other_section_id: 21,
          is_maybeboard: false,
          unchanged: 0,
          entries: [
            {
              card: solRing,
              name: 'Sol Ring',
              change: 'added',
              base_quantity: 0,
              other_quantity: 1,
              delta: 1,
              base_foil_quantity: 0,
              other_foil_quantity: 0,
            },
          ],
        },
      ],
    })
    const { wrapper } = await mountAt('/decks/mtg/1?compare=2')
    expect(wrapper.text()).toContain('No differences')
    const headings = wrapper.findAll('h3').map((h) => h.text())
    expect(headings).toEqual(['Ramp', 'Artifacts'])
    expect(wrapper.text()).toContain('3 unchanged')
    expect(wrapper.text()).toContain('−1')
    expect(wrapper.text()).toContain('+1')
    expect(wrapper.text()).toContain('1 → 0')

    const buttons = wrapper.findAll('button').filter((b) => b.text() === 'Whole deck')
    await buttons[0]!.trigger('click')
    expect(wrapper.findAll('h3')).toHaveLength(0)
    expect(wrapper.text()).toContain('only which section')
  })

  it('words a finish-only change with its foil counts and links the card', async () => {
    state.diff = diff({
      summary: { added: 0, removed: 0, changed: 0, finish_changed: 1, unchanged: 59 },
      cards: [
        {
          card: makeCard('elves', { name: 'Llanowar Elves' }),
          name: 'Llanowar Elves',
          change: 'finish',
          base_quantity: 4,
          other_quantity: 4,
          delta: 0,
          base_foil_quantity: 2,
          other_foil_quantity: 0,
        },
      ],
      sections: [],
    })
    const { wrapper } = await mountAt('/decks/mtg/1?compare=2')
    expect(wrapper.text()).toContain('1 finish change')
    const whole = wrapper.findAll('button').filter((b) => b.text() === 'Whole deck')
    await whole[0]!.trigger('click')
    expect(wrapper.text()).toContain('Finish changed')
    expect(wrapper.text()).toContain('±0')
    expect(wrapper.text()).toContain('4 → 4 · 2 foil → 0 foil')
    const link = wrapper.find('a[href="/cards/mtg/cards/elves"]')
    expect(link.exists()).toBe(true)
    expect(link.text()).toBe('Llanowar Elves')
  })

  it('flags a maybeboard section', async () => {
    state.diff = diff({
      sections: [
        {
          name: 'Maybeboard',
          base_section_id: null,
          other_section_id: 30,
          is_maybeboard: true,
          unchanged: 0,
          entries: [
            {
              card: makeCard('rhystic', { name: 'Rhystic Study' }),
              name: 'Rhystic Study',
              change: 'added',
              base_quantity: 0,
              other_quantity: 1,
              delta: 1,
              base_foil_quantity: 0,
              other_foil_quantity: 0,
            },
          ],
        },
      ],
    })
    const { wrapper } = await mountAt('/decks/mtg/1?compare=2')
    expect(wrapper.find('h3').text()).toContain('Maybeboard')
    expect(wrapper.text()).toContain('Rhystic Study')
  })

  it('says the decks match when the diff is empty, and reports a failed load', async () => {
    state.diff = diff()
    const { wrapper } = await mountAt('/decks/mtg/1?compare=2')
    expect(wrapper.text()).toContain('same cards in the same counts')

    state.loadingError = true
    state.diff = null
    const failed = await mountAt('/decks/mtg/1?compare=2')
    expect(failed.wrapper.text()).toContain("Couldn't compare these decks.")
  })

  it('points at Duplicate when there is nothing to compare with', async () => {
    state.decks = [deck(1, 'Only deck')]
    const { wrapper } = await mountAt('/decks/mtg/1')
    expect(wrapper.find('[data-test="picker"]').exists()).toBe(false)
    expect(wrapper.text()).toContain('Duplicate this deck')
  })
})
