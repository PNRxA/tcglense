import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from 'vitest'
import { nextTick, type Ref } from 'vue'
import { mount, type VueWrapper } from '@vue/test-utils'
import { Heart } from '@lucide/vue'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createPinia, setActivePinia } from 'pinia'

// The sealed-product grid control mirrors OwnedCountControl: one collection-primary
// bottom-left trigger opens both Collection and Wish list editors. Drive both editors
// through the real useOwnedCountEditor and mock only their query/mutation seams.
type Counts = { quantity: number; foil_quantity: number }
type MutateFn = (vars: { game: string; id: string } & Counts) => Promise<Counts>
type EntryMock = {
  data: Ref<Counts | undefined>
  isSuccess: Ref<boolean>
  isFetching: Ref<boolean>
}
type EntryOpts = { enabled?: Ref<boolean>; staleTime?: number }

const h = vi.hoisted(() => ({
  collectionEntry: null as unknown as EntryMock,
  wishlistEntry: null as unknown as EntryMock,
  collectionOpts: null as unknown as EntryOpts,
  wishlistOpts: null as unknown as EntryOpts,
  collectionMutate: null as unknown as Mock<MutateFn>,
  wishlistMutate: null as unknown as Mock<MutateFn>,
}))

vi.mock('@/composables/useWishlist', async () => {
  const { ref } = await import('vue')
  h.wishlistEntry = {
    data: ref<Counts | undefined>(undefined),
    isSuccess: ref(false),
    isFetching: ref(false),
  }
  h.wishlistMutate = vi.fn<MutateFn>().mockResolvedValue({ quantity: 0, foil_quantity: 0 })
  return {
    useWishlistProductEntryQuery: (_game: Ref<string>, _id: Ref<string>, opts: EntryOpts) => {
      h.wishlistOpts = opts
      return h.wishlistEntry
    },
    useSetWishlistProductEntryMutation: () => ({
      mutateAsync: h.wishlistMutate,
      isPending: ref(false),
    }),
    useSetWishlistEntryMutation: () => ({ mutateAsync: vi.fn<MutateFn>(), isPending: ref(false) }),
  }
})

vi.mock('@/composables/useCollection', async () => {
  const { ref } = await import('vue')
  h.collectionEntry = {
    data: ref<Counts | undefined>(undefined),
    isSuccess: ref(false),
    isFetching: ref(false),
  }
  h.collectionMutate = vi.fn<MutateFn>().mockResolvedValue({ quantity: 0, foil_quantity: 0 })
  return {
    useCollectionProductEntryQuery: (_game: Ref<string>, _id: Ref<string>, opts: EntryOpts) => {
      h.collectionOpts = opts
      return h.collectionEntry
    },
    useSetCollectionProductEntryMutation: () => ({
      mutateAsync: h.collectionMutate,
      isPending: ref(false),
    }),
    useSetCollectionEntryMutation: () => ({
      mutateAsync: vi.fn<MutateFn>(),
      isPending: ref(false),
    }),
  }
})

import ProductCountControl from '../ProductCountControl.vue'

function mountControl(
  props: { quantity?: number; foilQuantity?: number; wishlistSeed?: Counts } = {},
) {
  const pinia = createPinia()
  setActivePinia(pinia)
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return mount(ProductCountControl, {
    attachTo: document.body,
    props: {
      game: 'mtg',
      productId: '100',
      name: 'Booster Box',
      quantity: props.quantity ?? 0,
      foilQuantity: props.foilQuantity ?? 0,
      wishlistSeed: props.wishlistSeed,
    },
    global: { plugins: [pinia, [VueQueryPlugin, { queryClient }]] },
  })
}

// A resting wanted seed of N regular copies (products edit only regular; foil is preserved).
function want(quantity: number): Counts {
  return { quantity, foil_quantity: 0 }
}

// reka teleports the popover content to <body>, so reach its controls through the document.
function byLabel(label: string): HTMLButtonElement | null {
  return document.body.querySelector(`[aria-label="${label}"]`)
}

// `flushPromises` schedules via setImmediate, which fake timers intercept. Advancing timers
// also runs each editor's 350 ms debounce when requested.
async function flush(ms = 0) {
  await nextTick()
  await vi.advanceTimersByTimeAsync(ms)
  await nextTick()
}

async function openPopover(wrapper: VueWrapper, triggerLabel: string) {
  await wrapper.find(`[aria-label="${triggerLabel}"]`).trigger('click')
  await flush()
}

const ADD_COLLECTION = 'Add one Booster Box to your collection'
const REMOVE_COLLECTION = 'Remove one Booster Box from your collection'
const ADD_WISHLIST = 'Add one Booster Box to your wish list'
const REMOVE_WISHLIST = 'Remove one Booster Box from your wish list'

describe('ProductCountControl unified sealed quick add', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    for (const entry of [h.collectionEntry, h.wishlistEntry]) {
      entry.data.value = undefined
      entry.isSuccess.value = false
      entry.isFetching.value = false
    }
    h.collectionMutate.mockClear()
    h.wishlistMutate.mockClear()
  })

  afterEach(() => {
    document.body.innerHTML = ''
    vi.useRealTimers()
  })

  it('rests as one bottom-left badge containing collection and wanted counts', () => {
    const heldAndWanted = mountControl({ quantity: 2, foilQuantity: 1, wishlistSeed: want(4) })
    const trigger = heldAndWanted.get(
      '[aria-label="Edit copies of Booster Box in your collection"]',
    )
    expect(trigger.classes()).toContain('left-1.5')
    expect(trigger.classes()).toContain('bottom-1.5')
    expect(heldAndWanted.find('[aria-label="3 total"]').exists()).toBe(true)
    const wantedChip = heldAndWanted.get('[aria-label="4 wanted"]')
    expect(wantedChip.findComponent(Heart).exists()).toBe(true)
    heldAndWanted.unmount()

    // A wish-listed but unowned product rests as the Heart, not a second corner button.
    const wantedOnly = mountControl({ wishlistSeed: want(2) })
    expect(wantedOnly.find('[aria-label="2 wanted"]').exists()).toBe(true)
    expect(wantedOnly.find('[aria-label="Add Booster Box to your collection"]').exists()).toBe(true)
    wantedOnly.unmount()

    const untouched = mountControl()
    expect(untouched.find('[aria-label="Add Booster Box to your collection"]').exists()).toBe(true)
    expect(untouched.find('[aria-label="2 wanted"]').exists()).toBe(false)
    untouched.unmount()
  })

  it('opens both list editors and keeps each disabled until its own query settles', async () => {
    const wrapper = mountControl({ quantity: 2, wishlistSeed: want(3) })
    expect(h.collectionOpts.enabled!.value).toBe(false)
    expect(h.wishlistOpts.enabled!.value).toBe(false)

    await openPopover(wrapper, 'Edit copies of Booster Box in your collection')

    expect(h.collectionOpts.enabled!.value).toBe(true)
    expect(h.wishlistOpts.enabled!.value).toBe(true)
    expect(document.body.textContent).toContain('Collection')
    expect(document.body.textContent).toContain('Wish list')
    expect(byLabel(ADD_COLLECTION)!.disabled).toBe(true)
    expect(byLabel(REMOVE_COLLECTION)!.disabled).toBe(true)
    expect(byLabel(ADD_WISHLIST)!.disabled).toBe(true)
    expect(byLabel(REMOVE_WISHLIST)!.disabled).toBe(true)

    h.collectionEntry.data.value = { quantity: 2, foil_quantity: 0 }
    h.collectionEntry.isSuccess.value = true
    await flush()
    expect(byLabel(ADD_COLLECTION)!.disabled).toBe(false)
    expect(byLabel(ADD_WISHLIST)!.disabled).toBe(true)

    h.wishlistEntry.data.value = { quantity: 3, foil_quantity: 0 }
    h.wishlistEntry.isSuccess.value = true
    await flush()
    expect(byLabel(ADD_WISHLIST)!.disabled).toBe(false)

    wrapper.unmount()
  })

  it('saves each list independently and preserves both hidden foil counts', async () => {
    const wrapper = mountControl({ quantity: 1, foilQuantity: 9, wishlistSeed: want(2) })
    await openPopover(wrapper, 'Edit copies of Booster Box in your collection')

    h.collectionEntry.data.value = { quantity: 5, foil_quantity: 2 }
    h.collectionEntry.isSuccess.value = true
    h.wishlistEntry.data.value = { quantity: 7, foil_quantity: 1 }
    h.wishlistEntry.isSuccess.value = true
    await flush()

    await byLabel(ADD_COLLECTION)!.click()
    await byLabel(ADD_WISHLIST)!.click()
    await flush(350)

    expect(h.collectionMutate).toHaveBeenCalledTimes(1)
    expect(h.collectionMutate).toHaveBeenCalledWith({
      game: 'mtg',
      id: '100',
      quantity: 6,
      foil_quantity: 2,
    })
    expect(h.wishlistMutate).toHaveBeenCalledTimes(1)
    expect(h.wishlistMutate).toHaveBeenCalledWith({
      game: 'mtg',
      id: '100',
      quantity: 8,
      foil_quantity: 1,
    })

    wrapper.unmount()
  })

  it('seeds the wish-list row display from wishlistSeed so the want shows at once on open', async () => {
    // Bug parity with OwnedCountControl: the row used to flash "0" until the authoritative
    // single-product want (staleTime 0) landed. With `wishlistSeed` it shows the resting want
    // the instant the popover opens; the stepper stays disabled until the fetch settles, so the
    // seed can never drive an absolute-count save.
    const wrapper = mountControl({ wishlistSeed: want(3) })
    // Both entry queries are unresolved (beforeEach).
    await openPopover(wrapper, 'Add Booster Box to your collection')

    // The wish row already reads the seeded want, not 0...
    expect(byLabel('Wish list: 3')).not.toBeNull()
    // ...while its stepper stays disabled until the authoritative want lands.
    expect(byLabel(ADD_WISHLIST)!.disabled).toBe(true)

    wrapper.unmount()
  })
})
