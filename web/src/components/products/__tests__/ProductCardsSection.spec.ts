import { describe, it, expect, vi, beforeEach } from 'vitest'
import type { Ref } from 'vue'
import { mount } from '@vue/test-utils'
import type { ProductCardsPage } from '@/lib/api'
import ProductCardsSection from '../ProductCardsSection.vue'
import CardPagination from '@/components/cards/CardPagination.vue'
import CardGrid from '@/components/cards/CardGrid.vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'

// The block that implements #224's "each section paginates on its own": it must query with its
// OWN section key + the shared search, size its pagination off its OWN query's total, reset to
// page 1 when the product or search changes, drive the pagination spinner off its query's
// placeholder state (#223), and show a loading row (not an empty grid) until its first page
// arrives. Capture the exact args passed into the query and hand back controllable state.
const state = vi.hoisted(() => ({
  calls: [] as unknown[][],
  data: undefined as ProductCardsPage | undefined,
  isPending: false,
  isPlaceholderData: false,
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
      isPlaceholderData: {
        get value() {
          return state.isPlaceholderData
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

function mountSection(
  opts: {
    data?: ProductCardsPage
    isPending?: boolean
    isPlaceholderData?: boolean
    props?: object
  } = {},
) {
  state.calls = []
  state.data = 'data' in opts ? opts.data : pageData(0)
  state.isPending = opts.isPending ?? false
  state.isPlaceholderData = opts.isPlaceholderData ?? false
  return mount(ProductCardsSection, {
    props: {
      game: 'mtg',
      id: '100',
      sectionKey: 'exclusive',
      title: 'Collector Booster exclusives',
      blurb: 'b',
      search: '',
      ...opts.props,
    },
    global: { stubs: { CardGrid: true, CardPagination: true, LoadingRow: true } },
  })
}

beforeEach(() => {
  state.calls = []
  state.data = pageData(0)
  state.isPending = false
  state.isPlaceholderData = false
})

describe('ProductCardsSection', () => {
  it('queries its own section key + the shared search (so each block pages independently)', () => {
    mountSection({ props: { sectionKey: 'booster', search: 't:goblin' } })
    // useProductCardsQuery(game, id, page, section, search) — the 4th arg is this block's own
    // section, the 5th is the shared search (a ref).
    const call = state.calls[0] ?? []
    expect(call[3]).toBe('booster')
    expect((call[4] as Ref<string>).value).toBe('t:goblin')
  })

  it('sizes its pagination off its own query total, not the parent', () => {
    const wrapper = mountSection({ data: pageData(130) })
    expect(wrapper.findComponent(CardPagination).props('total')).toBe(130)
  })

  it('drives the pagination loading flag off its query placeholder state (#223)', () => {
    // Idle (fresh page loaded) → no spinner.
    const idle = mountSection({ data: pageData(130) })
    expect(idle.findComponent(CardPagination).props('loading')).toBe(false)
    // A page transition in flight (keepPreviousData is serving the prior page) → loading.
    const paging = mountSection({ data: pageData(130), isPlaceholderData: true })
    expect(paging.findComponent(CardPagination).props('loading')).toBe(true)
  })

  it('resets to page 1 when the product changes', async () => {
    const wrapper = mountSection()
    const page = (state.calls[0] ?? [])[2] as Ref<number>
    page.value = 3
    await wrapper.setProps({ id: '200' })
    expect(page.value).toBe(1)
  })

  it('resets to page 1 when the search changes', async () => {
    const wrapper = mountSection({ data: pageData(130) })
    const page = (state.calls[0] ?? [])[2] as Ref<number>
    page.value = 3
    await wrapper.setProps({ search: 'r:mythic' })
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

  it('collapses entirely once loaded with no matches (a search filtered every card out)', () => {
    const wrapper = mountSection({ data: pageData(0), props: { search: 'zzznope' } })
    // No bare heading left behind — the whole block is gone.
    expect(wrapper.find('h3').exists()).toBe(false)
    expect(wrapper.findComponent(CardGrid).exists()).toBe(false)
    expect(wrapper.findComponent(CardPagination).exists()).toBe(false)
  })
})
