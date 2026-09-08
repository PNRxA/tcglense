import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { CardSet, PreconDeck, Product, ReleaseCalendar } from '@/lib/api'
import { RELEASE_HEADS_UP_PATH } from '@/lib/releases'

// The month sections, the entries inside them and the links they carry are what the page is
// for; the query is mocked at the composable seam so the test pins the rendering, not fetch.
const state = vi.hoisted(() => ({
  calendar: undefined as ReleaseCalendar | undefined,
  isPending: false,
  isError: false,
}))

vi.mock('@/composables/useCatalog', async () => {
  const { ref: vueRef } = await import('vue')
  return {
    useGamesQuery: () => ({ data: vueRef({ data: [{ id: 'mtg', name: 'Magic' }] }) }),
    useGameName: () => vueRef('Magic'),
  }
})

vi.mock('@/composables/useReleases', async () => {
  const { ref: vueRef } = await import('vue')
  return {
    useReleaseCalendarQuery: () => ({
      data: vueRef(state.calendar),
      isPending: vueRef(state.isPending),
      isError: vueRef(state.isError),
    }),
  }
})

vi.mock('@/lib/seo', () => ({ usePageMeta: vi.fn<() => void>() }))

import ReleaseCalendarView from '@/views/ReleaseCalendarView.vue'

function calendarSet(code: string, name: string, released_at: string): CardSet {
  return {
    code,
    name,
    set_type: 'expansion',
    released_at,
    card_count: 281,
    icon_svg_uri: null,
    parent_set_code: null,
    has_drops: false,
    drop_noun: null,
    has_subtypes: false,
  }
}

const Stub = defineComponent({ template: '<div />' })

async function mountView() {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', component: Stub },
      { path: '/releases', component: Stub },
      { path: '/releases/:game', component: ReleaseCalendarView, props: true },
      { path: '/cards/:game/sets/:code', component: Stub },
      { path: '/decks/:game/precons/:slug', component: Stub },
      { path: '/decks/:game/precons/sets/:code', component: Stub },
      { path: '/sealed/:game/sets/:code', component: Stub },
      { path: '/sealed/:game/:id', component: Stub },
      { path: '/alerts', component: Stub },
    ],
  })
  await router.push('/releases/mtg')
  await router.isReady()
  const wrapper = mount(ReleaseCalendarView, {
    props: { game: 'mtg' },
    global: { plugins: [router] },
  })
  await flushPromises()
  return wrapper
}

describe('ReleaseCalendarView', () => {
  beforeEach(() => {
    // Pin "today" so the window, the "this month" badge and the tense are deterministic.
    vi.useFakeTimers({ toFake: ['Date'] })
    vi.setSystemTime(new Date(2026, 8, 8))
    state.calendar = undefined
    state.isPending = false
    state.isError = false
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it('renders month sections with each entry linked to its page and what it ships', async () => {
    state.calendar = {
      from: '2026-08-01',
      to: '2026-10-31',
      sets: [
        {
          set: calendarSet('blb', 'Bloomburrow', '2026-08-02'),
          released_at: '2026-08-02',
          secret_lair: false,
          precons: [
            { slug: 'squirreled-away-blc', name: 'Squirreled Away', deck_type: 'Commander Deck' },
          ] as PreconDeck[],
          products: [
            { id: '5551', name: 'Bloomburrow Bundle', product_type: 'bundle' },
          ] as Product[],
        },
        {
          set: calendarSet('slz', 'The Zeta Set', '2026-09-02'),
          released_at: '2026-09-02',
          secret_lair: true,
          precons: [],
          products: [],
        },
      ],
      secret_lair_drops: [
        {
          slug: 'wild-in-bloom',
          title: 'Wild in Bloom',
          set_code: 'sld',
          released_at: '2026-09-25',
          products: [],
        },
      ],
    }
    const wrapper = await mountView()

    expect(wrapper.find('h1').text()).toContain('Magic release calendar')
    // Three months of window, all present — October has nothing and says so.
    const sections = wrapper.findAll('section')
    expect(sections).toHaveLength(3)
    expect(sections[2]!.text()).toContain('Nothing in the catalog for this month yet')
    expect(sections[1]!.text()).toContain('This month')
    expect(sections[0]!.text()).not.toContain('This month')

    // Entries link to the pages they already have, and are classified as the heads-ups would.
    expect(wrapper.find('a[href="/cards/mtg/sets/blb"]').text()).toBe('Bloomburrow')
    expect(wrapper.find('a[href="/cards/mtg/sets/slz"]').exists()).toBe(true)
    expect(sections[1]!.text()).toContain('Secret Lair set')
    expect(wrapper.find('a[href="/cards/mtg/sets/sld?drop=Wild+in+Bloom"]').text()).toBe(
      'Wild in Bloom',
    )
    expect(sections[1]!.text()).toContain('Secret Lair drop')

    // What the set ships, nested and linked.
    expect(wrapper.find('a[href="/decks/mtg/precons/squirreled-away-blc"]').text()).toContain(
      'Squirreled Away',
    )
    expect(wrapper.find('a[href="/sealed/mtg/5551"]').text()).toContain('Bloomburrow Bundle')

    // Tense follows the pinned clock: August landed, the drop is still due.
    expect(sections[0]!.text()).toContain('Released')
    expect(sections[1]!.text()).toContain('Releases')

    // The header counts and the bridge to the subscription.
    expect(wrapper.text()).toContain('3 releases')
    expect(wrapper.text()).toContain('1 upcoming')
    expect(wrapper.find(`a[href="${RELEASE_HEADS_UP_PATH}"]`).text()).toContain('Get a heads-up')

    // A fact page: nothing here words a preview.
    const text = wrapper.text().toLowerCase()
    expect(text).not.toContain('spoiler')
    expect(text).not.toContain('preview')
  })

  it('shows the loading and error states', async () => {
    state.isPending = true
    let wrapper = await mountView()
    expect(wrapper.text()).toContain('Loading releases')

    state.isPending = false
    state.isError = true
    wrapper = await mountView()
    expect(wrapper.text()).toContain("Couldn't load the release calendar")
  })
})
