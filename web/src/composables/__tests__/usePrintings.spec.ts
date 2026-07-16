import { afterEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, ref } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { getCardPrintingsByName, type CardPage } from '@/lib/api'
import { makeCard } from '@/test/fixtures'
import { usePrintingPicker } from '@/composables/usePrintings'

vi.mock('@/lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api')>()
  return { ...actual, getCardPrintingsByName: vi.fn<typeof actual.getCardPrintingsByName>() }
})

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
})
