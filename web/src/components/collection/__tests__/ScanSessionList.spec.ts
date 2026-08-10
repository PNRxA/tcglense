import { describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import type { Card } from '@/lib/api'
import type { SessionEntry } from '@/composables/useScanSession'
import ScanSessionList from '../ScanSessionList.vue'

vi.mock('@/composables/useCurrency', () => ({
  useCurrency: () => ({ formatUsd: (amount: string) => `$${amount}` }),
}))

function makeCard(id: string, over: Partial<Card> = {}): Card {
  return {
    id,
    name: `Card ${id}`,
    set_code: 'tst',
    set_name: 'Test Set',
    collector_number: id,
    rarity: 'rare',
    lang: 'en',
    released_at: '2024-01-01',
    mana_cost: null,
    cmc: 0,
    type_line: 'Artifact',
    oracle_text: null,
    power: null,
    toughness: null,
    loyalty: null,
    color_identity: [],
    colors: [],
    layout: 'normal',
    prices: { usd: null, usd_foil: null, eur: null, tix: null },
    has_image: false,
    drop_name: null,
    drop_slug: null,
    secret_lair_bonus: false,
    secret_lair_spend_incentive: false,
    faces: [],
    legalities: null,
    ...over,
  }
}

function makeEntry(id: number, over: Partial<SessionEntry> = {}): SessionEntry {
  return {
    id,
    card: makeCard(String(id)),
    quantity: id,
    foil_quantity: id === 1 ? 1 : 0,
    previous: { quantity: id - 1, foil_quantity: id === 1 ? 1 : 0 },
    source: 'scan',
    ...over,
  }
}

// The list opens the shared detail modal over whatever page it is on, so it needs a real
// router (the scan page has no `:game` path param, which is exactly the case the shared seam
// carries in the query instead).
async function mountList(
  entries: SessionEntry[],
): Promise<{ wrapper: ReturnType<typeof mount>; router: Router }> {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/scan', component: { template: '<div />' } },
      { path: '/cards/:game/cards/:id', component: { template: '<div />' } },
    ],
  })
  await router.push('/scan')
  await router.isReady()
  const wrapper = mount(ScanSessionList, {
    props: { game: 'mtg', entries, disabled: false },
    global: { plugins: [router], stubs: { CardImage: true } },
  })
  return { wrapper, router }
}

describe('ScanSessionList', () => {
  it('keeps long sessions compact and preserves each visible entry index for undo', async () => {
    const { wrapper } = await mountList([1, 2, 3, 4, 5].map((id) => makeEntry(id)))

    expect(wrapper.findAll('li')).toHaveLength(3)
    expect(wrapper.text()).toContain('Now 1 regular')
    expect(wrapper.text()).toContain('View all (5)')
    const disclosure = wrapper.findAll('button').find((button) => button.text() === 'View all (5)')!
    expect(disclosure.attributes('aria-expanded')).toBe('false')
    expect(disclosure.attributes('aria-controls')).toBe(wrapper.get('ul').attributes('id'))

    await disclosure.trigger('click')
    expect(wrapper.findAll('li')).toHaveLength(5)
    expect(disclosure.attributes('aria-expanded')).toBe('true')

    await wrapper.findAll('button[aria-label^="Undo adding"]')[1]!.trigger('click')
    expect(wrapper.emitted('undo')).toEqual([[1]])
  })

  it('opens the card in the shared modal without leaving the scan page', async () => {
    // A scan session is expensive state — a live camera, a tentative match, the log itself.
    // Looking a logged card up must not navigate away from any of it, so the row opens the
    // same in-place modal a browse tile does, keeping the real card page as its href.
    const { wrapper, router } = await mountList([makeEntry(1)])
    const link = wrapper.get('li a')
    expect(link.attributes('href')).toBe('/cards/mtg/cards/1')

    await link.trigger('click')
    await flushPromises()
    expect(router.currentRoute.value.path).toBe('/scan')
    expect(router.currentRoute.value.query.card).toBe('1')
    // No `:game` in the path here, so the modal is told which game in the query.
    expect(router.currentRoute.value.query.game).toBe('mtg')
  })

  it('leaves a modifier-click to the browser so the full card page still opens', async () => {
    const { wrapper, router } = await mountList([makeEntry(1)])
    await wrapper.get('li a').trigger('click', { ctrlKey: true })
    await flushPromises()
    expect(router.currentRoute.value.query.card).toBeUndefined()
  })

  it('names the finish each row added and prices the card for it', async () => {
    // Which number a scan landed on is the session's most-asked question (the foil call is
    // made visually, off a printed star). The row states the change, and prices the copy at
    // the price of the finish it actually added.
    const prices = { usd: '2.00', usd_foil: '20.00', eur: null, tix: null }
    const { wrapper } = await mountList([
      {
        ...makeEntry(1, { card: makeCard('foil-add', { prices }) }),
        quantity: 3,
        foil_quantity: 2,
        previous: { quantity: 3, foil_quantity: 1 },
      },
      {
        ...makeEntry(2, { card: makeCard('regular-add', { prices }) }),
        quantity: 1,
        foil_quantity: 0,
        previous: { quantity: 0, foil_quantity: 0 },
      },
    ])

    const rows = wrapper.findAll('li')
    expect(rows[0]!.text()).toContain('+1 foil')
    expect(rows[0]!.text()).toContain('$20.00')
    expect(rows[0]!.text()).toContain('Now 3 regular · 2 foil')
    expect(rows[1]!.text()).toContain('+1 regular')
    expect(rows[1]!.text()).toContain('$2.00')
    expect(rows[1]!.text()).not.toContain('$20.00')
  })

  it('marks a card added by name, so the history says where each row came from', async () => {
    const { wrapper } = await mountList([
      makeEntry(1, { source: 'manual' }),
      makeEntry(2, { source: 'scan' }),
    ])
    const rows = wrapper.findAll('li')
    expect(rows[0]!.text()).toContain('added by name')
    expect(rows[1]!.text()).not.toContain('added by name')
  })
})
