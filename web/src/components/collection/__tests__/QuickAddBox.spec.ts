import { describe, it, expect, vi } from 'vitest'
import { ref } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'

// Drive the box off controlled suggestions rather than the network: the composable is
// mocked so the test exercises the combobox's rendering / keyboard / selection logic,
// not the query layer (covered by the API path + backend tests). The whole module is
// replaced, so the print-picker's query hook is stubbed here too (the dialog itself is
// stubbed out below, so it never runs).
vi.mock('@/composables/useQuickAdd', () => ({
  QUICK_ADD_MIN_CHARS: 2,
  useCardNameSuggestions: () => ({
    data: ref({ data: ['Lightning Bolt', 'Lightning Helix'] }),
    isFetching: ref(false),
  }),
  useCardPrintingsByName: () => ({
    data: ref(undefined),
    isFetching: ref(false),
    isPending: ref(false),
    isError: ref(false),
  }),
}))

import QuickAddBox from '../QuickAddBox.vue'

// Records the props the box hands the print picker, so we can assert which name was
// chosen and that it opened — without mounting the real dialog (and its own queries).
const DialogStub = {
  name: 'QuickAddPrintDialog',
  props: ['open', 'game', 'name'],
  emits: ['update:open'],
  template: '<div class="dialog-stub" :data-open="String(open)">{{ name }}</div>',
}

function mountBox() {
  return mount(QuickAddBox, {
    props: { game: 'mtg' },
    global: { stubs: { QuickAddPrintDialog: DialogStub } },
  })
}

describe('QuickAddBox', () => {
  it('shows the matching names as options once the term is long enough', async () => {
    const wrapper = mountBox()
    // Below the minimum: no dropdown.
    await wrapper.find('input').setValue('l')
    await flushPromises()
    expect(wrapper.find('[role="listbox"]').exists()).toBe(false)

    await wrapper.find('input').setValue('li')
    await flushPromises()
    const options = wrapper.findAll('[role="option"]')
    expect(options.map((o) => o.text())).toEqual(['Lightning Bolt', 'Lightning Helix'])
    // Options are navigated via arrow keys + aria-activedescendant, so they stay out of
    // the tab order (the input is the single tab stop).
    expect(options.every((o) => o.attributes('tabindex') === '-1')).toBe(true)
    wrapper.unmount()
  })

  it('opens the print picker for the clicked name', async () => {
    const wrapper = mountBox()
    await wrapper.find('input').setValue('li')
    await flushPromises()

    await wrapper.findAll('[role="option"]')[1]!.trigger('click')
    await flushPromises()

    const dialog = wrapper.findComponent(DialogStub)
    expect(dialog.props('open')).toBe(true)
    expect(dialog.props('name')).toBe('Lightning Helix')
    // The box resets so the next quick-add starts fresh.
    expect((wrapper.find('input').element as HTMLInputElement).value).toBe('')
    wrapper.unmount()
  })

  it('lets the keyboard highlight and choose a name', async () => {
    const wrapper = mountBox()
    const input = wrapper.find('input')
    await input.setValue('li')
    await flushPromises()

    await input.trigger('keydown', { key: 'ArrowDown' }) // highlight first
    await input.trigger('keydown', { key: 'ArrowDown' }) // highlight second
    await input.trigger('keydown', { key: 'Enter' })
    await flushPromises()

    const dialog = wrapper.findComponent(DialogStub)
    expect(dialog.props('open')).toBe(true)
    expect(dialog.props('name')).toBe('Lightning Helix')
    wrapper.unmount()
  })
})
