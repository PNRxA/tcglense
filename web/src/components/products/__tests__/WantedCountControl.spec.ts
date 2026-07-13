import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from 'vitest'
import { nextTick, type Ref } from 'vue'
import { mount, type VueWrapper } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createPinia, setActivePinia } from 'pinia'

// WantedCountControl is the product twin of OwnedCountControl. This spec drives it through
// the REAL useOwnedCountEditor (kind: 'product'), mocking only at the composable seam: the
// wanted-count entry query (controllable ready state) and the set-product-entry mutation (a
// spied `mutateAsync`). The collection mutation is mocked too because useOwnedCountEditor
// imports it statically even though product mode never calls it.
type Counts = { quantity: number; foil_quantity: number }
type MutateFn = (vars: { game: string; id: string } & Counts) => Promise<Counts>
type EntryMock = {
  data: Ref<Counts | undefined>
  isSuccess: Ref<boolean>
  isFetching: Ref<boolean>
}
type EntryOpts = { enabled?: Ref<boolean>; staleTime?: number }

const h = vi.hoisted(() => ({
  entry: null as unknown as EntryMock,
  entryOpts: null as unknown as EntryOpts,
  productMutate: null as unknown as Mock<MutateFn>,
}))

vi.mock('@/composables/useWishlist', async () => {
  const { ref } = await import('vue')
  h.entry = {
    data: ref<Counts | undefined>(undefined),
    isSuccess: ref(false),
    isFetching: ref(false),
  }
  h.productMutate = vi.fn<MutateFn>().mockResolvedValue({ quantity: 0, foil_quantity: 0 })
  return {
    // Capture the caller's options so the spec can assert `enabled` tracks the popover
    // (open) state, not just stub the query result.
    useWishlistProductEntryQuery: (_game: Ref<string>, _id: Ref<string>, opts: EntryOpts) => {
      h.entryOpts = opts
      return h.entry
    },
    useSetWishlistProductEntryMutation: () => ({
      mutateAsync: h.productMutate,
      isPending: ref(false),
    }),
    // Imported by useOwnedCountEditor's static import list; never called in product mode.
    useSetWishlistEntryMutation: () => ({ mutateAsync: vi.fn<MutateFn>(), isPending: ref(false) }),
  }
})

vi.mock('@/composables/useCollection', async () => {
  const { ref } = await import('vue')
  return {
    // Imported by useOwnedCountEditor's static import list; never called in product mode.
    useSetCollectionEntryMutation: () => ({
      mutateAsync: vi.fn<MutateFn>(),
      isPending: ref(false),
    }),
  }
})

import WantedCountControl from '../WantedCountControl.vue'

function mountControl(props: { quantity?: number; foilQuantity?: number } = {}) {
  const pinia = createPinia()
  setActivePinia(pinia)
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return mount(WantedCountControl, {
    attachTo: document.body,
    props: {
      game: 'mtg',
      productId: '100',
      name: 'Booster Box',
      quantity: props.quantity ?? 0,
      foilQuantity: props.foilQuantity ?? 0,
    },
    global: { plugins: [pinia, [VueQueryPlugin, { queryClient }]] },
  })
}

// reka teleports the popover content to <body>, so reach its controls through the document.
function byLabel(label: string): HTMLButtonElement | null {
  return document.body.querySelector(`[aria-label="${label}"]`)
}

// `flushPromises` schedules via setImmediate, which the fake timers fake, so flush by
// advancing them instead (also runs the editor's 350 ms debounce when `ms` covers it).
async function flush(ms = 0) {
  await nextTick()
  await vi.advanceTimersByTimeAsync(ms)
  await nextTick()
}

async function openPopover(wrapper: VueWrapper, triggerLabel: string) {
  await wrapper.find(`[aria-label="${triggerLabel}"]`).trigger('click')
  await flush()
}

const ADD = 'Add one Booster Box to your wish list'
const REMOVE = 'Remove one Booster Box from your wish list'

describe('WantedCountControl (issue #364)', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    h.entry.data.value = undefined
    h.entry.isSuccess.value = false
    h.entry.isFetching.value = false
    h.productMutate.mockClear()
  })

  afterEach(() => {
    document.body.innerHTML = ''
    vi.useRealTimers()
  })

  it('rests as a count chip when wanted and a "+" trigger when not', () => {
    const wanted = mountControl({ quantity: 2, foilQuantity: 1 })
    // The trigger carries the OwnedCountBadge total chip and the edit-worded label.
    expect(wanted.find('[aria-label="3 total"]').exists()).toBe(true)
    expect(
      wanted.find('[aria-label="Edit copies of Booster Box in your wish list"]').exists(),
    ).toBe(true)
    wanted.unmount()

    const none = mountControl({ quantity: 0, foilQuantity: 0 })
    expect(none.find('[aria-label="Add Booster Box to your wish list"]').exists()).toBe(true)
    expect(none.find('[aria-label="3 total"]').exists()).toBe(false)
    none.unmount()
  })

  it('keeps steppers disabled until the want settles and shows one quantity row', async () => {
    const wrapper = mountControl({ quantity: 2, foilQuantity: 1 })

    // Fetches the authoritative want only while the popover is open (a big grid must not
    // fire one request per resting tile).
    expect(h.entryOpts.enabled!.value).toBe(false)

    await openPopover(wrapper, 'Edit copies of Booster Box in your wish list')

    expect(h.entryOpts.enabled!.value).toBe(true)

    // Unresolved entry (isSuccess false) → both steppers disabled.
    expect(byLabel(ADD)).not.toBeNull()
    expect(byLabel(ADD)!.disabled).toBe(true)
    expect(byLabel(REMOVE)!.disabled).toBe(true)

    // Exactly one quantity row — no Foil / Regular counterparts.
    expect(document.body.textContent).toContain('Quantity')
    expect(document.body.textContent).not.toContain('Foil')
    expect(document.body.textContent).not.toContain('Regular')

    // The authoritative want resolves; the add stepper unlocks.
    h.entry.data.value = { quantity: 2, foil_quantity: 1 }
    h.entry.isSuccess.value = true
    h.entry.isFetching.value = false
    await flush()
    expect(byLabel(ADD)!.disabled).toBe(false)

    wrapper.unmount()
  })

  it('saves the absolute count and preserves the seeded foil want', async () => {
    // These props are just the grid's stale display counts; the entry query resolved below
    // is deliberately different, so the editor must seed from it, not from these props.
    const wrapper = mountControl({ quantity: 2, foilQuantity: 1 })
    await openPopover(wrapper, 'Edit copies of Booster Box in your wish list')

    // Resolve the want to 5 regular + 0 foil — distinct from the display props above, so a
    // regression seeding from the stale grid map instead of this query would fail below.
    h.entry.data.value = { quantity: 5, foil_quantity: 0 }
    h.entry.isSuccess.value = true
    h.entry.isFetching.value = false
    await flush()

    await byLabel(ADD)!.click()
    await flush(350)

    // One write of the absolute counts: regular bumped to 6, foil preserved at 0.
    expect(h.productMutate).toHaveBeenCalledTimes(1)
    expect(h.productMutate).toHaveBeenCalledWith({
      game: 'mtg',
      id: '100',
      quantity: 6,
      foil_quantity: 0,
    })

    wrapper.unmount()
  })
})
