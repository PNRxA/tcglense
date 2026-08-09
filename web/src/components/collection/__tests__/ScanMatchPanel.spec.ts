import { describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import ScanMatchPanel from '../ScanMatchPanel.vue'
import type { Card } from '@/lib/api'

vi.mock('@/composables/useCurrency', () => ({
  useCurrency: () => ({ formatUsd: (amount: string) => `$${amount}` }),
}))

// Only the fields the panel reads matter here; the rest of the wire type is filled in
// so the props stay typed against the real DTO.
const baseCard = {
  id: 'card-sol-ring',
  name: 'Sol Ring',
  set_code: 'c21',
  set_name: 'Commander 2021',
  collector_number: '263',
  rarity: 'uncommon',
  lang: 'en',
  released_at: '2021-04-23',
  mana_cost: '{1}',
  cmc: 1,
  type_line: 'Artifact',
  oracle_text: null,
  power: null,
  toughness: null,
  loyalty: null,
  color_identity: [],
  colors: [],
  layout: 'normal',
  prices: { usd: '2.49', usd_foil: null, eur: null, tix: null },
  has_image: false,
  drop_name: null,
  drop_slug: null,
  secret_lair_bonus: false,
  secret_lair_spend_incentive: false,
  faces: [],
  legalities: null,
} satisfies Card

// The panel is mounted shallow: only the plain stepper markup is under test, and the real
// children (printing picker, select, card image) would drag in queries and a camera-less DOM.
function mountPanel(overrides: Record<string, unknown> = {}) {
  return mount(ScanMatchPanel, {
    shallow: true,
    props: {
      game: 'mtg',
      match: {
        ocrName: 'Sol Ring',
        hint: {},
        candidates: ['Sol Ring'],
        name: 'Sol Ring',
      },
      prints: [],
      printsFiltered: [],
      printsLoading: false,
      printsLoadingMore: false,
      printsError: false,
      printsTotal: 0,
      printsHasMore: false,
      selectedCard: null,
      selectedId: '',
      owned: { quantity: 2, foil_quantity: 0 },
      target: { quantity: 3, foil_quantity: 0 },
      ready: true,
      resolving: false,
      ownedError: false,
      disabled: false,
      candidates: [],
      filter: '',
      ...overrides,
    },
  })
}

function counts(wrapper: ReturnType<typeof mountPanel>) {
  const found = wrapper.findAll('[aria-live="polite"]')
  expect(found).toHaveLength(2)
  return { regular: found[0]!, foil: found[1]! }
}

describe('ScanMatchPanel', () => {
  it('shows the seeded counts once the holding has settled', () => {
    const wrapper = mountPanel()
    const { regular, foil } = counts(wrapper)

    expect(regular.text()).toBe('3')
    expect(regular.attributes('aria-label')).toBe('Regular: 3')
    expect(foil.text()).toBe('0')
    expect(wrapper.text()).toContain('(had 2)')
  })

  it('shows a spinner instead of the previous card counts while the new one seeds', () => {
    // `target` still holds the last scanned card's numbers until the new printing's holding
    // loads and re-seeds it — showing them would flash a count belonging to another card.
    const wrapper = mountPanel({ ready: false, resolving: true, selectedCard: null })
    const { regular, foil } = counts(wrapper)

    expect(regular.text()).toBe('')
    expect(regular.find('.animate-spin').exists()).toBe(true)
    expect(regular.attributes('aria-label')).toBe('Regular: reading count')
    expect(foil.find('.animate-spin').exists()).toBe(true)
    expect(wrapper.text()).not.toContain('(had 2)')
  })

  it('keeps spinning while the picked printing’s holding is still in flight', () => {
    // Printings have settled and one is selected, but the holding read behind it hasn't —
    // that is genuinely still working, unlike the terminal states below.
    const wrapper = mountPanel({ ready: false, printsLoading: false, selectedCard: baseCard })

    expect(counts(wrapper).regular.find('.animate-spin').exists()).toBe(true)
  })

  // "Not seeded" is not the same as "still loading": each of these has stopped, and the
  // surface shows its own error/empty text plus a Retry. A spinner there would promise
  // progress that never arrives.
  const terminal = [
    ['the holding read failed', { ownedError: true, selectedCard: baseCard }],
    ['the printings page failed before anything was picked', { printsError: true }],
    ['the name resolved to no printings at all', { printsTotal: 0 }],
  ] as const

  it.each(terminal)('shows a placeholder, not a spinner, when %s', (_name, overrides) => {
    const wrapper = mountPanel({
      ready: false,
      resolving: false,
      printsLoading: false,
      selectedCard: null,
      ...overrides,
    })
    const { regular, foil } = counts(wrapper)

    expect(regular.find('.animate-spin').exists()).toBe(false)
    expect(foil.find('.animate-spin').exists()).toBe(false)
    expect(regular.text()).toBe('—')
    expect(regular.attributes('aria-label')).toBe('Regular: count unavailable')
  })
})
