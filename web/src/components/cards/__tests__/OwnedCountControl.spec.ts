import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from 'vitest'
import { nextTick, type Ref } from 'vue'
import { mount, type VueWrapper } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createPinia, setActivePinia } from 'pinia'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { CardListTarget } from '@/composables/useOwnedCountEditor'

// This is the control's first dedicated spec. It drives the popover's new "Wish list" row
// through the REAL useOwnedCountEditor (so the absolute-count/debounce plumbing is exercised
// end to end), mocking only at the composable seam: the entry queries (controllable ready
// state) and the set-entry mutations (spied `mutateAsync`). Both the collection editor (the
// existing rows) and the wish-list editor (the new row) run their real logic; the two spies
// prove a wish-list add writes ONLY the wish list.
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
  collectionMutate: null as unknown as Mock<MutateFn>,
  wishEntry: null as unknown as EntryMock,
  wishEntryOpts: [] as EntryOpts[],
  wishMutate: null as unknown as Mock<MutateFn>,
}))

vi.mock('@/composables/useCollection', async () => {
  const { ref } = await import('vue')
  h.collectionEntry = {
    data: ref<Counts | undefined>({ quantity: 0, foil_quantity: 0 }),
    isSuccess: ref(true),
    isFetching: ref(false),
  }
  h.collectionMutate = vi.fn<MutateFn>().mockResolvedValue({ quantity: 0, foil_quantity: 0 })
  return {
    useCollectionEntryQuery: () => h.collectionEntry,
    useSetCollectionEntryMutation: () => ({
      mutateAsync: h.collectionMutate,
      isPending: ref(false),
    }),
  }
})

vi.mock('@/composables/useWishlist', async () => {
  const { ref } = await import('vue')
  h.wishEntry = {
    data: ref<Counts | undefined>(undefined),
    isSuccess: ref(false),
    isFetching: ref(false),
  }
  h.wishMutate = vi.fn<MutateFn>().mockResolvedValue({ quantity: 0, foil_quantity: 0 })
  return {
    // Capture each caller's options so the spec can assert the lazy `enabled` gate, not
    // just stub the result. Setup order matters for the indices: a wishlist-targeting
    // instance creates the popover's primary query first, then the row's hook; a
    // collection-targeting one creates only the row's hook.
    useWishlistEntryQuery: (_game: Ref<string>, _id: Ref<string>, opts: EntryOpts) => {
      h.wishEntryOpts.push(opts)
      return h.wishEntry
    },
    useSetWishlistEntryMutation: () => ({ mutateAsync: h.wishMutate, isPending: ref(false) }),
    // useOwnedCountEditor imports this even though the card editor never uses it; the mock
    // module must export it or the editor's static import resolves to undefined.
    useSetWishlistProductEntryMutation: () => ({
      mutateAsync: vi.fn<() => Promise<Counts>>(),
      isPending: ref(false),
    }),
  }
})

import OwnedCountControl from '../OwnedCountControl.vue'

function mountControl(
  props: {
    list?: CardListTarget
    quantity?: number
    foilQuantity?: number
    wishlistQuantity?: number
  } = {},
) {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/', component: { template: '<div />' } }],
  })
  const pinia = createPinia()
  setActivePinia(pinia)
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return mount(OwnedCountControl, {
    attachTo: document.body,
    props: {
      game: 'mtg',
      cardId: 'c1',
      name: 'Card c1',
      quantity: props.quantity ?? 0,
      foilQuantity: props.foilQuantity ?? 0,
      list: props.list ?? 'collection',
      wishlistQuantity: props.wishlistQuantity ?? 0,
    },
    global: { plugins: [router, pinia, [VueQueryPlugin, { queryClient }]] },
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

describe('OwnedCountControl wish-list quick-add row (issue #364)', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    h.collectionEntry.data.value = { quantity: 0, foil_quantity: 0 }
    h.collectionEntry.isSuccess.value = true
    h.collectionEntry.isFetching.value = false
    h.collectionMutate.mockClear()
    h.wishEntry.data.value = undefined
    h.wishEntry.isSuccess.value = false
    h.wishEntry.isFetching.value = false
    h.wishEntryOpts = []
    h.wishMutate.mockClear()
  })

  afterEach(() => {
    document.body.innerHTML = ''
    vi.useRealTimers()
  })

  it('adds to the wish list from a collection-targeting control, not the collection', async () => {
    const wrapper = mountControl({ list: 'collection' })

    // Lazy gate: the row's wish-entry hook exists from setup but stays disabled while the
    // popover is closed — a big grid must not fire one wish-list request per resting tile.
    expect(h.wishEntryOpts).toHaveLength(1)
    expect(h.wishEntryOpts[0]!.enabled!.value).toBe(false)

    await openPopover(wrapper, 'Add Card c1 to your collection')
    expect(h.wishEntryOpts[0]!.enabled!.value).toBe(true)

    const addWish = 'Add one copy of Card c1 to your wish list'
    // The row is present but its steppers are disabled while the authoritative want is
    // unresolved — an early click must never save an adjustment off a stale zero.
    expect(byLabel(addWish)).not.toBeNull()
    expect(byLabel(addWish)!.disabled).toBe(true)

    // The single-card want resolves to counts distinct from the display props (0/0) and
    // the collection entry (0/0), so a regression seeding the row from either — or one
    // clobbering the foil want — would fail the write assertion below.
    h.wishEntry.data.value = { quantity: 4, foil_quantity: 2 }
    h.wishEntry.isSuccess.value = true
    h.wishEntry.isFetching.value = false
    await flush()
    expect(byLabel(addWish)!.disabled).toBe(false)

    await byLabel(addWish)!.click()
    await flush(350)

    // Exactly one wish-list write of the absolute counts, seeded from the wish entry:
    // regular bumped to 5, the untouched foil want preserved. The collection is untouched.
    expect(h.wishMutate).toHaveBeenCalledTimes(1)
    expect(h.wishMutate).toHaveBeenCalledWith({
      game: 'mtg',
      id: 'c1',
      quantity: 5,
      foil_quantity: 2,
    })
    expect(h.collectionMutate).not.toHaveBeenCalled()

    wrapper.unmount()
  })

  it('renders no wish-list row on a wishlist-targeting control (the popover already is it)', async () => {
    const wrapper = mountControl({ list: 'wishlist' })

    // Setup creates two wishlist entry hooks — the popover's primary query, then the
    // row's unconditional hook (composables can't be conditional) — both disabled at rest.
    expect(h.wishEntryOpts).toHaveLength(2)
    expect(h.wishEntryOpts[0]!.enabled!.value).toBe(false)
    expect(h.wishEntryOpts[1]!.enabled!.value).toBe(false)

    await openPopover(wrapper, 'Add Card c1 to your wish list')

    // Opening enables only the primary query; the self-suppressed row's hook never turns on.
    expect(h.wishEntryOpts[0]!.enabled!.value).toBe(true)
    expect(h.wishEntryOpts[1]!.enabled!.value).toBe(false)

    // The Regular/Foil rows render as normal...
    expect(byLabel('Add one regular copy of Card c1')).not.toBeNull()
    expect(byLabel('Add one foil copy of Card c1')).not.toBeNull()
    // ...but the secondary "Wish list" row (and its list-worded stepper) does not.
    expect(byLabel('Add one copy of Card c1 to your wish list')).toBeNull()
    expect(document.body.textContent).not.toContain('Wish list')

    wrapper.unmount()
  })

  it('adds to the collection from the regular row, not the wish list', async () => {
    const wrapper = mountControl({ list: 'collection' })
    await openPopover(wrapper, 'Add Card c1 to your collection')

    // The collection entry is already resolved (beforeEach), so the Regular stepper is live.
    await byLabel('Add one regular copy of Card c1')!.click()
    await flush(350)

    // Exactly one collection write of the absolute counts; the wish list is untouched.
    expect(h.collectionMutate).toHaveBeenCalledTimes(1)
    expect(h.collectionMutate).toHaveBeenCalledWith({
      game: 'mtg',
      id: 'c1',
      quantity: 1,
      foil_quantity: 0,
    })
    expect(h.wishMutate).not.toHaveBeenCalled()

    wrapper.unmount()
  })

  it('shows a failed wish save on the row status, not the collection header', async () => {
    const wrapper = mountControl({ list: 'collection' })
    await openPopover(wrapper, 'Add Card c1 to your collection')

    h.wishEntry.data.value = { quantity: 0, foil_quantity: 0 }
    h.wishEntry.isSuccess.value = true
    h.wishEntry.isFetching.value = false
    await flush()

    h.wishMutate.mockRejectedValueOnce(new Error('boom'))
    await byLabel('Add one copy of Card c1 to your wish list')!.click()
    await flush(350)

    // The row carries its own error state, and it's the only destructive message — the
    // header (the collection editor's status) must not report a wish-list failure.
    const errors = Array.from(document.body.querySelectorAll('.text-destructive')).map((el) =>
      el.textContent?.trim(),
    )
    expect(errors).toEqual(['Retry — not saved'])

    // A later successful collection save reports Saved in the header while the row's
    // sticky error stays put — a merged status region would pin Retry over both.
    await byLabel('Add one regular copy of Card c1')!.click()
    await flush(350)
    expect(document.body.textContent).toContain('Saved')
    expect(document.body.textContent).toContain('Retry — not saved')

    wrapper.unmount()
  })

  it('pins the resting trigger wording for a collection control', () => {
    const unowned = mountControl({ list: 'collection', quantity: 0, foilQuantity: 0 })
    expect(unowned.find('[aria-label="Add Card c1 to your collection"]').exists()).toBe(true)
    unowned.unmount()

    const owned = mountControl({ list: 'collection', quantity: 2, foilQuantity: 1 })
    expect(owned.find('[aria-label="Edit copies of Card c1 in your collection"]').exists()).toBe(
      true,
    )
    owned.unmount()
  })

  it('rests a wanted-but-unowned collection control as a heart with an add label', () => {
    const wrapper = mountControl({
      list: 'collection',
      quantity: 0,
      foilQuantity: 0,
      wishlistQuantity: 2,
    })
    // The wish-list heart chip shows even though the card is unowned...
    const trigger = wrapper.find('[aria-label="Add Card c1 to your collection"]')
    expect(wrapper.find('[aria-label="2 wanted"]').exists()).toBe(true)
    // ...the trigger still adds to the collection (owned is false)...
    expect(trigger.exists()).toBe(true)
    // ...and it rests VISIBLE — the heart marks it, so the opacity gate opens (no sm:opacity-0
    // hover reveal, unlike a bare "+"). The gate widened from `owned` to `owned || wanted`.
    expect(trigger.classes()).not.toContain('sm:opacity-0')
    wrapper.unmount()

    // Contrast: a truly-untouched (unowned AND unwanted) control keeps the bare "+" hidden
    // until hover from sm up — its trigger DOES carry the sm:opacity-0 gate.
    const bare = mountControl({ list: 'collection', quantity: 0, foilQuantity: 0 })
    expect(bare.find('[aria-label="Add Card c1 to your collection"]').classes()).toContain(
      'sm:opacity-0',
    )
    bare.unmount()
  })
})
