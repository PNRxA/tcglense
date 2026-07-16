import { describe, it, expect, vi } from 'vitest'
import { ref } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'

// Drive the box off controlled suggestions rather than the network: the composable is
// mocked so the test exercises the combobox's rendering / keyboard / selection logic,
// not the query layer (covered by the API path + backend tests). The whole module is
// replaced, so the print-picker's query hook is stubbed here too (the dialog itself is
// stubbed out below, so it never runs), and the product-suggestion hook (#364) is added.
const products = vi.hoisted(() => [
  {
    id: '100',
    name: 'Bloomburrow Play Booster Box',
    set_code: 'blb',
    set_name: 'Bloomburrow',
    product_type: 'play_display',
    url: null,
    has_image: true,
    prices: { usd: '99.99', usd_foil: null },
    msrp: null,
    released_at: null,
  },
  {
    id: '200',
    name: 'Bloomburrow Collector Booster Box',
    set_code: 'blb',
    set_name: 'Bloomburrow',
    product_type: 'collector_display',
    url: null,
    has_image: true,
    prices: { usd: '249.99', usd_foil: null },
    msrp: null,
    released_at: null,
  },
])

vi.mock('@/composables/useQuickAdd', () => ({
  QUICK_ADD_MIN_CHARS: 2,
  useCardNameSuggestions: () => ({
    data: ref({ data: ['Lightning Bolt', 'Lightning Helix'] }),
    isFetching: ref(false),
  }),
  useProductSuggestions: () => ({
    data: ref({ data: products }),
    isFetching: ref(false),
  }),
}))

import QuickAddBox from '../QuickAddBox.vue'

// Records the props the box hands the print picker, so we can assert which name was
// chosen, which list it targets, and that it opened — without mounting the real dialog
// (and its own queries).
const DialogStub = {
  name: 'QuickAddPrintDialog',
  props: ['open', 'game', 'name', 'list'],
  emits: ['update:open'],
  template: '<div class="dialog-stub" :data-open="String(open)">{{ name }}</div>',
}

// The sealed-product step-two dialog (#364), stubbed the same way.
const ProductDialogStub = {
  name: 'QuickAddProductDialog',
  props: ['open', 'game', 'product'],
  emits: ['update:open'],
  template: '<div class="product-dialog-stub" :data-open="String(open)">{{ product?.name }}</div>',
}

function mountBox(props: Record<string, unknown> = {}) {
  return mount(QuickAddBox, {
    props: { game: 'mtg', ...props },
    global: {
      stubs: { QuickAddPrintDialog: DialogStub, QuickAddProductDialog: ProductDialogStub },
    },
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

  it('labels the box for the wish list and targets the print picker at it (#167)', async () => {
    const wrapper = mountBox({ list: 'wishlist' })
    expect(wrapper.find('input').attributes('aria-label')).toBe(
      'Quick add a card to your wish list',
    )

    await wrapper.find('input').setValue('li')
    await flushPromises()
    await wrapper.findAll('[role="option"]')[0]!.trigger('click')
    await flushPromises()

    expect(wrapper.findComponent(DialogStub).props('list')).toBe('wishlist')
    wrapper.unmount()
  })

  it('suggests sealed products and opens the product dialog in kind="product" (#364)', async () => {
    const wrapper = mountBox({ kind: 'product' })
    expect(wrapper.find('input').attributes('aria-label')).toBe(
      'Quick add a sealed product to your wish list',
    )

    await wrapper.find('input').setValue('bloom')
    await flushPromises()
    const options = wrapper.findAll('[role="option"]')
    expect(options).toHaveLength(2)
    // Each option shows the product name plus a set · type sublabel.
    expect(options[0]!.text()).toContain('Bloomburrow Play Booster Box')
    expect(options[0]!.text()).toContain('Bloomburrow')
    expect(options[0]!.text()).toContain('Play Booster Box')

    await options[1]!.trigger('click')
    await flushPromises()

    const productDialog = wrapper.findComponent(ProductDialogStub)
    expect(productDialog.props('open')).toBe(true)
    expect(productDialog.props('product')).toMatchObject({ id: '200' })
    // The card print picker never mounts in product mode.
    expect(wrapper.findComponent(DialogStub).exists()).toBe(false)
    // The box resets so the next quick-add starts fresh.
    expect((wrapper.find('input').element as HTMLInputElement).value).toBe('')
    wrapper.unmount()
  })
})
