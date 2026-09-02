import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount, type DOMWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import type { Card, Deck, Game, SearchResults } from '@/lib/api'
import { SEARCH_DEBOUNCE_MS } from '@/composables/useUniversalSearch'
import { SEARCH_GROUP_LIMIT } from '@/lib/universalSearch'
import { useAuthStore } from '@/stores/auth'

// The box is exercised WITH its composable (the engine is where the behaviour lives) over a
// mocked API: what's asserted is the whole contract — a typed term becomes one debounced
// request, the answer renders as labelled groups of real links, the keyboard drives a
// highlight and opens it, Enter hands off to the full card search, and the signed-in user's
// decks join in only when there is a session.
const api = vi.hoisted(() => ({
  searchCatalog: vi.fn<(...args: unknown[]) => Promise<SearchResults>>(),
  getDecks: vi.fn<(...args: unknown[]) => Promise<{ data: Deck[] }>>(),
}))

// Mocked at the module each importer actually reads: the search composable takes
// `searchCatalog` off the barrel (which re-exports `./search`), while `useDecks` imports
// `getDecks` from `@/lib/api/decks` directly.
vi.mock('@/lib/api/search', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/lib/api/search')>()),
  searchCatalog: api.searchCatalog,
}))
vi.mock('@/lib/api/decks', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/lib/api/decks')>()),
  getDecks: api.getDecks,
}))

import UniversalSearchBox from '../UniversalSearchBox.vue'

const MTG: Game = {
  id: 'mtg',
  name: 'Magic: The Gathering',
  publisher: 'WotC',
  data_source: 'Scryfall',
}
const LORCANA: Game = {
  id: 'lorcana',
  name: 'Lorcana',
  publisher: 'Ravensburger',
  data_source: 'x',
}

function card(id: string, name: string): Card {
  return {
    id,
    name,
    set_code: 'lea',
    set_name: 'Alpha',
    collector_number: '1',
    rarity: null,
    lang: 'en',
    released_at: null,
    mana_cost: null,
    cmc: null,
    type_line: 'Instant',
    oracle_text: null,
    power: null,
    toughness: null,
    loyalty: null,
    color_identity: [],
    colors: [],
    layout: null,
    prices: { usd: null, usd_foil: null, usd_etched: null, eur: null, tix: null },
    has_image: true,
    drop_name: null,
    drop_slug: null,
    secret_lair_bonus: false,
    secret_lair_spend_incentive: false,
    faces: [],
    legalities: null,
  } as Card
}

function results(overrides: Partial<SearchResults> = {}): SearchResults {
  return {
    cards: { data: [], has_more: false },
    products: { data: [], has_more: false },
    precons: { data: [], has_more: false },
    keywords: { data: [], has_more: false },
    ...overrides,
  }
}

const FULL = results({
  cards: { data: [card('c1', 'Lightning Bolt'), card('c2', 'Bolt of Lightning')], has_more: false },
  products: {
    data: [
      {
        id: '100',
        name: 'Bolt Bundle',
        set_code: 'blb',
        set_name: 'Bloomburrow',
        product_type: 'bundle',
        url: null,
        has_image: true,
        prices: { usd: null, usd_foil: null },
        msrp: null,
        released_at: null,
      },
    ],
    has_more: true,
  },
  precons: { data: [], has_more: false },
  keywords: {
    data: [
      {
        name: 'Bolster',
        slug: 'bolster',
        kind: 'action',
        text: 'Choose a creature…',
        parameterized: true,
        match_mode: 'anywhere',
      },
    ],
    has_more: false,
  },
})

function deck(id: number, name: string): Deck {
  return {
    id,
    game: 'mtg',
    name,
    description: null,
    format: 'Commander',
    folder_id: null,
    is_public: false,
    card_count: 100,
    color_identity: [],
    commanders: [],
    value_usd: null,
    created_at: '',
    updated_at: '',
  }
}

const CardImageStub = {
  name: 'CardImage',
  props: ['game', 'id', 'name', 'hasImage', 'size'],
  template: '<div class="card-image-stub" :data-id="id" />',
}
const ProductImageStub = {
  name: 'ProductImage',
  props: ['game', 'id', 'name', 'hasImage', 'size'],
  template: '<div class="product-image-stub" :data-id="id" />',
}

let router: Router

/** A promise the test resolves by hand, to hold one read in flight past another. */
function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((r) => {
    resolve = r
  })
  return { promise, resolve }
}

async function mountBox(games: Game[] = [MTG], signedIn = false, { resolved = true } = {}) {
  const pinia = createPinia()
  setActivePinia(pinia)
  const auth = useAuthStore()
  // The router guard resolves the session on the first navigation; a mounted homepage has
  // that latch set unless a test is specifically about the window before it.
  auth.sessionResolved = resolved
  if (signedIn) auth.accessToken = 'token'
  router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', component: { template: '<div />' } },
      { path: '/:pathMatch(.*)*', component: { template: '<div />' } },
    ],
  })
  await router.push('/')
  await router.isReady()
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return mount(UniversalSearchBox, {
    props: { games },
    global: {
      plugins: [pinia, router, [VueQueryPlugin, { queryClient }]],
      stubs: { CardImage: CardImageStub, ProductImage: ProductImageStub },
    },
    attachTo: document.body,
  })
}

/** The visible heading a `role="group"` is labelled by. */
function groupLabel(
  listbox: Pick<DOMWrapper<Element>, 'find'>,
  group: Pick<DOMWrapper<Element>, 'attributes'>,
): string {
  return listbox.find(`[id="${group.attributes('aria-labelledby')}"]`).text()
}

/** Type into the box and let the debounce + request settle. */
async function type(wrapper: Awaited<ReturnType<typeof mountBox>>, text: string) {
  const input = wrapper.get('input[role="combobox"]')
  await input.setValue(text)
  vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS)
  await flushPromises()
  await flushPromises()
  return input
}

beforeEach(() => {
  // Only the debounce timers are faked; vue-query and flushPromises keep the real clock.
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
  api.searchCatalog.mockReset()
  api.getDecks.mockReset()
  api.searchCatalog.mockResolvedValue(FULL)
  api.getDecks.mockResolvedValue({ data: [] })
})

afterEach(() => {
  vi.useRealTimers()
  document.body.innerHTML = ''
})

describe('UniversalSearchBox', () => {
  it('is a combobox that asks nothing until two characters are typed', async () => {
    const wrapper = await mountBox()
    const input = wrapper.get('input[role="combobox"]')
    expect(input.attributes('aria-expanded')).toBe('false')
    expect(input.attributes('placeholder')).toContain('Magic: The Gathering')

    await type(wrapper, 'b')
    expect(api.searchCatalog).not.toHaveBeenCalled()
    expect(wrapper.find('[role="listbox"]').exists()).toBe(false)

    await type(wrapper, 'bo')
    expect(api.searchCatalog).toHaveBeenCalledTimes(1)
    expect(api.searchCatalog.mock.calls[0]?.slice(0, 3)).toEqual(['mtg', 'bo', SEARCH_GROUP_LIMIT])
    expect(input.attributes('aria-expanded')).toBe('true')
    wrapper.unmount()
  })

  it('debounces: one request for a burst of keystrokes', async () => {
    const wrapper = await mountBox()
    const input = wrapper.get('input[role="combobox"]')
    await input.setValue('bo')
    await input.setValue('bol')
    await input.setValue('bolt')
    vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS)
    await flushPromises()
    expect(api.searchCatalog).toHaveBeenCalledTimes(1)
    expect(api.searchCatalog.mock.calls[0]?.[1]).toBe('bolt')
    wrapper.unmount()
  })

  it('renders the answer as labelled groups of links, with thumbnails, plus the closing row', async () => {
    const wrapper = await mountBox()
    await type(wrapper, 'bolt')

    const listbox = wrapper.get('[role="listbox"]')
    const groups = listbox.findAll('[role="group"]')
    const labels = groups.map((g) => groupLabel(listbox, g))
    expect(labels).toEqual(['Cards', 'Sealed products', 'Keywords'])

    const hrefs = listbox.findAll('a[role="option"]').map((a) => a.attributes('href'))
    expect(hrefs).toEqual([
      '/cards/mtg/cards/c1',
      '/cards/mtg/cards/c2',
      '/sealed/mtg/100',
      '/sealed/mtg/products?q=bolt',
      '/keywords/mtg/bolster',
      '/cards/mtg/cards?q=bolt',
    ])
    // Card and product rows draw the image their tile draws; a keyword row an icon.
    expect(listbox.findAll('.card-image-stub').map((s) => s.attributes('data-id'))).toEqual([
      'c1',
      'c2',
    ])
    expect(listbox.findAll('.product-image-stub').map((s) => s.attributes('data-id'))).toEqual([
      '100',
    ])
    expect(listbox.text()).toContain('All sealed products matching “bolt”')
    expect(listbox.text()).toContain('Search all cards for “bolt”')
    // Every row is a real link, so it is a listbox option and a middle-clickable anchor.
    expect(listbox.findAll('[role="option"]').every((o) => o.element.tagName === 'A')).toBe(true)
    wrapper.unmount()
  })

  it('moves a highlight with the arrow keys and opens it with Enter', async () => {
    const wrapper = await mountBox()
    const input = await type(wrapper, 'bolt')
    expect(input.attributes('aria-activedescendant')).toBeUndefined()

    await input.trigger('keydown', { key: 'ArrowDown' })
    await input.trigger('keydown', { key: 'ArrowDown' })
    const options = wrapper.findAll('[role="option"]')
    expect(options[1]?.attributes('aria-selected')).toBe('true')
    expect(options[0]?.attributes('aria-selected')).toBe('false')
    expect(input.attributes('aria-activedescendant')).toBe(options[1]?.attributes('id'))

    await input.trigger('keydown', { key: 'ArrowUp' })
    expect(options[0]?.attributes('aria-selected')).toBe('true')

    await input.trigger('keydown', { key: 'Enter' })
    await flushPromises()
    expect(router.currentRoute.value.fullPath).toBe('/cards/mtg/cards/c1')
    expect(wrapper.find('[role="listbox"]').exists()).toBe(false)
    wrapper.unmount()
  })

  it('hands Enter with nothing highlighted to the full card search, with the live text', async () => {
    const wrapper = await mountBox()
    const input = await type(wrapper, 'bolt')
    // A trailing keystroke the debounce hasn't caught up with still travels.
    await input.setValue('bolts')
    await input.trigger('keydown', { key: 'Enter' })
    await flushPromises()
    expect(router.currentRoute.value.fullPath).toBe('/cards/mtg/cards?q=bolts')
    wrapper.unmount()
  })

  it('closes on Escape and reopens on focus', async () => {
    const wrapper = await mountBox()
    const input = await type(wrapper, 'bolt')
    expect(wrapper.find('[role="listbox"]').exists()).toBe(true)
    await input.trigger('keydown', { key: 'Escape' })
    expect(wrapper.find('[role="listbox"]').exists()).toBe(false)
    await input.trigger('focus')
    expect(wrapper.find('[role="listbox"]').exists()).toBe(true)
    wrapper.unmount()
  })

  it('says when nothing matched, and when the search is unavailable', async () => {
    api.searchCatalog.mockResolvedValue(results())
    const wrapper = await mountBox()
    await type(wrapper, 'zzzz')
    const status = wrapper.get('[role="status"]')
    expect(status.text()).toContain('No cards, sealed products, decks, or keywords match “zzzz”')
    // The closing row is still there, so the full grammar is one Enter away.
    expect(wrapper.get('[role="listbox"]').text()).toContain('Search all cards for “zzzz”')
    wrapper.unmount()

    api.searchCatalog.mockRejectedValue(new Error('boom'))
    const failing = await mountBox()
    await type(failing, 'zzzz')
    await flushPromises()
    expect(failing.get('[role="status"]').text()).toContain('Search is unavailable right now')
    failing.unmount()
  })

  it('never fetches decks signed out, and lists matching decks second when signed in', async () => {
    const wrapper = await mountBox([MTG], false)
    await type(wrapper, 'bolt')
    expect(api.getDecks).not.toHaveBeenCalled()
    expect(wrapper.get('[role="listbox"]').text()).not.toContain('Your decks')
    wrapper.unmount()

    api.getDecks.mockResolvedValue({ data: [deck(7, 'Bolt Storm'), deck(8, 'Elves')] })
    const signedIn = await mountBox([MTG], true)
    await type(signedIn, 'bolt')
    await flushPromises()
    expect(api.getDecks).toHaveBeenCalledTimes(1)
    const listbox = signedIn.get('[role="listbox"]')
    const groups = listbox.findAll('[role="group"]')
    const labels = groups.map((g) => groupLabel(listbox, g))
    expect(labels).toEqual(['Cards', 'Your decks', 'Sealed products', 'Keywords'])
    const mine = groups[1]
    expect(mine?.findAll('[role="option"]').map((o) => o.attributes('href'))).toEqual([
      '/decks/mtg/7',
    ])
    expect(mine?.text()).not.toContain('Elves')
    signedIn.unmount()
  })

  it('keeps the previous rows up while the next term loads, labelled with the term they answer', async () => {
    const wrapper = await mountBox()
    const input = await type(wrapper, 'bolt')
    const next = deferred<SearchResults>()
    api.searchCatalog.mockImplementation(() => next.promise)
    await input.setValue('island')
    vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS)
    await flushPromises()

    const listbox = wrapper.get('[role="listbox"]')
    // The bolt rows are still on screen, and their "see all" row still says bolt.
    expect(listbox.text()).toContain('Lightning Bolt')
    expect(listbox.text()).toContain('All sealed products matching “bolt”')
    expect(listbox.text()).not.toContain('matching “island”')
    // The closing row is the hand-off for what was typed, so it does say island…
    expect(listbox.text()).toContain('Search all cards for “island”')
    // …and nothing claims that island matched nothing while it is still loading.
    expect(wrapper.find('[role="status"]').exists()).toBe(false)

    next.resolve(results())
    await flushPromises()
    await flushPromises()
    expect(wrapper.get('[role="status"]').text()).toContain('match “island”')
    expect(listbox.text()).not.toContain('Lightning Bolt')
    wrapper.unmount()
  })

  it('does not say "no matches" for a new term while it is still loading', async () => {
    api.searchCatalog.mockResolvedValue(results())
    const wrapper = await mountBox()
    const input = await type(wrapper, 'zzzz')
    expect(wrapper.get('[role="status"]').text()).toContain('match “zzzz”')

    // A cached empty answer must not stand in for the next term's verdict.
    const next = deferred<SearchResults>()
    api.searchCatalog.mockImplementation(() => next.promise)
    await input.setValue('bolt')
    vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS)
    await flushPromises()
    expect(wrapper.get('[role="status"]').text()).toContain('Searching')
    expect(wrapper.get('[role="listbox"]').text()).not.toContain('match “bolt”')

    next.resolve(FULL)
    await flushPromises()
    await flushPromises()
    expect(wrapper.get('[role="listbox"]').text()).toContain('Lightning Bolt')
    wrapper.unmount()
  })

  it('holds the "no matches" verdict until the session has resolved', async () => {
    api.searchCatalog.mockResolvedValue(results())
    const wrapper = await mountBox([MTG], false, { resolved: false })
    await type(wrapper, 'zzzz')
    // Signed-in or not is unknown, so whether "your decks" match is unknown too.
    expect(wrapper.get('[role="status"]').text()).toContain('Searching')

    useAuthStore().sessionResolved = true
    await flushPromises()
    expect(wrapper.get('[role="status"]').text()).toContain('match “zzzz”')
    wrapper.unmount()
  })

  it('keeps the keyboard highlight when the deck list lands after the catalog answer', async () => {
    const decks = deferred<{ data: Deck[] }>()
    api.getDecks.mockImplementation(() => decks.promise)
    const wrapper = await mountBox([MTG], true)
    const input = await type(wrapper, 'bolt')
    // Three rows down: the product, after the two cards.
    for (let i = 0; i < 3; i += 1) await input.trigger('keydown', { key: 'ArrowDown' })
    const before = wrapper.findAll('[role="option"]')
    expect(before[2]?.text()).toContain('Bolt Bundle')
    const productId = before[2]?.attributes('id')
    expect(input.attributes('aria-activedescendant')).toBe(productId)

    decks.resolve({ data: [deck(7, 'Bolt Storm')] })
    await flushPromises()
    await flushPromises()
    const after = wrapper.findAll('[role="option"]')
    // The deck row slid in above the product, and the highlight followed the product.
    expect(after[2]?.text()).toContain('Bolt Storm')
    expect(after[3]?.attributes('id')).toBe(productId)
    expect(after[3]?.attributes('aria-selected')).toBe('true')
    expect(input.attributes('aria-activedescendant')).toBe(productId)

    await input.trigger('keydown', { key: 'Enter' })
    await flushPromises()
    expect(router.currentRoute.value.fullPath).toBe('/sealed/mtg/100')
    wrapper.unmount()
  })

  it('ignores Enter and the arrows while an IME is composing', async () => {
    const wrapper = await mountBox()
    const input = await type(wrapper, 'bolt')
    await input.trigger('keydown', { key: 'ArrowDown', isComposing: true })
    expect(input.attributes('aria-activedescendant')).toBeUndefined()
    await input.trigger('keydown', { key: 'Enter', isComposing: true })
    await flushPromises()
    expect(router.currentRoute.value.fullPath).toBe('/')
    expect(wrapper.find('[role="listbox"]').exists()).toBe(true)
    wrapper.unmount()
  })

  it('asks nothing — not even for decks — until the registry names a game', async () => {
    const wrapper = await mountBox([], true)
    await type(wrapper, 'bolt')
    expect(api.searchCatalog).not.toHaveBeenCalled()
    expect(api.getDecks).not.toHaveBeenCalled()
    expect(wrapper.get('[role="status"]').text()).toContain('Searching')

    await wrapper.setProps({ games: [MTG] })
    await flushPromises()
    await flushPromises()
    expect(api.searchCatalog).toHaveBeenCalledTimes(1)
    expect(api.getDecks).toHaveBeenCalledTimes(1)
    wrapper.unmount()
  })

  it('shows a game picker only when the registry has more than one game', async () => {
    const one = await mountBox([MTG])
    expect(one.find('[data-slot="select-trigger"]').exists()).toBe(false)
    one.unmount()

    const two = await mountBox([MTG, LORCANA])
    expect(two.find('[data-slot="select-trigger"]').exists()).toBe(true)
    await type(two, 'bolt')
    // The first game is searched until another is picked.
    expect(api.searchCatalog.mock.calls[0]?.[0]).toBe('mtg')
    two.unmount()
  })
})
