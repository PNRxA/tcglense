import { afterEach, describe, expect, it, vi } from 'vitest'
import { computed, defineComponent, ref } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { getCardPrintingsByName, type CardPage } from '@/lib/api'
import { makeCard } from '@/test/fixtures'
import { useOwnedCounts } from '@/composables/useCollection'
import { usePrintingPicker } from '@/composables/usePrintings'

vi.mock('@/lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api')>()
  return { ...actual, getCardPrintingsByName: vi.fn<typeof actual.getCardPrintingsByName>() }
})

// The collection filter folds this batch-counts hook over the loaded printings; mock it so
// the picker's filtering is exercised without the auth/query stack behind the real hook.
vi.mock('@/composables/useCollection', () => ({
  useOwnedCounts: vi.fn<(...args: unknown[]) => unknown>(),
}))

const Harness = defineComponent({
  setup() {
    const picker = usePrintingPicker(ref('mtg'), ref('Island'))
    return { ...picker }
  },
  template: `
    <input v-model="filter" aria-label="Filter" />
    <span data-loaded>{{ loadedCount }}</span>
    <span data-filtered>{{ filteredPrintings.map((card) => card.id).join(',') }}</span>
    <button :disabled="!hasNextPage" @click="loadMore">Load more</button>
  `,
})

function page(number: number): CardPage {
  if (number === 1) {
    return {
      data: Array.from({ length: 200 }, (_, index) =>
        makeCard(`new-${index}`, { collector_number: String(index + 1) }),
      ),
      page: 1,
      page_size: 200,
      total: 201,
      has_more: true,
    }
  }
  return {
    data: [
      makeCard('old-printing', {
        set_code: 'old',
        set_name: 'Old Set',
        collector_number: '999',
      }),
    ],
    page: 2,
    page_size: 200,
    total: 201,
    has_more: false,
  }
}

afterEach(() => vi.clearAllMocks())

describe('usePrintingPicker', () => {
  it('accumulates pages beyond 200 and filters the loaded result set', async () => {
    vi.mocked(getCardPrintingsByName).mockImplementation(async (_game, _name, number) =>
      page(number ?? 1),
    )
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const wrapper = mount(Harness, {
      global: { plugins: [[VueQueryPlugin, { queryClient }]] },
    })
    await flushPromises()

    expect(wrapper.get('[data-loaded]').text()).toBe('200')
    await wrapper.get('input').setValue('old')
    expect(wrapper.get('[data-filtered]').text()).toBe('')

    await wrapper.get('button').trigger('click')
    await flushPromises()

    expect(wrapper.get('[data-loaded]').text()).toBe('201')
    expect(wrapper.get('[data-filtered]').text()).toBe('old-printing')
    expect(vi.mocked(getCardPrintingsByName).mock.calls.map((call) => call[2])).toEqual([1, 2])
    expect(queryClient.getQueryCache().getAll()[0]?.queryKey).toEqual([
      'card-printings',
      'mtg',
      'Island',
    ])
  })

  it('narrows the loaded printings to owned cards when the collection filter is on', async () => {
    vi.mocked(getCardPrintingsByName).mockResolvedValue({
      data: [makeCard('owned-a'), makeCard('unowned-b'), makeCard('owned-c')],
      page: 1,
      page_size: 200,
      total: 3,
      has_more: false,
    })
    // Only two of the three printings are held (a regular copy, and a foil-only copy).
    vi.mocked(useOwnedCounts).mockReturnValue({
      ownership: computed(() => ({
        'owned-a': { quantity: 1, foil_quantity: 0 },
        'owned-c': { quantity: 0, foil_quantity: 2 },
      })),
      ready: computed(() => true),
      fetching: computed(() => false),
    })

    const CollectionHarness = defineComponent({
      setup() {
        const picker = usePrintingPicker(ref('mtg'), ref('Island'), { collectionFilter: true })
        return { ...picker }
      },
      template: `
        <input type="checkbox" v-model="collectionOnly" />
        <span data-filtered>{{ filteredPrintings.map((card) => card.id).join(',') }}</span>
      `,
    })

    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const wrapper = mount(CollectionHarness, {
      global: { plugins: [[VueQueryPlugin, { queryClient }]] },
    })
    await flushPromises()

    // Default-off: the (empty) filter passes every loaded printing through.
    expect(wrapper.get('[data-filtered]').text()).toBe('owned-a,unowned-b,owned-c')

    // Toggled on: only the held printings survive, order preserved.
    await wrapper.get('input[type="checkbox"]').setValue(true)
    expect(wrapper.get('[data-filtered]').text()).toBe('owned-a,owned-c')
  })
})
