import { beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import DeckSuggestions from '@/components/decks/DeckSuggestions.vue'
import type {
  DeckCardEntry,
  DeckSection,
  DeckSuggestionCard,
  DeckSuggestions as DeckSuggestionsPayload,
} from '@/lib/api'
import { TOP_SUGGESTED } from '@/lib/deckSuggestions'
import { makeCard } from '@/test/fixtures'

// Every card, filter and role is the server's (issue #684), so what's under test is that
// the panel keeps the response's honesty — it names the filters the server applied (and says
// when none was), keeps the "global popularity, not synergy" caveat in view even collapsed,
// and never words a fit it wasn't told — and that "Add" files a card through the deck's
// existing card write exactly as the add-cards box would: the automatic section by type, one
// more than the deck holds, a pinned section when the owner picks one, and no write at all for
// a type with no safe bucket.

const query = vi.hoisted(() => ({
  pending: false,
  fetching: false,
  loadingError: false,
  refetchError: false,
  data: null as DeckSuggestionsPayload | null,
  calls: 0,
}))

const mocks = vi.hoisted(() => ({
  setCard: vi.fn<(vars: unknown) => Promise<unknown>>(),
}))

vi.mock('@/composables/useDeckAnalysis', async () => {
  const { computed } = await import('vue')
  return {
    useDeckSuggestionsQuery: () => {
      query.calls += 1
      return {
        data: computed(() => (query.pending ? undefined : (query.data ?? undefined))),
        isPending: computed(() => query.pending),
        isFetching: computed(() => query.fetching || query.pending),
        isLoadingError: computed(() => query.loadingError),
        isRefetchError: computed(() => query.refetchError),
      }
    },
  }
})

vi.mock('@/composables/useDecks', async () => {
  const { ref } = await import('vue')
  return {
    useSetDeckCardMutation: () => ({ mutateAsync: mocks.setCard, isPending: ref(false) }),
  }
})

vi.mock('@/composables/useDetailModalLink', () => ({
  useDetailModalLink: () => ({
    hrefFor: (_kind: string, game: string, id: string) => `/cards/${game}/cards/${id}`,
    onActivate: () => {},
    warm: () => {},
  }),
}))

const PassThrough = defineComponent({ template: '<div><slot /></div>' })
const ButtonStub = defineComponent({
  inheritAttrs: false,
  template: '<button v-bind="$attrs"><slot /></button>',
})

const SECTIONS: DeckSection[] = [
  { id: 1, name: 'Commander', position: 0, is_maybeboard: false },
  { id: 2, name: 'Creatures', position: 1, is_maybeboard: false },
  { id: 3, name: 'Instants', position: 2, is_maybeboard: false },
  { id: 9, name: 'Maybeboard', position: 3, is_maybeboard: true },
]

function suggestion(
  id: string,
  over: Partial<DeckSuggestionCard> & { type_line?: string } = {},
): DeckSuggestionCard {
  const { type_line, ...rest } = over
  return {
    card: makeCard(id, {
      name: `Card ${id}`,
      type_line: type_line ?? 'Creature — Bear',
      set_code: 'one',
      collector_number: id,
    }),
    edhrec_rank: 100,
    owned: 1,
    roles: [],
    ...rest,
  }
}

function payload(top: DeckSuggestionCard[], over: Partial<DeckSuggestionsPayload> = {}) {
  const role = (
    key: DeckSuggestionsPayload['roles'][number]['role'],
    label: string,
    inDeck: number,
    cards: DeckSuggestionCard[],
    count = cards.length,
  ) => ({ role: key, label, description: `${label} does things.`, in_deck: inDeck, count, cards })
  return {
    format_key: 'commander',
    format_label: 'Commander',
    color_identity: ['W', 'U'],
    commanders: [{ card_id: 'cmd', name: 'Aminatou' }],
    candidate_count: top.length,
    scanned_count: top.length,
    top,
    roles: [
      role('ramp', 'Ramp', 8, []),
      role(
        'card_draw',
        'Card draw',
        6,
        top.filter((c) => c.roles.includes('card_draw')),
      ),
      role('removal', 'Removal', 2, [], 0),
      role('board_wipe', 'Board wipes', 0, []),
      role('counterspell', 'Counterspells', 0, []),
      role('tutor', 'Tutors', 0, []),
      role('recursion', 'Recursion', 0, []),
      role('protection', 'Protection', 0, []),
    ],
    unclassified_count: top.filter((c) => c.roles.length === 0).length,
    caveats: [
      'Ranked by EDHREC’s global popularity, not by synergy with this deck’s commander.',
      'Only cards the catalog marks legal in the deck’s format are listed.',
    ],
    ...over,
  }
}

function mountPanel(cards: DeckCardEntry[] = []) {
  return mount(DeckSuggestions, {
    props: { game: 'mtg', deckId: 3, sections: SECTIONS, cards },
    global: {
      stubs: {
        Button: ButtonStub,
        Card: PassThrough,
        CardContent: PassThrough,
        CardHeader: PassThrough,
        CardTitle: PassThrough,
        StaleNotice: defineComponent({ props: ['label'], template: '<p>{{ label }}</p>' }),
        UpdatingCue: defineComponent({ props: ['label'], template: '<span>{{ label }}</span>' }),
        Skeleton: defineComponent({ template: '<div data-testid="skeleton" />' }),
      },
    },
  })
}

async function expand(wrapper: ReturnType<typeof mountPanel>) {
  await wrapper.get('button[aria-expanded]').trigger('click')
}

beforeEach(() => {
  query.pending = false
  query.fetching = false
  query.loadingError = false
  query.refetchError = false
  query.data = null
  query.calls = 0
  mocks.setCard.mockReset().mockResolvedValue({ quantity: 1, foil_quantity: 0 })
})

describe('DeckSuggestions', () => {
  it('rests on the most popular rows with the filters named, and opens to the role groups', async () => {
    query.data = payload(
      [
        suggestion('a', { edhrec_rank: 42, owned: 3, roles: ['card_draw'] }),
        suggestion('b', { edhrec_rank: 87 }),
        suggestion('c', { edhrec_rank: 1204, roles: ['card_draw'] }),
      ],
      { candidate_count: 14, scanned_count: 14 },
    )
    const wrapper = mountPanel()
    expect(query.calls).toBe(1)

    const text = wrapper.text()
    expect(text).toContain("14 cards you own fit · in Aminatou's colours (WU) · legal in Commander")
    const rows = wrapper.findAll('[data-testid="suggestion-row"]')
    expect(rows).toHaveLength(3)
    expect(rows[0]!.get('[data-testid="rank"]').text()).toBe('#42')
    expect(rows[0]!.get('[data-testid="owned"]').text()).toContain('×3 owned')
    expect(rows[0]!.text()).toContain('Card draw')
    expect(rows[1]!.text()).not.toContain('Card draw')
    // The rank caveat is in view while collapsed; the role groups are not.
    expect(wrapper.get('[data-testid="rank-caveat"]').text()).toContain('global popularity')
    expect(wrapper.findAll('[data-testid="role-group"]')).toHaveLength(0)

    await expand(wrapper)
    const groups = wrapper.findAll('[data-testid="role-group"]')
    expect(groups).toHaveLength(8)
    const draw = groups.find((g) => g.text().startsWith('Card draw'))!
    expect(draw.text()).toContain('in deck 6 · you own 2 more')
    expect(draw.text()).toContain('Card a')
    expect(draw.text()).toContain('Card c')
    expect(wrapper.text()).toContain('Only cards the catalog marks legal')
  })

  it('caps the resting rows and says when a group holds more than it lists', async () => {
    const many = Array.from({ length: TOP_SUGGESTED + 4 }, (_, i) =>
      suggestion(`c${i}`, { edhrec_rank: i + 1, roles: ['ramp'] }),
    )
    query.data = payload(many, {
      roles: payload(many).roles.map((g) =>
        g.role === 'ramp' ? { ...g, cards: many.slice(0, 2), count: many.length } : g,
      ),
    })
    const wrapper = mountPanel()
    expect(wrapper.findAll('[data-testid="suggestion-row"]')).toHaveLength(TOP_SUGGESTED)
    await expand(wrapper)
    const ramp = wrapper
      .findAll('[data-testid="role-group"]')
      .find((g) => g.text().startsWith('Ramp'))!
    expect(ramp.text()).toContain(`…and ${many.length - 2} more`)
  })

  it('says when the server applied no filter rather than implying a fit', () => {
    query.data = payload([suggestion('a')], {
      format_key: null,
      format_label: null,
      color_identity: null,
      commanders: [],
    })
    const wrapper = mountPanel()
    const text = wrapper.text()
    expect(text).toContain('any colour')
    expect(text).toContain('any format')
    expect(text).not.toContain("'s colours")
  })

  it('adds a card through the deck card write, filed by type, one more than the deck holds', async () => {
    query.data = payload([
      suggestion('bear', { type_line: 'Creature — Bear' }),
      suggestion('bolt', { type_line: 'Instant' }),
    ])
    const held: DeckCardEntry[] = [
      {
        card: makeCard('bear', { type_line: 'Creature — Bear' }),
        section_id: 2,
        quantity: 1,
        foil_quantity: 1,
      },
    ]
    const wrapper = mountPanel(held)
    const rows = wrapper.findAll('[data-testid="suggestion-row"]')
    expect(rows[0]!.get('[data-testid="owned"]').text()).toContain('2 in section')

    await rows[0]!.get('button[aria-label^="Add"]').trigger('click')
    await flushPromises()
    expect(mocks.setCard).toHaveBeenCalledWith({
      game: 'mtg',
      deckId: 3,
      sectionId: 2,
      id: 'bear',
      quantity: 2,
      foil_quantity: 1,
    })

    await rows[1]!.get('button[aria-label^="Add"]').trigger('click')
    await flushPromises()
    expect(mocks.setCard).toHaveBeenLastCalledWith(
      expect.objectContaining({ id: 'bolt', sectionId: 3, quantity: 1, foil_quantity: 0 }),
    )
  })

  it('files into a pinned section, and refuses a type with no safe automatic bucket', async () => {
    query.data = payload([
      suggestion('battle', { type_line: 'Battle — Siege' }),
      suggestion('bear', { type_line: 'Creature — Bear' }),
    ])
    const wrapper = mountPanel()
    const rows = wrapper.findAll('[data-testid="suggestion-row"]')
    const battleAdd = rows[0]!.get('button[aria-label^="Choose a section"]')
    expect(battleAdd.attributes('disabled')).toBeDefined()

    await wrapper.get('select').setValue('9')
    await rows[0]!.get('button[aria-label^="Add"]').trigger('click')
    await flushPromises()
    expect(mocks.setCard).toHaveBeenCalledWith(
      expect.objectContaining({ id: 'battle', sectionId: 9, quantity: 1 }),
    )
    // A failed write is reported, not swallowed.
    mocks.setCard.mockRejectedValueOnce(new Error('boom'))
    await rows[1]!.get('button[aria-label^="Add"]').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain("Couldn't add Card bear")
  })

  it('tells the truth about an empty answer, a failed load and a stale refetch', async () => {
    query.data = payload([])
    let wrapper = mountPanel()
    expect(wrapper.text()).toContain('Nothing you own fits this deck yet')

    query.loadingError = true
    query.data = null
    wrapper = mountPanel()
    expect(wrapper.text()).toContain("Couldn't read your collection")

    query.loadingError = false
    query.refetchError = true
    query.data = payload([suggestion('a')])
    wrapper = mountPanel()
    expect(wrapper.text()).toContain("Couldn't refresh")
    expect(wrapper.findAll('[data-testid="suggestion-row"]')).toHaveLength(1)
  })
})
