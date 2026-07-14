import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, ref, type Ref } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createPinia, setActivePinia } from 'pinia'
import {
  useOwnedCountEditor,
  type CardListTarget,
  type OwnedCountSeed,
} from '@/composables/useOwnedCountEditor'
import { useAuthStore } from '@/stores/auth'

type FetchInit = { method?: string; body?: string }
type FetchStub = (
  url: string,
  init?: FetchInit,
) => Promise<{
  ok: boolean
  status: number
  text: () => Promise<string>
}>

const makeFetch = () =>
  vi.fn<FetchStub>(async (_url, init) => ({
    ok: true,
    status: 200,
    text: async () => init?.body ?? '{"quantity":0,"foil_quantity":0}',
  }))

// The editor writes through useSetCollectionEntryMutation, which PUTs to the collection
// API. Stub fetch so the save resolves without a network, and record the PUT bodies so we
// can assert what was actually sent (the debounce should collapse rapid clicks into one
// PUT of the final absolute counts).
let fetchMock: ReturnType<typeof makeFetch>
const mounted: Array<ReturnType<typeof mount>> = []

function putCalls() {
  return fetchMock.mock.calls
    .filter((call) => (call[1]?.method ?? 'GET') === 'PUT')
    .map((call) => ({
      url: call[0] as string,
      body: JSON.parse(call[1]!.body as string) as OwnedCountSeed,
    }))
}

function putBodies() {
  return putCalls().map((call) => call.body)
}

function mountEditor(
  seed: Ref<OwnedCountSeed | undefined>,
  cardId: Ref<string> = ref('card-a'),
  list?: CardListTarget,
  kind?: 'card' | 'product',
) {
  const pinia = createPinia()
  setActivePinia(pinia)
  useAuthStore().accessToken = 'test-token'
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const game = ref('mtg')
  const host = defineComponent({
    setup: () => useOwnedCountEditor(game, cardId, seed, { list, kind }),
    render: () => null,
  })
  const wrapper = mount(host, {
    global: { plugins: [pinia, [VueQueryPlugin, { queryClient }]] },
  })
  mounted.push(wrapper)
  // `wrapper.vm` exposes the setup return with refs already unwrapped, so `regular`/`foil`
  // read as plain (reactive) numbers rather than `Ref`s.
  return wrapper.vm as unknown as {
    regular: number
    foil: number
    adjust: (which: 'quantity' | 'foil', delta: number) => void
  }
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 400))

describe('useOwnedCountEditor', () => {
  beforeEach(() => {
    fetchMock = makeFetch()
    vi.stubGlobal('fetch', fetchMock)
  })
  afterEach(async () => {
    // Unmount each host so its onBeforeUnmount flushes any pending debounced save now,
    // rather than leaking a real timer that fires (and records a stray PUT) mid-next-test.
    mounted.forEach((wrapper) => wrapper.unmount())
    mounted.length = 0
    await flushPromises()
    vi.unstubAllGlobals()
  })

  it('collapses rapid adjusts into one absolute-count save', async () => {
    const seed = ref<OwnedCountSeed | undefined>({ quantity: 0, foil_quantity: 0 })
    const editor = mountEditor(seed)
    await flushPromises()
    expect(editor.regular).toBe(0)

    editor.adjust('quantity', 1)
    editor.adjust('quantity', 1)
    editor.adjust('quantity', 1)
    editor.adjust('foil', 1)
    // Local state updates instantly, before any save.
    expect(editor.regular).toBe(3)
    expect(editor.foil).toBe(1)
    expect(putBodies()).toHaveLength(0)

    await settle()
    await flushPromises()

    // One PUT of the final absolute counts, not one per click.
    expect(putBodies()).toEqual([{ quantity: 3, foil_quantity: 1 }])
  })

  it('saves to the collection endpoint by default and the wish list when targeted', async () => {
    // Default target: the collection write.
    const editor = mountEditor(ref<OwnedCountSeed | undefined>({ quantity: 0, foil_quantity: 0 }))
    await flushPromises()
    editor.adjust('quantity', 1)
    await settle()
    await flushPromises()
    expect(putCalls()).toHaveLength(1)
    expect(putCalls()[0]!.url).toContain('/api/collection/mtg/cards/card-a')

    // Explicit wish-list target: the same save shape against the wish-list endpoint.
    const wishlistEditor = mountEditor(
      ref<OwnedCountSeed | undefined>({ quantity: 0, foil_quantity: 0 }),
      ref('card-a'),
      'wishlist',
    )
    await flushPromises()
    wishlistEditor.adjust('quantity', 1)
    await settle()
    await flushPromises()
    const calls = putCalls()
    expect(calls).toHaveLength(2)
    expect(calls[1]!.url).toContain('/api/wishlist/mtg/cards/card-a')
    expect(calls[1]!.body).toEqual({ quantity: 1, foil_quantity: 0 })
  })

  it('never goes below zero and does not save a clamped no-op', async () => {
    const seed = ref<OwnedCountSeed | undefined>({ quantity: 0, foil_quantity: 0 })
    const editor = mountEditor(seed)
    await flushPromises()
    editor.adjust('quantity', -1)
    expect(editor.regular).toBe(0)
    await settle()
    await flushPromises()
    // A clamped no-op must not trigger a redundant PUT.
    expect(putBodies()).toHaveLength(0)
  })

  it('flushes a pending edit against the previous card when the card changes', async () => {
    const seed = ref<OwnedCountSeed | undefined>({ quantity: 0, foil_quantity: 0 })
    const cardId = ref('card-a')
    const editor = mountEditor(seed, cardId)
    await flushPromises()

    // Edit card A, then switch to card B before the debounce fires.
    editor.adjust('quantity', 1)
    cardId.value = 'card-b'
    await settle()
    await flushPromises()

    // The pending edit saved against card A's id (not the newly-selected card B).
    const calls = putCalls()
    expect(calls).toHaveLength(1)
    expect(calls[0]!.body).toEqual({ quantity: 1, foil_quantity: 0 })
    expect(calls[0]!.url).toContain('/cards/card-a')
  })

  it('does not let a background reseed clobber a pending edit', async () => {
    const seed = ref<OwnedCountSeed | undefined>({ quantity: 2, foil_quantity: 0 })
    const editor = mountEditor(seed)
    await flushPromises()
    expect(editor.regular).toBe(2)

    // Local edit in flight (dirty).
    editor.adjust('quantity', 1)
    expect(editor.regular).toBe(3)

    // A background refetch resolves with a different value — must be ignored while dirty.
    seed.value = { quantity: 5, foil_quantity: 0 }
    await flushPromises()
    expect(editor.regular).toBe(3)

    // The debounced save fires with the local (not the refetched) value.
    await settle()
    await flushPromises()
    expect(putBodies()).toEqual([{ quantity: 3, foil_quantity: 0 }])

    // Once the edit has settled (clean), a later reseed is applied.
    seed.value = { quantity: 7, foil_quantity: 0 }
    await flushPromises()
    expect(editor.regular).toBe(7)
  })

  it('saves to the wish-list product endpoint when kind is product', async () => {
    const editor = mountEditor(
      ref<OwnedCountSeed | undefined>({ quantity: 0, foil_quantity: 0 }),
      ref('prod-1'),
      undefined,
      'product',
    )
    await flushPromises()
    editor.adjust('quantity', 1)
    await settle()
    await flushPromises()

    // The write targets the wish list's sealed-product route with an absolute count body.
    const calls = putCalls()
    expect(calls).toHaveLength(1)
    expect(calls[0]!.url).toContain('/api/wishlist/mtg/products/prod-1')
    expect(calls[0]!.body).toEqual({ quantity: 1, foil_quantity: 0 })
  })

  it('collapses rapid product adjusts into one PUT of the final count', async () => {
    const editor = mountEditor(
      ref<OwnedCountSeed | undefined>({ quantity: 0, foil_quantity: 0 }),
      ref('prod-1'),
      undefined,
      'product',
    )
    await flushPromises()
    editor.adjust('quantity', 1)
    editor.adjust('quantity', 1)
    editor.adjust('quantity', 1)
    expect(editor.regular).toBe(3)
    await settle()
    await flushPromises()

    // Debounced to a single PUT of the final absolute value, product route.
    const calls = putCalls()
    expect(calls).toHaveLength(1)
    expect(calls[0]!.url).toContain('/api/wishlist/mtg/products/prod-1')
    expect(calls[0]!.body).toEqual({ quantity: 3, foil_quantity: 0 })
  })

  it('preserves a seeded foil count on a product quantity save', async () => {
    const editor = mountEditor(
      ref<OwnedCountSeed | undefined>({ quantity: 2, foil_quantity: 5 }),
      ref('prod-1'),
      undefined,
      'product',
    )
    await flushPromises()
    expect(editor.regular).toBe(2)
    expect(editor.foil).toBe(5)
    editor.adjust('quantity', 1)
    await settle()
    await flushPromises()

    // Only the regular row is user-editable in product mode; the seeded foil count rides
    // along unchanged rather than being clobbered to 0.
    const calls = putCalls()
    expect(calls).toHaveLength(1)
    expect(calls[0]!.url).toContain('/api/wishlist/mtg/products/prod-1')
    expect(calls[0]!.body).toEqual({ quantity: 3, foil_quantity: 5 })
  })

  it('issues a zero-count PUT to remove a product from the wish list', async () => {
    const editor = mountEditor(
      ref<OwnedCountSeed | undefined>({ quantity: 1, foil_quantity: 0 }),
      ref('prod-1'),
      undefined,
      'product',
    )
    await flushPromises()
    expect(editor.regular).toBe(1)
    editor.adjust('quantity', -1)
    expect(editor.regular).toBe(0)
    await settle()
    await flushPromises()

    // Both-zero is the delete signal; the PUT still carries the zeros.
    const calls = putCalls()
    expect(calls).toHaveLength(1)
    expect(calls[0]!.url).toContain('/api/wishlist/mtg/products/prod-1')
    expect(calls[0]!.body).toEqual({ quantity: 0, foil_quantity: 0 })
  })

  it('routes saves through an injected saveFn (the deck path), not the list mutation', async () => {
    // Decks reuse this editor via an injected `saveFn` (they write a (deck, section, card)
    // row through their own mutation). Verify the debounce/collapse still applies and that
    // NO collection/wish-list PUT is issued.
    const pinia = createPinia()
    setActivePinia(pinia)
    useAuthStore().accessToken = 'test-token'
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const game = ref('mtg')
    const cardId = ref('card-a')
    const seed = ref<OwnedCountSeed | undefined>({ quantity: 0, foil_quantity: 0 })
    const saves: Array<[string, number, number]> = []
    const saveFn = vi.fn<(id: string, q: number, f: number) => Promise<unknown>>(
      async (id, q, f) => {
        saves.push([id, q, f])
        return {}
      },
    )
    const host = defineComponent({
      setup: () => useOwnedCountEditor(game, cardId, seed, { saveFn }),
      render: () => null,
    })
    const wrapper = mount(host, {
      global: { plugins: [pinia, [VueQueryPlugin, { queryClient }]] },
    })
    mounted.push(wrapper)
    const editor = wrapper.vm as unknown as {
      adjust: (which: 'quantity' | 'foil', delta: number) => void
    }
    await flushPromises()
    editor.adjust('quantity', 1)
    editor.adjust('quantity', 1)
    await settle()
    await flushPromises()

    // One collapsed call to the injected writer with the final absolute count; no list PUT.
    expect(saveFn).toHaveBeenCalledTimes(1)
    expect(saves).toEqual([['card-a', 2, 0]])
    expect(putBodies()).toHaveLength(0)
  })
})
