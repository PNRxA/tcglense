import { describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import ScanMatchPanel from '../ScanMatchPanel.vue'

vi.mock('@/composables/useCurrency', () => ({
  useCurrency: () => ({ formatUsd: (amount: string) => `$${amount}` }),
}))

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
    const wrapper = mountPanel({ ready: false })
    const { regular, foil } = counts(wrapper)

    expect(regular.text()).toBe('')
    expect(regular.find('.animate-spin').exists()).toBe(true)
    expect(regular.attributes('aria-label')).toBe('Regular: reading count')
    expect(foil.find('.animate-spin').exists()).toBe(true)
    expect(wrapper.text()).not.toContain('(had 2)')
  })
})
