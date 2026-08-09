import { describe, it, expect, beforeEach, vi } from 'vitest'
import { nextTick, ref } from 'vue'
import { mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createPinia } from 'pinia'
import type { Card, OwnedCountsMap } from '@/lib/api'
import { makeCard } from '@/test/fixtures'

// These specs exist for one property: the held-first order the dialog hands the grid must be
// decided once per printing and then STAY PUT. The counts it reads are the same value the
// tiles edit, and that query refetches constantly inside an open dialog (every debounced save
// invalidates it, "Load more" widens its batch key, a window refocus re-runs it at
// staleTime 0) — so anything that re-derived the order from the live map would reshuffle the
// grid under the user's pointer. The picker and the counts hook are mocked as plain refs so a
// test can drive exactly those transitions.
const printings = ref<Card[]>([])
const ownership = ref<OwnedCountsMap>({})
const ready = ref(true)
const fetching = ref(false)

vi.mock('@/composables/usePrintings', () => ({
  usePrintingPicker: () => ({
    filter: ref(''),
    printings,
    filteredPrintings: printings,
    total: ref(0),
    isPending: ref(false),
    failed: ref(false),
    hasNextPage: ref(false),
    isFetchingNextPage: ref(false),
    loadMore: vi.fn<() => Promise<void>>(),
  }),
}))

const counts = () => ({ ownership, ready, fetching })
vi.mock('@/composables/useCollection', () => ({ useOwnedCounts: () => counts() }))
vi.mock('@/composables/useWishlist', () => ({ useWishlistCounts: () => counts() }))

import QuickAddPrintDialog from '../QuickAddPrintDialog.vue'

const CARDS = [
  makeCard('new', { released_at: '2024-01-01' }),
  makeCard('mid', { released_at: '2021-01-01' }),
  makeCard('old', { released_at: '2019-01-01' }),
]
const NEWEST_FIRST = ['new', 'mid', 'old']

const DialogRootStub = { props: ['open'], emits: ['update:open'], template: '<div><slot /></div>' }
const DialogContentStub = {
  name: 'DialogContent',
  emits: ['closeAutoFocus'],
  template: '<div class="dialog-content"><slot /></div>',
}
const PassThrough = { template: '<div><slot /></div>' }
// The tile renders only its printing id, so a spec reads the rendered order straight off it.
const TileStub = {
  name: 'QuickAddPrintTile',
  props: ['game', 'card', 'seed', 'ready', 'list'],
  template: '<span class="pid">{{ card.id }}</span>',
}

function mountDialog(list: 'collection' | 'wishlist' = 'collection') {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return mount(QuickAddPrintDialog, {
    props: { open: true, game: 'mtg', name: 'Dummy Reprinted Relic', list },
    global: {
      plugins: [createPinia(), [VueQueryPlugin, { queryClient }]],
      stubs: {
        Dialog: DialogRootStub,
        DialogContent: DialogContentStub,
        DialogTitle: PassThrough,
        DialogDescription: PassThrough,
        DialogClose: PassThrough,
        QuickAddPrintTile: TileStub,
        CardSearchBox: true,
        CardSortMenu: true,
      },
    },
  })
}

const order = (wrapper: ReturnType<typeof mountDialog>) =>
  wrapper.findAll('.pid').map((n) => n.text())

/** One counts refetch: `fetching` flips true, then settles on `next`. */
async function refetchCounts(next: OwnedCountsMap) {
  fetching.value = true
  await nextTick()
  ownership.value = next
  fetching.value = false
  await nextTick()
}

describe('QuickAddPrintDialog held-first ordering', () => {
  beforeEach(() => {
    printings.value = [...CARDS]
    ownership.value = {}
    ready.value = true
    fetching.value = false
  })

  it('leads with the printings already held, in newest-first order within each group', async () => {
    ownership.value = { old: { quantity: 1, foil_quantity: 0 } }
    const wrapper = mountDialog()
    await nextTick()
    expect(order(wrapper)).toEqual(['old', 'new', 'mid'])
    wrapper.unmount()
  })

  it('keeps the plain order until the counts are authoritative', async () => {
    ready.value = false
    ownership.value = {}
    const wrapper = mountDialog()
    await nextTick()
    // Nothing is decided yet, so the grid must show the order it always showed — never a
    // premature "you hold none of these".
    expect(order(wrapper)).toEqual(NEWEST_FIRST)

    ready.value = true
    ownership.value = { old: { quantity: 2, foil_quantity: 0 } }
    await nextTick()
    expect(order(wrapper)).toEqual(['old', 'new', 'mid'])
    wrapper.unmount()
  })

  it('does not reorder when a counts refetch flips through a fetching window', async () => {
    ownership.value = { old: { quantity: 1, foil_quantity: 0 } }
    const wrapper = mountDialog()
    await nextTick()
    expect(order(wrapper)).toEqual(['old', 'new', 'mid'])

    // The refetch every save/refocus triggers. Mid-flight the map is not authoritative — the
    // order must not collapse back to newest-first for the round trip.
    fetching.value = true
    await nextTick()
    expect(order(wrapper)).toEqual(['old', 'new', 'mid'])

    fetching.value = false
    await nextTick()
    expect(order(wrapper)).toEqual(['old', 'new', 'mid'])
    wrapper.unmount()
  })

  it('leaves a printing where it is after the user adds their first copy of it', async () => {
    ownership.value = { old: { quantity: 1, foil_quantity: 0 } }
    const wrapper = mountDialog()
    await nextTick()
    expect(order(wrapper)).toEqual(['old', 'new', 'mid'])

    // `+` on `mid` (an unheld printing): the save's invalidation refetches the counts, which
    // now report it held. It must NOT teleport to the top — the tile the pointer is on stays
    // under the pointer, so a second `+` hits the same printing.
    await refetchCounts({
      old: { quantity: 1, foil_quantity: 0 },
      mid: { quantity: 1, foil_quantity: 0 },
    })
    expect(order(wrapper)).toEqual(['old', 'new', 'mid'])
    wrapper.unmount()
  })

  it('classifies a newly loaded page without disturbing the printings already shown', async () => {
    ownership.value = { old: { quantity: 1, foil_quantity: 0 } }
    const wrapper = mountDialog()
    await nextTick()

    // "Load more printings": the batch key widens, so the counts query re-runs over the wider
    // set. The new page's held printing joins the held block; the settled ones do not move.
    const extra = makeCard('extra-held', { released_at: '2016-01-01' })
    printings.value = [...CARDS, extra]
    await refetchCounts({
      old: { quantity: 1, foil_quantity: 0 },
      'extra-held': { quantity: 3, foil_quantity: 0 },
    })
    expect(order(wrapper)).toEqual(['old', 'extra-held', 'new', 'mid'])
    wrapper.unmount()
  })

  it('re-snapshots on reopen, so copies added last time lead the next picker', async () => {
    ownership.value = { old: { quantity: 1, foil_quantity: 0 } }
    const wrapper = mountDialog()
    await nextTick()

    await refetchCounts({
      old: { quantity: 1, foil_quantity: 0 },
      mid: { quantity: 1, foil_quantity: 0 },
    })
    expect(order(wrapper)).toEqual(['old', 'new', 'mid'])

    await wrapper.setProps({ open: false })
    await wrapper.setProps({ open: true })
    await nextTick()
    expect(order(wrapper)).toEqual(['mid', 'old', 'new'])
    wrapper.unmount()
  })

  it('reads the wish list when that is the target', async () => {
    ownership.value = { old: { quantity: 0, foil_quantity: 2 } }
    const wrapper = mountDialog('wishlist')
    await nextTick()
    expect(order(wrapper)).toEqual(['old', 'new', 'mid'])
    wrapper.unmount()
  })
})
