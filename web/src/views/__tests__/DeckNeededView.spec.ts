import { describe, expect, it, vi } from 'vitest'
import { defineComponent, type Ref } from 'vue'
import { mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { NeededCard } from '@/lib/api'

// Capture the reactive `mode` ref the view passes in, so the toggle can be asserted.
const captured = vi.hoisted(() => ({ mode: null as Ref<string> | null }))

vi.mock('@/composables/useCatalog', async () => {
  const { ref: vueRef } = await import('vue')
  return { useGamesQuery: () => ({ data: vueRef({ data: [{ id: 'mtg', name: 'Magic' }] }) }) }
})

vi.mock('@/composables/useDecks', async () => {
  const { ref: vueRef } = await import('vue')
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
  }
  return {
    useNeededCardsQuery: (_game: unknown, mode: Ref<string>) => {
      captured.mode = mode
      return {
        data: vueRef({ data: [entry] }),
        isPending: vueRef(false),
        isError: vueRef(false),
      }
    },
  }
})

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

function mountView() {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:pathMatch(.*)*', component: PassThrough }],
  })
  return mount(DeckNeededView, {
    props: { game: 'mtg' },
    global: {
      plugins: [router],
      stubs: { CardTile: CardTileStub, LoadingRow: PassThrough },
    },
  })
}

describe('DeckNeededView', () => {
  it('renders each shortfall with counts and the decks that want it', () => {
    const wrapper = mountView()
    const text = wrapper.text()
    // Summary + per-card counts.
    expect(text).toContain('1 card · 1 copy to acquire')
    expect(text).toContain('Command Tower')
    expect(text).toContain('need 1')
    expect(text).toContain('want 2 · own 1')
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
    const wrapper = mountView()
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
})
