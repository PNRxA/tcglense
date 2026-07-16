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

const defaults = {
  printings: [] as ReturnType<typeof makeCard>[],
  filteredPrintings: [] as ReturnType<typeof makeCard>[],
  filter: '',
  total: 0,
  pending: false,
  error: false,
  hasMore: false,
  loadingMore: false,
}

function mountGrid(props: Partial<typeof defaults> = {}) {
  return mount(PrintingPickerGrid, {
    props: { ...defaults, ...props },
    slots: { tile: '<span>tile</span>' },
    global: {
      stubs: { Button: ButtonStub, CardSearchBox: SearchStub, LoadingRow: LoadingStub },
    },
  })
}

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
})
