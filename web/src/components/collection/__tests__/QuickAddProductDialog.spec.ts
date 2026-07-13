import { describe, it, expect, beforeEach, vi } from 'vitest'
import { ref } from 'vue'
import { mount } from '@vue/test-utils'
import type { Product } from '@/lib/api'

// Drive the dialog off controlled composables: the authoritative-count query and the
// owned-count editor are both mocked, so the test exercises the dialog's seeding gate
// (steppers disabled until the count settles), its single quantity row, the editor
// wiring (`kind: 'product'`), and the close-focus forwarding — not the query/mutation
// layers (covered elsewhere). `holder` carries mutable state the factories read at
// setup, so each test sets it *before* mounting.
const holder = vi.hoisted(() => ({
  want: {
    isSuccess: true,
    isError: false,
    isFetching: false,
    data: { quantity: 0, foil_quantity: 0 } as
      | { quantity: number; foil_quantity: number }
      | undefined,
  },
  editor: { regular: 0, saving: false, saveError: false },
  editorOpts: undefined as unknown,
  adjust: undefined as unknown,
}))

vi.mock('@/composables/useWishlist', () => ({
  useWishlistProductEntryQuery: () => ({
    isSuccess: ref(holder.want.isSuccess),
    isError: ref(holder.want.isError),
    isFetching: ref(holder.want.isFetching),
    data: ref(holder.want.data),
  }),
}))

vi.mock('@/composables/useOwnedCountEditor', () => ({
  useOwnedCountEditor: (_game: unknown, _id: unknown, _seed: unknown, opts: unknown) => {
    holder.editorOpts = opts
    const adjust = vi.fn<(which: 'quantity' | 'foil', delta: number) => void>()
    holder.adjust = adjust
    return {
      regular: ref(holder.editor.regular),
      adjust,
      saving: ref(holder.editor.saving),
      saveError: ref(holder.editor.saveError),
    }
  },
}))

import QuickAddProductDialog from '../QuickAddProductDialog.vue'

const PRODUCT: Product = {
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
}

// Minimal pass-through stubs for the reka dialog chrome so the body renders inline (not
// teleported) and the close-focus event can be fired directly. The steppers use the real
// Button so `disabled` reflects on the native element.
const DialogRootStub = { props: ['open'], emits: ['update:open'], template: '<div><slot /></div>' }
const DialogContentStub = {
  name: 'DialogContent',
  emits: ['closeAutoFocus'],
  template: '<div class="dialog-content"><slot /></div>',
}
const PassThrough = { template: '<div><slot /></div>' }

function mountDialog(product: Product | null = PRODUCT) {
  return mount(QuickAddProductDialog, {
    props: { open: true, game: 'mtg', product },
    global: {
      stubs: {
        Dialog: DialogRootStub,
        DialogContent: DialogContentStub,
        DialogTitle: PassThrough,
        DialogDescription: PassThrough,
        DialogClose: PassThrough,
        ProductImage: true,
      },
    },
  })
}

const addBtn = '[aria-label="Add one to your wish list"]'
const removeBtn = '[aria-label="Remove one from your wish list"]'

function isDisabled(wrapper: ReturnType<typeof mountDialog>, selector: string): boolean {
  return (wrapper.find(selector).element as HTMLButtonElement).disabled
}

describe('QuickAddProductDialog', () => {
  beforeEach(() => {
    holder.want = {
      isSuccess: true,
      isError: false,
      isFetching: false,
      data: { quantity: 0, foil_quantity: 0 },
    }
    holder.editor = { regular: 0, saving: false, saveError: false }
    holder.editorOpts = undefined
    holder.adjust = undefined
  })

  it('drives the wish list via the product editor (kind: "product")', () => {
    const wrapper = mountDialog()
    expect(holder.editorOpts).toEqual({ kind: 'product' })
    wrapper.unmount()
  })

  it('renders exactly one quantity row — no foil counterpart', () => {
    const wrapper = mountDialog()
    expect(wrapper.text()).toContain('Quantity')
    expect(wrapper.text()).not.toContain('Foil')
    expect(wrapper.text()).not.toContain('Regular')
    // One remove + one add button, and nothing else quantity-shaped.
    expect(wrapper.findAll(addBtn)).toHaveLength(1)
    expect(wrapper.findAll(removeBtn)).toHaveLength(1)
    wrapper.unmount()
  })

  it('keeps the steppers disabled while the count is refetching, even after success', () => {
    // The stale-seed gate: `isSuccess` true off a retained cache but a staleTime-0
    // refetch is in flight, so the absolute-count editor must not be seeded yet.
    holder.want = {
      isSuccess: true,
      isError: false,
      isFetching: true,
      data: { quantity: 4, foil_quantity: 0 },
    }
    holder.editor = { regular: 4, saving: false, saveError: false }
    const wrapper = mountDialog()
    expect(isDisabled(wrapper, addBtn)).toBe(true)
    expect(isDisabled(wrapper, removeBtn)).toBe(true)
    wrapper.unmount()
  })

  it('surfaces a load error and keeps the steppers disabled when the count query fails', () => {
    // Retries exhausted: `isSuccess` never flips true, so `ready` stays false. Without an
    // error branch the row would be blank and the steppers stuck disabled with no reason.
    holder.want = { isSuccess: false, isError: true, isFetching: false, data: undefined }
    const wrapper = mountDialog()
    expect(wrapper.text()).toContain("Couldn't load")
    expect(isDisabled(wrapper, addBtn)).toBe(true)
    expect(isDisabled(wrapper, removeBtn)).toBe(true)
    wrapper.unmount()
  })

  it("masks the count and hides the saved tick while a new product's count loads", () => {
    // Picking product B right after editing A: the query is refetching and the editor's
    // `regular` still holds A's value until the new seed lands. The count and saved tick
    // must be gated on `ready` so B never flashes A's stale count with a Saved tick.
    holder.want = {
      isSuccess: true,
      isError: false,
      isFetching: true,
      data: { quantity: 4, foil_quantity: 0 },
    }
    holder.editor = { regular: 4, saving: false, saveError: false }
    const wrapper = mountDialog()
    // Placeholder, not the retained 4; no Saved tick; steppers disabled.
    expect(wrapper.find('.w-8').text()).toBe('—')
    expect(wrapper.text()).not.toContain('Saved')
    expect(isDisabled(wrapper, addBtn)).toBe(true)
    expect(isDisabled(wrapper, removeBtn)).toBe(true)
    wrapper.unmount()
  })

  it('enables the steppers once the count has settled', () => {
    holder.want = {
      isSuccess: true,
      isError: false,
      isFetching: false,
      data: { quantity: 2, foil_quantity: 0 },
    }
    holder.editor = { regular: 2, saving: false, saveError: false }
    const wrapper = mountDialog()
    expect(isDisabled(wrapper, addBtn)).toBe(false)
    // Minus is enabled because the current count is above zero.
    expect(isDisabled(wrapper, removeBtn)).toBe(false)

    wrapper.find(addBtn).trigger('click')
    expect(holder.adjust).toHaveBeenCalledWith('quantity', 1)
    wrapper.unmount()
  })

  it('disables minus at zero even once settled', () => {
    holder.want = {
      isSuccess: true,
      isError: false,
      isFetching: false,
      data: { quantity: 0, foil_quantity: 0 },
    }
    holder.editor = { regular: 0, saving: false, saveError: false }
    const wrapper = mountDialog()
    expect(isDisabled(wrapper, addBtn)).toBe(false)
    expect(isDisabled(wrapper, removeBtn)).toBe(true)
    wrapper.unmount()
  })

  it('shows a saved confirmation once a want exists', () => {
    holder.editor = { regular: 3, saving: false, saveError: false }
    const wrapper = mountDialog()
    expect(wrapper.text()).toContain('Saved')
    wrapper.unmount()
  })

  it('re-emits the dialog close-auto-focus so the parent can restore focus', async () => {
    const wrapper = mountDialog()
    wrapper.findComponent(DialogContentStub).vm.$emit('closeAutoFocus', new Event('x'))
    await wrapper.vm.$nextTick()
    expect(wrapper.emitted('closeAutoFocus')).toBeTruthy()
    wrapper.unmount()
  })
})
