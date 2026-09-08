import { describe, expect, it, vi } from 'vitest'
import { defineComponent, type Ref } from 'vue'
import { mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { NeededCard, NeededCards } from '@/lib/api'

// Capture the reactive `mode` + `deckId` refs the view passes in, so the toggle and the
// `?deck=` scope can be asserted; and what the mocked query answers.
const captured = vi.hoisted(() => ({
  mode: null as Ref<string> | null,
  deckId: null as Ref<number | null> | null,
  response: null as NeededCards | null,
  toAdd: 0,
  addAll: vi.fn<() => Promise<{ added: number; failed: number }>>(),
}))

vi.mock('@/composables/useCatalog', async () => {
  const { ref: vueRef } = await import('vue')
  return { useGamesQuery: () => ({ data: vueRef({ data: [{ id: 'mtg', name: 'Magic' }] }) }) }
})

vi.mock('@/composables/useDecks', async () => {
  const { ref: vueRef } = await import('vue')
  return {
    useNeededCardsQuery: (_game: unknown, mode: Ref<string>, deckId: Ref<number | null>) => {
      captured.mode = mode
      captured.deckId = deckId
      return {
        data: vueRef(captured.response),
        error: vueRef(null),
        isPending: vueRef(false),
        // The view splits a failure with nothing loaded from one over cached data (issue #622),
        // so the mock carries both of query-core's predicates.
        isLoadingError: vueRef(false),
        isRefetchError: vueRef(false),
      }
    },
  }
})

vi.mock('@/composables/useNeededWishlist', async () => {
  const { ref: vueRef } = await import('vue')
  return {
    useNeededWishlist: () => ({
      ready: vueRef(true),
      toAdd: vueRef(captured.toAdd),
      pending: vueRef(false),
      result: vueRef(null),
      error: vueRef(null),
      addAll: captured.addAll,
    }),
  }
})

vi.mock('@/composables/useCurrency', () => ({
  useCurrency: () => ({
    formatUsd: (raw: string | null | undefined) => (raw == null ? null : `$${raw}`),
  }),
}))

vi.mock('@/stores/auth', () => ({
  useAuthStore: () => ({ sessionResolved: true, isAuthenticated: true }),
}))

vi.mock('@/lib/seo', () => ({ usePageMeta: vi.fn<() => void>() }))

import DeckNeededView from '../DeckNeededView.vue'

const PassThrough = defineComponent({ template: '<div><slot /></div>' })
const CardTileStub = defineComponent({
  props: ['card', 'game'],
  template: '<div class="card-tile">{{ card.name }}<slot name="badge" /></div>',
})

const entry: NeededCard = {
  card: {
    id: 'tower-a',
    name: 'Command Tower',
    set_code: 'cmr',
    collector_number: '350',
  } as NeededCard['card'],
  needed: 1,
  required: 2,
  owned: 1,
  decks: [
    { id: 1, name: 'Deck A' },
    { id: 2, name: 'Deck B' },
  ],
  held_usd: '3.50',
  cheapest_usd: '1.20',
}

function response(over: Partial<NeededCards> = {}): NeededCards {
  return {
    data: [entry],
    deck: null,
    totals: {
      cards: 1,
      copies: 1,
      held_usd: '3.50',
      held_unpriced_cards: 0,
      cheapest_usd: '1.20',
      cheapest_unpriced_cards: 0,
    },
    ...over,
  }
}

async function mountView(path = '/decks/mtg/needed') {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:pathMatch(.*)*', component: PassThrough }],
  })
  await router.push(path)
  await router.isReady()
  return mount(DeckNeededView, {
    props: { game: 'mtg' },
    global: {
      plugins: [router],
      stubs: { CardTile: CardTileStub, LoadingRow: PassThrough },
    },
  })
}

describe('DeckNeededView', () => {
  it('renders each shortfall with counts, prices, and the decks that want it', async () => {
    captured.response = response()
    captured.toAdd = 1
    const wrapper = await mountView()
    const text = wrapper.text()
    // Summary (through the shared wording seam) + per-card counts and prices.
    expect(text).toContain('1 card · ~$3.50 at your printings · from $1.20 at cheapest printings')
    expect(text).toContain('Command Tower')
    expect(text).toContain('need 1')
    expect(text).toContain('want 2 · own 1')
    expect(text).toContain('$3.50 · from $1.20')
    // Game-wide: no scope was asked for.
    expect(captured.deckId?.value).toBeNull()
    expect(text).toContain('Cards needed')
    expect(text).not.toContain('Cards needed for')
    // The affected decks link to each deck.
    const deckLinks = wrapper
      .findAll('a')
      .filter((a) => a.attributes('href')?.includes('/decks/mtg/'))
    const hrefs = deckLinks.map((a) => a.attributes('href'))
    expect(hrefs).toContain('/decks/mtg/1')
    expect(hrefs).toContain('/decks/mtg/2')
    wrapper.unmount()
  })

  it('toggles the matching mode', async () => {
    captured.response = response()
    const wrapper = await mountView()
    expect(captured.mode?.value).toBe('card')
    const printingButton = wrapper
      .findAll('button')
      .find((b) => b.text().trim() === 'Exact printing')
    if (!printingButton) throw new Error('missing Exact printing toggle')
    expect(printingButton.attributes('aria-pressed')).toBe('false')

    await printingButton.trigger('click')
    expect(captured.mode?.value).toBe('printing')
    expect(printingButton.attributes('aria-pressed')).toBe('true')
    wrapper.unmount()
  })

  it('scopes to one deck from ?deck= and names it (issue #675)', async () => {
    captured.response = response({ deck: { id: 7, name: 'Krenko Goblins' } })
    const wrapper = await mountView('/decks/mtg/needed?deck=7')
    expect(captured.deckId?.value).toBe(7)
    const text = wrapper.text()
    expect(text).toContain('Cards needed for Krenko Goblins')
    expect(text).toContain('Back to deck')
    // Back goes to the deck; the escape hatch goes to the game-wide list.
    const hrefs = wrapper.findAll('a').map((a) => a.attributes('href'))
    expect(hrefs).toContain('/decks/mtg/7')
    expect(hrefs).toContain('/decks/mtg/needed')
    wrapper.unmount()
  })

  it('reads a non-numeric ?deck= as the game-wide list', async () => {
    captured.response = response()
    const wrapper = await mountView('/decks/mtg/needed?deck=nope')
    expect(captured.deckId?.value).toBeNull()
    wrapper.unmount()
  })

  it('offers "add all to wish list" for the cards not yet wanted, and disarms once they are', async () => {
    captured.response = response()
    captured.toAdd = 1
    captured.addAll.mockClear()
    const armed = await mountView()
    const button = armed.find('[data-testid="add-all-wishlist"]')
    expect(button.text()).toContain('Add 1 to wish list')
    expect(button.attributes('disabled')).toBeUndefined()
    await button.trigger('click')
    expect(captured.addAll).toHaveBeenCalledOnce()
    armed.unmount()

    captured.toAdd = 0
    const satisfied = await mountView()
    const done = satisfied.find('[data-testid="add-all-wishlist"]')
    expect(done.text()).toContain('On your wish list')
    expect(done.attributes('disabled')).toBeDefined()
    satisfied.unmount()
  })
})
