import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, ref, type Ref } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createPinia, setActivePinia } from 'pinia'
import { useOwnedCountEditor, type OwnedCountSeed } from '@/composables/useOwnedCountEditor'
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

function putBodies() {
  return fetchMock.mock.calls
    .filter((call) => (call[1]?.method ?? 'GET') === 'PUT')
    .map((call) => JSON.parse(call[1]!.body as string) as OwnedCountSeed)
}

function mountEditor(seed: Ref<OwnedCountSeed | undefined>) {
  const pinia = createPinia()
  setActivePinia(pinia)
  useAuthStore().accessToken = 'test-token'
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const game = ref('mtg')
  const cardId = ref('card-a')
  const host = defineComponent({
    setup: () => useOwnedCountEditor(game, cardId, seed),
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

  it('never goes below zero', async () => {
    const seed = ref<OwnedCountSeed | undefined>({ quantity: 0, foil_quantity: 0 })
    const editor = mountEditor(seed)
    await flushPromises()
    editor.adjust('quantity', -1)
    expect(editor.regular).toBe(0)
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
})
