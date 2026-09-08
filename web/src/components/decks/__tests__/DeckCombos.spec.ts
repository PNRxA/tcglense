import { beforeEach, describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import DeckCombos from '@/components/decks/DeckCombos.vue'
import type { DeckCombo, DeckCombos as DeckCombosPayload } from '@/lib/api'

// The combos themselves are the server's (and, one level up, Commander Spellbook's), so
// what's under test here is what the panel is allowed to *say* about them: it never turns
// "no combo data has been synced" into "this deck has no combos", it never reports a
// wildcard template as a requirement already met, and it always names the source the
// data's terms of use ask it to name.

const query = vi.hoisted(() => ({
  pending: false,
  fetching: false,
  loadingError: false,
  refetchError: false,
  data: null as DeckCombosPayload | null,
  authedCalls: 0,
  publicCalls: 0,
  preconCalls: 0,
}))

vi.mock('@/composables/useDeckAnalysis', async () => {
  const { computed } = await import('vue')
  const result = () => ({
    data: computed(() => (query.pending ? undefined : (query.data ?? undefined))),
    isPending: computed(() => query.pending),
    isFetching: computed(() => query.fetching || query.pending),
    isLoadingError: computed(() => query.loadingError),
    isRefetchError: computed(() => query.refetchError),
  })
  return {
    useDeckCombosQuery: () => {
      query.authedCalls += 1
      return result()
    },
    usePublicDeckCombosQuery: () => {
      query.publicCalls += 1
      return result()
    },
    usePreconCombosQuery: () => {
      query.preconCalls += 1
      return result()
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

function combo(over: Partial<DeckCombo> = {}): DeckCombo {
  return {
    id: '245-2034',
    url: 'https://commanderspellbook.com/combo/245-2034',
    identity: ['U'],
    mana_needed: '{2}',
    mana_value_needed: 2,
    prerequisites: 'Both on the battlefield.',
    description: 'Tap the first.\nUntap it with the second.',
    popularity: 4200,
    bracket_tag: null,
    templates: [],
    produces: ['Infinite card draw'],
    pieces: [
      {
        oracle_id: 'o-basalt',
        name: 'Basalt Monolith',
        quantity: 1,
        must_be_commander: false,
        card_id: 'basalt-1',
        in_deck: true,
      },
      {
        oracle_id: 'o-rings',
        name: 'Rings of Brighthearth',
        quantity: 1,
        must_be_commander: false,
        card_id: 'rings-1',
        in_deck: true,
      },
    ],
    missing: [],
    ...over,
  }
}

function makePayload(over: Partial<DeckCombosPayload> = {}): DeckCombosPayload {
  return {
    combos: [combo()],
    combo_count: 1,
    almost: [],
    almost_count: 0,
    available: true,
    source: 'Commander Spellbook',
    source_url: 'https://commanderspellbook.com',
    ...over,
  }
}

beforeEach(() => {
  query.pending = false
  query.fetching = false
  query.loadingError = false
  query.refetchError = false
  query.data = makePayload()
  query.authedCalls = 0
  query.publicCalls = 0
  query.preconCalls = 0
})

function mountPanel(props: Record<string, unknown> = {}) {
  return mount(DeckCombos, {
    props: { game: 'mtg', deckId: 7, ...props },
    // ManaSymbols renders for real: it needs no QueryClient without `keywords`, and
    // stubbing it would hide the combo's own description, which is passed as a prop.
    global: { stubs: { RouterLink: { template: '<a><slot /></a>' } } },
  })
}

/** The panel mounts collapsed; most of the suite reads what is behind "Details". */
async function mountOpen(props: Record<string, unknown> = {}) {
  const wrapper = mountPanel(props)
  await wrapper.get('button[aria-label="Details for combos"]').trigger('click')
  return wrapper
}

describe('DeckCombos', () => {
  it('is collapsed by default, with the answer and the credit in the header', async () => {
    const wrapper = mountPanel()

    // The one-line answer and the source are readable without opening anything.
    expect(wrapper.text()).toContain('This deck can assemble 1 combo (Commander Spellbook).')
    expect(wrapper.text()).not.toContain('In this deck')
    expect(wrapper.text()).not.toContain('Basalt Monolith')
    const toggle = wrapper.get('button[aria-label="Details for combos"]')
    expect(toggle.attributes('aria-expanded')).toBe('false')

    await toggle.trigger('click')
    expect(toggle.attributes('aria-expanded')).toBe('true')
    expect(wrapper.text()).toContain('Basalt Monolith')
  })

  it('lists an assembled combo by its pieces and what it makes', async () => {
    const wrapper = await mountOpen()

    expect(wrapper.text()).toContain('Combos')
    expect(wrapper.text()).toContain('In this deck')
    expect(wrapper.text()).toContain('Basalt Monolith')
    expect(wrapper.text()).toContain('Rings of Brighthearth')
    expect(wrapper.text()).toContain('Infinite card draw')
    // Counted from the exact totals, not the capped list.
    expect(wrapper.text()).toContain('This deck can assemble 1 combo')
    // Each piece links to a printing, and the combo links back to its source page.
    expect(wrapper.find('a[href="/cards/mtg/cards/basalt-1"]').exists()).toBe(true)
    expect(
      wrapper.find('a[href="https://commanderspellbook.com/combo/245-2034"]').attributes('rel'),
    ).toBe('noopener noreferrer')
  })

  it('shows the steps only once the disclosure is opened', async () => {
    const wrapper = await mountOpen()
    expect(wrapper.text()).not.toContain('Both on the battlefield.')

    await wrapper
      .get('button[aria-expanded]:not([aria-label="Details for combos"])')
      .trigger('click')

    expect(wrapper.text()).toContain('Both on the battlefield.')
    expect(wrapper.text()).toContain('Untap it with the second.')
  })

  it('says what a one-card-short combo still needs', async () => {
    query.data = makePayload({
      combos: [],
      combo_count: 0,
      almost: [
        combo({
          id: '900-1',
          pieces: [
            {
              oracle_id: 'o-basalt',
              name: 'Basalt Monolith',
              quantity: 1,
              must_be_commander: false,
              card_id: 'basalt-1',
              in_deck: true,
            },
            {
              oracle_id: 'o-dockside',
              name: 'Dockside Extortionist',
              quantity: 1,
              must_be_commander: false,
              card_id: 'dockside-1',
              in_deck: false,
            },
          ],
          missing: [
            { name: 'Dockside Extortionist', kind: 'card', card_id: 'dockside-1' },
            { name: 'A free sacrifice outlet', kind: 'template', card_id: null },
          ],
        }),
      ],
      almost_count: 1,
    })

    const text = (await mountOpen()).text()

    expect(text).toContain('One card short')
    expect(text).toContain('This deck is one card short of 1 combo')
    expect(text).toContain('Needs:')
    expect(text).toContain('Dockside Extortionist')
    // A template is a wildcard the app can't evaluate, so it is stated as a further
    // requirement — never as one already met.
    expect(text).toContain('…also needs: A free sacrifice outlet')
    expect(text).not.toMatch(/you (already )?have it/i)
  })

  it('names a piece that has to be the commander as such', async () => {
    query.data = makePayload({
      combos: [],
      combo_count: 0,
      almost: [
        combo({
          id: '900-2',
          missing: [{ name: 'Basalt Monolith', kind: 'commander', card_id: 'basalt-1' }],
        }),
      ],
      almost_count: 1,
    })

    expect((await mountOpen()).text()).toContain('in the command zone')
  })

  it('says how many combos a capped list left out', async () => {
    query.data = makePayload({ combo_count: 137, almost: [combo({ id: 'a-1' })], almost_count: 9 })
    const text = (await mountOpen()).text()

    expect(text).toContain('…and 136 more')
    expect(text).toContain('…and 8 more')
    expect(text).toContain(
      'This deck can assemble 137 combos and is one card short of 9 more (Commander Spellbook).',
    )
  })

  it('reports unsynced combo data as unknown, never as "no combos"', async () => {
    query.data = makePayload({ combos: [], combo_count: 0, almost: [], almost_count: 0 })
    query.data.available = false

    const text = (await mountOpen()).text()

    expect(text).toContain("Combo data hasn't been synced yet")
    // The one wording that would be a confident wrong answer.
    expect(text).not.toContain('None')
  })

  it('says a deck has no combos only when the data is there to say it', async () => {
    query.data = makePayload({ combos: [], combo_count: 0, almost: [], almost_count: 0 })

    const text = (await mountOpen()).text()

    expect(text).toContain("None of Commander Spellbook's combos are in this deck")
    expect(text).not.toContain("hasn't been synced")
  })

  it('credits the source with a link, whatever it is showing', async () => {
    const link = (await mountOpen()).get('a[href="https://commanderspellbook.com"]')
    expect(link.text()).toContain('Commander Spellbook')
    expect(link.attributes('target')).toBe('_blank')
    expect(link.attributes('rel')).toBe('noopener noreferrer')

    query.data = makePayload({ combos: [], combo_count: 0 })
    query.data.available = false
    expect((await mountOpen()).find('a[href="https://commanderspellbook.com"]').exists()).toBe(true)
  })

  it('keeps the list on screen when a background refetch fails', async () => {
    // query-core flips `status` to 'error' on ANY failed fetch while keeping `data`, so
    // gating the destructive branch on bare `isError` would swap a good list for "couldn't
    // work out" the first time a refetch hiccups (issue #622).
    query.refetchError = true
    const wrapper = await mountOpen()

    expect(wrapper.text()).toContain('Basalt Monolith')
    expect(wrapper.text()).not.toContain("Couldn't work out")
    expect(wrapper.text()).toContain("Couldn't refresh")
  })

  it('says it couldn’t work them out only when nothing ever loaded', async () => {
    query.loadingError = true
    query.data = null
    const text = (await mountOpen()).text()

    expect(text).toContain("Couldn't work out")
    expect(text).not.toContain('None of')
  })

  it('picks its query by the addressing mode it was mounted with', () => {
    mountPanel()
    expect(query.authedCalls).toBe(1)
    expect(query.publicCalls).toBe(0)

    mountPanel({ handle: 'alice-0001' })
    expect(query.publicCalls).toBe(1)

    mountPanel({ deckId: undefined, preconSlug: 'turtle-power-tmc' })
    expect(query.preconCalls).toBe(1)
  })
})
