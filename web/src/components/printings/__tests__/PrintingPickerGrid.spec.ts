import { describe, expect, it } from 'vitest'
import { defineComponent } from 'vue'
import { mount } from '@vue/test-utils'
import PrintingPickerGrid from '@/components/printings/PrintingPickerGrid.vue'
import { makeCard } from '@/test/fixtures'

const ButtonStub = defineComponent({
  inheritAttrs: false,
  template: '<button v-bind="$attrs"><slot /></button>',
})
const SearchStub = defineComponent({
  props: { modelValue: { type: String, required: true } },
  emits: ['update:modelValue'],
  template: `
    <input
      :value="modelValue"
      @input="$emit('update:modelValue', $event.target.value)"
    />
  `,
})
const LoadingStub = defineComponent({
  props: { label: String },
  template: '<p>{{ label }}</p>',
})
// A plain <select> stand-in for the sort dropdown (a popover menu that's awkward to drive
// in jsdom), exposing just the `field:dir` value the grid reorders by.
const SortStub = defineComponent({
  props: {
    modelValue: { type: String, required: true },
    options: { type: Array as () => { value: string; label: string }[], default: () => [] },
  },
  emits: ['update:modelValue'],
  template: `
    <select
      aria-label="Sort"
      :value="modelValue"
      @change="$emit('update:modelValue', $event.target.value)"
    >
      <option v-for="o in options" :key="o.value" :value="o.value">{{ o.label }}</option>
    </select>
  `,
})

const defaults = {
  printings: [] as ReturnType<typeof makeCard>[],
  filteredPrintings: [] as ReturnType<typeof makeCard>[],
  filter: '',
  total: 0,
  pending: false,
  error: false,
  hasMore: false,
  loadingMore: false,
  collectionFilter: false,
  collectionOnly: false,
  collectionLoading: false,
}

function mountGrid(props: Partial<typeof defaults> = {}) {
  return mount(PrintingPickerGrid, {
    props: { ...defaults, ...props },
    // The tile renders its printing id so a spec can assert the rendered order.
    slots: { tile: '<span class="pid">{{ params.printing.id }}</span>' },
    global: {
      stubs: {
        Button: ButtonStub,
        CardSearchBox: SearchStub,
        CardSortMenu: SortStub,
        LoadingRow: LoadingStub,
      },
    },
  })
}

const renderedIds = (wrapper: ReturnType<typeof mountGrid>) =>
  wrapper.findAll('.pid').map((n) => n.text())

describe('PrintingPickerGrid', () => {
  it('shares the initial loading, error, and empty states', () => {
    expect(mountGrid({ pending: true }).text()).toContain('Loading printings…')
    expect(mountGrid({ error: true }).text()).toContain('Could not load printings')
    expect(mountGrid().text()).toContain('No printings found')
  })

  it('makes loaded-page filter scope explicit and emits load-more', async () => {
    const cards = [makeCard('one'), makeCard('two')]
    const wrapper = mountGrid({
      printings: cards,
      filteredPrintings: [],
      filter: 'old',
      total: 816,
      hasMore: true,
    })

    expect(wrapper.text()).toContain('0 matching · 2 loaded of 816')
    expect(wrapper.text()).toContain('Filter searches loaded printings only.')
    expect(wrapper.text()).toContain('No loaded printings match “old”.')

    await wrapper.get('button').trigger('click')
    expect(wrapper.emitted('loadMore')).toHaveLength(1)
  })

  it('offers an opt-in collection filter that emits its toggle', async () => {
    const cards = [makeCard('one'), makeCard('two')]
    // Off by default and absent unless opted in.
    expect(
      mountGrid({ printings: cards, filteredPrintings: cards, total: 2 }).text(),
    ).not.toContain('In my collection')

    const wrapper = mountGrid({
      printings: cards,
      filteredPrintings: cards,
      total: 2,
      collectionFilter: true,
    })
    const checkbox = wrapper.get('input[type="checkbox"]')
    expect((checkbox.element as HTMLInputElement).checked).toBe(false)

    await checkbox.setValue(true)
    expect(wrapper.emitted('update:collectionOnly')).toEqual([[true]])
  })

  it('shows a checking state while owned counts load and collection-aware empty copy', () => {
    const cards = [makeCard('one'), makeCard('two')]
    expect(
      mountGrid({
        printings: cards,
        filteredPrintings: cards,
        total: 2,
        collectionFilter: true,
        collectionOnly: true,
        collectionLoading: true,
      }).text(),
    ).toContain('Checking your collection…')

    // Toggle on, no text filter, nothing owned in the loaded pages.
    expect(
      mountGrid({
        printings: cards,
        filteredPrintings: [],
        total: 2,
        collectionFilter: true,
        collectionOnly: true,
      }).text(),
    ).toContain('None of the loaded printings are in your collection.')

    // Toggle on plus a text filter that matches nothing owned.
    expect(
      mountGrid({
        printings: cards,
        filteredPrintings: [],
        total: 2,
        filter: 'zne',
        collectionFilter: true,
        collectionOnly: true,
      }).text(),
    ).toContain('No loaded printings in your collection match “zne”.')
  })

  it('reorders the rendered printings by the chosen sort (default newest-first)', async () => {
    const cards = [
      makeCard('old', { released_at: '2019-01-01' }),
      makeCard('new', { released_at: '2024-01-01' }),
      makeCard('mid', { released_at: '2021-01-01' }),
    ]
    const wrapper = mountGrid({ printings: cards, filteredPrintings: cards, total: 3 })

    // Default sort is newest-first, regardless of the incoming order.
    expect(renderedIds(wrapper)).toEqual(['new', 'mid', 'old'])

    await wrapper.get('select[aria-label="Sort"]').setValue('released:asc')
    expect(renderedIds(wrapper)).toEqual(['old', 'mid', 'new'])
  })
})
