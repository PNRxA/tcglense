import { describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import type { CardCombo, CardCombos as CardCombosPayload } from '@/lib/api'
import CardCombos from '../CardCombos.vue'

// The card page's "Combos with this card". The list is the server's; what's pinned here is
// that the block stays out of the way when there is nothing to show, that it never lets a
// capped list read as the whole answer, and that it credits Commander Spellbook — which is
// a term of use of the data, not a nicety.

vi.mock('@/composables/useDetailModalLink', () => ({
  useDetailModalLink: () => ({
    hrefFor: (_kind: string, game: string, id: string) => `/cards/${game}/cards/${id}`,
    onActivate: () => {},
    warm: () => {},
  }),
}))

function combo(over: Partial<CardCombo> = {}): CardCombo {
  return {
    id: '245-2034',
    url: 'https://commanderspellbook.com/combo/245-2034',
    identity: ['U'],
    mana_needed: null,
    mana_value_needed: null,
    prerequisites: null,
    description: 'Tap the first, untap it with the second.',
    popularity: 4200,
    bracket_tag: null,
    templates: [],
    produces: ['Infinite colorless mana'],
    pieces: [
      {
        oracle_id: 'o-basalt',
        name: 'Basalt Monolith',
        quantity: 1,
        must_be_commander: false,
        card_id: 'basalt-1',
      },
      {
        oracle_id: 'o-rings',
        name: 'Rings of Brighthearth',
        quantity: 1,
        must_be_commander: false,
        card_id: 'rings-1',
      },
    ],
    ...over,
  }
}

function mountCombos(id: string, payload: Partial<CardCombosPayload>) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  // Seed the cache so the combos are available synchronously (no network in tests).
  queryClient.setQueryData(['card-combos', 'mtg', id], {
    combos: [],
    total: 0,
    source: 'Commander Spellbook',
    source_url: 'https://commanderspellbook.com',
    ...payload,
  } satisfies CardCombosPayload)
  return mount(CardCombos, {
    props: { game: 'mtg', id },
    global: { plugins: [[VueQueryPlugin, { queryClient }]] },
  })
}

describe('CardCombos', () => {
  it('renders nothing when the card is a piece of no combo', () => {
    const wrapper = mountCombos('basalt-1', { combos: [], total: 0 })

    expect(wrapper.find('section').exists()).toBe(false)
    expect(wrapper.text()).not.toContain('Combos with this card')
  })

  it('lists each combo’s pieces and result once expanded, crediting the source', async () => {
    const wrapper = mountCombos('basalt-1', { combos: [combo()], total: 1 })

    // Collapsed by default — a staple is a piece of dozens — but the count and the credit
    // are on the header either way.
    expect(wrapper.text()).toContain('Combos with this card (1)')
    expect(wrapper.text()).toContain('from Commander Spellbook')

    await wrapper.get('button[aria-expanded]').trigger('click')

    expect(wrapper.text()).toContain('Basalt Monolith')
    expect(wrapper.text()).toContain('Rings of Brighthearth')
    expect(wrapper.text()).toContain('Infinite colorless mana')

    // The viewed card is shown but not linked; the other piece is.
    expect(wrapper.find('a[href="/cards/mtg/cards/basalt-1"]').exists()).toBe(false)
    expect(wrapper.find('a[href="/cards/mtg/cards/rings-1"]').exists()).toBe(true)

    // Each combo links back to its page, and the block links to the source itself.
    const external = wrapper.get('a[href="https://commanderspellbook.com/combo/245-2034"]')
    expect(external.attributes('target')).toBe('_blank')
    expect(external.attributes('rel')).toBe('noopener noreferrer')
    const credit = wrapper.get('a[href="https://commanderspellbook.com"]')
    expect(credit.attributes('rel')).toBe('noopener noreferrer')
    expect(wrapper.text()).toContain('Combo data from')
  })

  it('says how many combos the capped list left out', async () => {
    const wrapper = mountCombos('basalt-1', { combos: [combo()], total: 51 })

    expect(wrapper.text()).toContain('Combos with this card (51)')
    await wrapper.get('button[aria-expanded]').trigger('click')

    expect(wrapper.text()).toContain('…and 50 more on Commander Spellbook')
  })
})
