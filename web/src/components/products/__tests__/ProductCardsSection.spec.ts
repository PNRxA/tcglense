import { describe, it, expect, vi, beforeEach } from 'vitest'
import type { Ref } from 'vue'
import { mount } from '@vue/test-utils'
import type { ProductCardsPage } from '@/lib/api'
import ProductCardsSection from '../ProductCardsSection.vue'
import CardPagination from '@/components/cards/CardPagination.vue'
import CardGrid from '@/components/cards/CardGrid.vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'

// The block that implements #224's "each section paginates on its own": it must query with its
// OWN section key, size its pagination off its OWN query's total, reset to page 1 when the
// product changes, and show a loading row (not an empty grid) until its first page arrives.
// Capture the exact args passed into the query and hand back controllable data/isPending.
const state = vi.hoisted(() => ({
  calls: [] as unknown[][],
  data: undefined as ProductCardsPage | undefined,
  isPending: false,
}))

vi.mock('@/composables/useProducts', () => ({
  PRODUCT_CARDS_PAGE_SIZE: 60,
  useProductCardsQuery: (...args: unknown[]) => {
    state.calls.push(args)
    return {
      data: {
        get value() {
          return state.data
        },
      },
      isPending: {
        get value() {
          return state.isPending
        },
      },
    }
  },
}))
vi.mock('@/composables/useCollection', () => ({
  useOwnedCounts: () => ({ ownership: {} }),
}))

function pageData(total: number): ProductCardsPage {
  return { data: [], page: 1, page_size: 60, total, has_more: total > 60 }
}

function mountSection(opts: { data?: ProductCardsPage; isPending?: boolean; props?: object } = {}) {
  state.calls = []
  state.data = 'data' in opts ? opts.data : pageData(0)
  state.isPending = opts.isPending ?? false
  return mount(ProductCardsSection, {
    props: {
      game: 'mtg',
      id: '100',
      sectionKey: 'exclusive',
      title: 'Collector Booster exclusives',
      blurb: 'b',
      ...opts.props,
    },
    global: { stubs: { CardGrid: true, CardPagination: true, LoadingRow: true } },
  })
}

beforeEach(() => {
  state.calls = []
  state.data = pageData(0)
  state.isPending = false
})

describe('ProductCardsSection', () => {
  it('queries its own section key (so each block pages independently)', () => {
    mountSection({ props: { sectionKey: 'booster' } })
    // useProductCardsQuery(game, id, page, section) — the 4th arg is this block's own section.
    const call = state.calls[0] ?? []
    expect(call[3]).toBe('booster')
  })

  it('sizes its pagination off its own query total, not the parent', () => {
    const wrapper = mountSection({ data: pageData(130) })
    expect(wrapper.findComponent(CardPagination).props('total')).toBe(130)
  })

  it('resets to page 1 when the product changes', async () => {
    const wrapper = mountSection()
    const page = (state.calls[0] ?? [])[2] as Ref<number>
    page.value = 3
    await wrapper.setProps({ id: '200' })
    expect(page.value).toBe(1)
  })

  it('shows a loading row (not an empty grid) until the first page loads', () => {
    const wrapper = mountSection({ data: undefined, isPending: true })
    expect(wrapper.findComponent(LoadingRow).exists()).toBe(true)
    expect(wrapper.findComponent(CardGrid).exists()).toBe(false)
    expect(wrapper.findComponent(CardPagination).exists()).toBe(false)
  })

  it('renders the grid + pagination once loaded', () => {
    const wrapper = mountSection({ data: pageData(5) })
    expect(wrapper.findComponent(CardGrid).exists()).toBe(true)
    expect(wrapper.findComponent(LoadingRow).exists()).toBe(false)
  })
})
