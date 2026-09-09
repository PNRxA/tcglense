import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, type Ref } from 'vue'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import type { DeckAnalytics, DeckLegality, DeckManaBase } from '@/lib/api'

// Which hooks the component asked, and with what `enabled` — the addressing mode is chosen
// once at mount and a mode that can't ask a question must never call its hook.
const calls = vi.hoisted(() => ({
  names: [] as string[],
  enabled: {} as Record<string, boolean>,
}))
const answers = vi.hoisted(() => ({
  stats: null as DeckAnalytics | null,
  mana: null as DeckManaBase | null,
  statsPending: false,
}))

vi.mock('@/composables/useDeckAnalysis', async () => {
  const { computed: vueComputed, ref: vueRef } = await import('vue')
  function settled<T>(name: string, data: () => T, enabled?: Ref<boolean>, pending = false) {
    calls.names.push(name)
    calls.enabled[name] = enabled?.value ?? true
    // A disabled read never answers, as vue-query's wouldn't.
    return {
      data: vueComputed(() => (enabled?.value === false ? undefined : data())),
      isPending: vueRef(pending),
      isLoadingError: vueRef(false),
    }
  }
  const stats =
    (name: string) => (_a: unknown, _b: unknown, _params: unknown, enabled?: Ref<boolean>) =>
      settled(name, () => answers.stats, enabled, answers.statsPending)
  const plain =
    <T>(name: string, data: () => T) =>
    (_a: unknown, _b: unknown, enabled?: Ref<boolean>) =>
      settled(name, data, enabled)
  return {
    useDeckStatsQuery: stats('deck-stats'),
    usePublicDeckStatsQuery: stats('public-stats'),
    usePreconStatsQuery: stats('precon-stats'),
    useDeckBracketQuery: plain('deck-bracket', () => ({ data: null })),
    usePublicDeckBracketQuery: plain('public-bracket', () => ({ data: null })),
    usePreconBracketQuery: plain('precon-bracket', () => ({ data: null })),
    useDeckManaQuery: plain('deck-mana', () => answers.mana),
    usePublicDeckManaQuery: plain('public-mana', () => answers.mana),
    usePreconManaQuery: plain('precon-mana', () => answers.mana),
    useDeckPricingQuery: plain('deck-pricing', () => ({
      lines: [],
      total_usd: '40.00',
      cheapest_total_usd: '30.00',
      saving_usd: '10.00',
      unpriced_count: 0,
      swappable_count: 2,
    })),
    usePublicDeckPricingQuery: plain('public-pricing', () => null),
    useDeckSuggestionsQuery: plain('deck-suggestions', () => null),
  }
})

vi.mock('@/composables/useCurrency', () => ({
  useCurrency: () => ({
    formatUsd: (raw: string | null | undefined) => (raw == null ? null : `$${raw}`),
  }),
}))

import DeckOverview from '../DeckOverview.vue'

const STORAGE_KEY = 'tcglense_deck_overview_expanded'
const Panels = defineComponent({ template: '<section data-testid="panels">the stack</section>' })

const legal: DeckLegality = {
  format_key: 'commander',
  format_label: 'Commander',
  issues: [],
  violations: [],
  card_statuses: {},
  unknown_count: 0,
  legal: true,
}

function analytics(): DeckAnalytics {
  const deck = {
    total_copies: 100,
    unique_cards: 90,
    land_copies: 37,
    average_mana_value: 2.5,
    mana_curve: [],
    colors: [],
    card_types: [],
    card_odds: [],
  }
  return {
    deck,
    library: deck,
    library_section_ids: [],
    default_library_section_ids: [],
    odds: null,
  }
}

function mountOverview(props: Partial<InstanceType<typeof DeckOverview>['$props']> = {}) {
  return mount(DeckOverview, {
    props: {
      game: 'mtg',
      deckId: 7,
      format: 'commander',
      legality: legal,
      totalCards: 100,
      description: 'Everything else.',
      ...props,
    },
    slots: { default: Panels },
  })
}

function disclosure(wrapper: ReturnType<typeof mountOverview>) {
  return wrapper.get<HTMLButtonElement>('[data-slot="card-header"] button')
}

beforeEach(() => {
  setActivePinia(createPinia())
  calls.names = []
  calls.enabled = {}
  answers.stats = analytics()
  answers.mana = null
  answers.statsPending = false
})

afterEach(() => {
  localStorage.removeItem(STORAGE_KEY)
})

describe('DeckOverview', () => {
  it('rests collapsed on the chip strip, with the stack unmounted and its contents named', () => {
    const wrapper = mountOverview()
    expect(disclosure(wrapper).attributes('aria-expanded')).toBe('false')
    expect(disclosure(wrapper).text()).toContain('Show details')
    expect(wrapper.find('[data-testid="panels"]').exists()).toBe(false)
    const chips = wrapper.findAll('ul li').map((li) => li.text().replace(/\s+/g, ' ').trim())
    expect(chips).toEqual([
      'Legal in Commander',
      '37 lands · 37% of 100',
      'Avg mana value 2.50',
      'Save $10.00 at cheapest printings',
    ])
    expect(wrapper.text()).toContain('Everything else.')
    wrapper.unmount()
  })

  it('mounts the stack on demand and remembers the choice across mounts', async () => {
    const wrapper = mountOverview()
    await disclosure(wrapper).trigger('click')
    expect(disclosure(wrapper).attributes('aria-expanded')).toBe('true')
    const bodyId = disclosure(wrapper).attributes('aria-controls')
    expect(bodyId).toBeTruthy()
    expect(wrapper.find(`[id="${bodyId}"] [data-testid="panels"]`).exists()).toBe(true)
    // The strip stays: it is the table of contents for what opened below it.
    expect(wrapper.text()).toContain('37 lands · 37% of 100')
    expect(wrapper.text()).not.toContain('Everything else.')
    expect(localStorage.getItem(STORAGE_KEY)).toBe('1')
    wrapper.unmount()

    // A fresh store seeds from the remembered choice, so the next deck opens the same way.
    setActivePinia(createPinia())
    const again = mountOverview()
    expect(again.find('[data-testid="panels"]').exists()).toBe(true)
    await disclosure(again).trigger('click')
    expect(again.find('[data-testid="panels"]').exists()).toBe(false)
    expect(localStorage.getItem(STORAGE_KEY)).toBe('0')
    again.unmount()
  })

  it('draws the mana verdict with its pips and the legality verdict in the banner’s tone', () => {
    answers.mana = {
      deck_size: 99,
      table_size: 99,
      library_size: 99,
      land_count: 37,
      colors: [
        {
          color: 'U',
          label: 'Blue',
          pips: 3,
          hybrid_pips: 0,
          demand_count: 4,
          demand: [],
          sources: 10,
          land_sources: 10,
          nonland_sources: 0,
          source_count: 10,
          source_cards: [],
          sources_needed: 14,
          shortfall: 4,
          status: 'short',
          verdict: 'Four blue sources short.',
        },
      ],
      unchecked_count: 0,
      caveats: [],
      source: 'Karsten',
    }
    const wrapper = mountOverview({ legality: { ...legal, legal: false } })
    const mana = wrapper.findAll('ul li').find((li) => li.text().includes('Mana base short on'))!
    expect(mana.attributes('title')).toBe('Four blue sources short.')
    expect(mana.find('.ms-u').exists()).toBe(true)
    expect(mana.classes()).toContain('text-warning')
    const legality = wrapper.findAll('ul li')[0]!
    expect(legality.text()).toBe('Not legal in Commander')
    expect(legality.classes()).toContain('text-destructive')
    wrapper.unmount()
  })

  it('asks only through the mode it was mounted in, and never for pricing on a precon', () => {
    mountOverview().unmount()
    expect(calls.names).toEqual([
      'deck-stats',
      'deck-bracket',
      'deck-mana',
      'deck-pricing',
      'deck-suggestions',
    ])

    calls.names = []
    mountOverview({ deckId: 7, handle: 'alice-0001' }).unmount()
    expect(calls.names).toEqual(['public-stats', 'public-bracket', 'public-mana', 'public-pricing'])

    calls.names = []
    mountOverview({ deckId: undefined, preconSlug: 'dmu-commander' }).unmount()
    expect(calls.names).toEqual(['precon-stats', 'precon-bracket', 'precon-mana'])
  })

  it('gates the bracket on Commander and every read on the deck having cards', () => {
    mountOverview({ format: 'modern' }).unmount()
    expect(calls.enabled['deck-bracket']).toBe(false)
    expect(calls.enabled['deck-stats']).toBe(true)

    calls.enabled = {}
    const empty = mountOverview({ totalCards: 0, legality: null })
    expect(Object.values(calls.enabled).every((on) => on === false)).toBe(true)
    expect(empty.text()).toContain('Add cards to see the deck’s numbers here.')
    empty.unmount()
  })

  it('holds the strip’s shape with placeholders while a read is on its first trip', () => {
    answers.statsPending = true
    answers.stats = null
    const wrapper = mountOverview()
    expect(wrapper.find('[data-slot="skeleton"]').exists()).toBe(true)
    expect(wrapper.find('[data-slot="card"]').attributes('aria-busy')).toBe('true')
    wrapper.unmount()
  })
})
