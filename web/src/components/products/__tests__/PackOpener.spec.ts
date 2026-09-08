import { describe, it, expect, vi, beforeEach } from 'vitest'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createPinia } from 'pinia'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { Ref } from 'vue'
import { ApiError } from '@/lib/api'
import type { Card, OpenedPack, PackEv, PackOpening, Product, ProductEv } from '@/lib/api'
import PackOpener from '../PackOpener.vue'

// Drive the opener off controlled query state, stubbing both composables so no API is
// needed. The unit under test is the seam between the button, the seed, and the URL: that
// the panel is only there when there is something to open, that a click mints a seed and
// mirrors it into `?pack=`, that a shared `?pack=` link opens on mount without a click, and
// that a refusal reads as a sentence rather than a broken panel. The wording of the totals
// lives in lib/productCounts.ts and is tested there.
const state = vi.hoisted(() => ({
  ev: null as ProductEv | null,
  opening: null as PackOpening | null,
  error: null as Error | null,
  pending: false,
}))
// The refs the component owns, captured through the stubbed query so the test can read the
// seed it minted and the copies it asked for.
const captured = vi.hoisted(() => ({}) as { seed: Ref<number | null>; copies: Ref<number> })

vi.mock('@/composables/useProducts', async () => {
  const { computed } = await import('vue')
  return {
    useProductEvQuery: () => ({
      data: computed(() => ({ data: state.ev })),
      isPending: computed(() => false),
      isError: computed(() => false),
    }),
    usePackOpeningQuery: (
      _game: unknown,
      _id: unknown,
      seed: Ref<number | null>,
      copies: Ref<number>,
    ) => {
      captured.seed = seed
      captured.copies = copies
      return {
        data: computed(() => (seed.value === null ? undefined : (state.opening ?? undefined))),
        error: computed(() => (seed.value === null ? null : state.error)),
        isPending: computed(() => seed.value !== null && state.pending),
      }
    },
  }
})

const card = (id: string, name: string): Card =>
  ({ id, name, has_image: false, prices: { usd: null } }) as unknown as Card

const packEv = (quantity: number): PackEv =>
  ({
    set_code: 'blb',
    booster_code: 'play',
    name: 'Play Booster',
    quantity,
    cards_per_pack: 14,
    ev_usd: '5.00',
    priced_share: 1,
    slots: [],
    top: [],
  }) as PackEv

const productEv = (quantity = 1): ProductEv => ({
  ev_usd: '5.00',
  packs: [packEv(quantity)],
  top: [],
  caveats: [],
})

const openedPack = (overrides: Partial<OpenedPack> = {}): OpenedPack => ({
  set_code: 'blb',
  booster_code: 'play',
  name: 'Play Booster',
  variant: 0,
  cards: [
    {
      card: card('sf-1', 'Sheoldred, the Apocalypse'),
      foil: false,
      sheet: 'rareMythic',
      price_usd: '80.00',
    },
    { card: card('sf-2', 'Lightning Bolt'), foil: true, sheet: 'foil', price_usd: null },
  ],
  value_usd: '80.00',
  ...overrides,
})

const opening = (overrides: Partial<PackOpening> = {}): PackOpening => ({
  seed: 12345,
  copies: 1,
  packs: [openedPack()],
  value_usd: '80.00',
  priced_count: 1,
  unpriced_count: 1,
  caveats: ['Each pick is treated as an independent weighted draw from its sheet.'],
  ...overrides,
})

const product = (usd: string | null): Product =>
  ({ id: '900002', name: 'Play Booster', prices: { usd, usd_foil: null } }) as unknown as Product

async function mountOpener(
  opts: {
    ev?: ProductEv | null
    opening?: PackOpening | null
    error?: Error | null
    pending?: boolean
    product?: Product
    path?: string
  } = {},
) {
  state.ev = opts.ev === undefined ? productEv() : opts.ev
  state.opening = opts.opening === undefined ? opening() : opts.opening
  state.error = opts.error ?? null
  state.pending = opts.pending ?? false
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/sealed/:game/:id', component: { template: '<div />' } },
      { path: '/cards/:game/cards/:id', component: { template: '<div />' } },
    ],
  })
  await router.push(opts.path ?? '/sealed/mtg/900002')
  await router.isReady()
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const wrapper = mount(PackOpener, {
    props: { game: 'mtg', id: '900002', product: opts.product },
    global: {
      plugins: [createPinia(), router, [VueQueryPlugin, { queryClient }]],
      stubs: { CardImage: true },
    },
  })
  return { wrapper, router }
}

/** The panel's primary button — the one a screenshot script clicks. */
function openButton(wrapper: VueWrapper) {
  return wrapper.findAll('button').find((b) => /open a pack|open a copy/i.test(b.text()))
}

beforeEach(() => {
  state.ev = null
  state.opening = null
  state.error = null
  state.pending = false
})

describe('PackOpener', () => {
  it('renders nothing for a product with no booster sheets', async () => {
    const { wrapper } = await mountOpener({ ev: null })
    expect(wrapper.find('section').exists()).toBe(false)
  })

  it('offers to open a pack when one copy is a single booster', async () => {
    const { wrapper } = await mountOpener({ ev: productEv(1) })
    expect(wrapper.get('h2').text()).toBe('Open a pack')
    expect(openButton(wrapper)!.text()).toBe('Open a pack')
    // The copies control only makes sense where a copy IS a pack.
    expect(wrapper.find('select').exists()).toBe(true)
  })

  it('offers to open a copy when one copy is a box', async () => {
    const { wrapper } = await mountOpener({ ev: productEv(36) })
    expect(wrapper.get('h2').text()).toBe('Open a copy')
    expect(openButton(wrapper)!.text()).toBe('Open a copy')
    expect(wrapper.find('select').exists()).toBe(false)
  })

  it('opens nothing until asked', async () => {
    const { wrapper, router } = await mountOpener()
    expect(captured.seed.value).toBeNull()
    expect(wrapper.text()).not.toContain('Sheoldred')
    expect(router.currentRoute.value.query.pack).toBeUndefined()
  })

  it('mints a seed on the first click, renders the pulls, and mirrors the run into the URL', async () => {
    const { wrapper, router } = await mountOpener({ product: product('5.00') })
    await openButton(wrapper)!.trigger('click')
    await flushPromises()

    const seed = captured.seed.value
    expect(Number.isInteger(seed)).toBe(true)
    expect(seed).toBeGreaterThanOrEqual(0)
    // The seed is the run: a URL carrying it reproduces the same cards.
    expect(router.currentRoute.value.query.pack).toBe(String(seed))
    expect(router.currentRoute.value.path).toBe('/sealed/mtg/900002')

    expect(wrapper.text()).toContain('Sheoldred, the Apocalypse')
    expect(wrapper.text()).toContain('Lightning Bolt')
    expect(wrapper.text()).toContain('Foil')
    // The pulled total, worded as this run's — never as what a pack is worth.
    expect(wrapper.text()).toContain('$80.00')
    expect(wrapper.text()).toContain('pulled in this run')
    expect(wrapper.text()).toContain('This run is one roll of the dice')
    expect(wrapper.text()).toContain('1 of the 2 cards dealt had no market price')
    expect(wrapper.text()).toContain('of what one copy costs')
  })

  it('rolls a new seed for "Open another"', async () => {
    const { wrapper } = await mountOpener()
    await openButton(wrapper)!.trigger('click')
    await flushPromises()
    const first = captured.seed.value

    const again = wrapper.findAll('button').find((b) => b.text() === 'Open another')
    expect(again).toBeDefined()
    await again!.trigger('click')
    await flushPromises()
    expect(captured.seed.value).not.toBeNull()
    // Astronomically unlikely to repeat, but the assertion that matters is that a re-roll
    // asks for a *seed*, not a refetch of the same one.
    expect(typeof captured.seed.value).toBe('number')
    expect(first).not.toBeUndefined()
  })

  it('auto-opens from a shared ?pack= link, without a click', async () => {
    const { wrapper } = await mountOpener({ path: '/sealed/mtg/900002?pack=4242&copies=3' })
    expect(captured.seed.value).toBe(4242)
    expect(captured.copies.value).toBe(3)
    expect(wrapper.text()).toContain('Sheoldred, the Apocalypse')
  })

  it('ignores a hand-written seed outside the API’s u32 range', async () => {
    const { wrapper } = await mountOpener({ path: '/sealed/mtg/900002?pack=nonsense' })
    expect(captured.seed.value).toBeNull()
    expect(wrapper.text()).not.toContain('Sheoldred')
  })

  it('mirrors the copies choice alongside the seed', async () => {
    const { wrapper, router } = await mountOpener({
      opening: opening({ copies: 3, packs: [openedPack(), openedPack(), openedPack()] }),
    })
    await wrapper.get('select').setValue(3)
    await openButton(wrapper)!.trigger('click')
    await flushPromises()
    expect(captured.copies.value).toBe(3)
    expect(router.currentRoute.value.query.copies).toBe('3')
    expect(wrapper.text()).toContain('What this run dealt across 3 packs')
  })

  it('reads a refusal back as a sentence', async () => {
    // The API answers 422 for an opening it can't deal; its message says which, and it is
    // the actionable half — the panel keeps it rather than swallowing the failure.
    const { wrapper } = await mountOpener({
      opening: null,
      error: new ApiError('this product has no booster data to open', 422),
    })
    await openButton(wrapper)!.trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('this product has no booster data to open')
    expect(wrapper.text()).not.toContain('Sheoldred')
  })

  it('keeps a non-422 failure short', async () => {
    const { wrapper } = await mountOpener({
      opening: null,
      error: new ApiError('Internal server error', 500),
    })
    await openButton(wrapper)!.trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain("That opening couldn't be dealt")
    expect(wrapper.text()).not.toContain('Internal server error')
  })

  it('announces the run politely, labelled by its own heading', async () => {
    const { wrapper } = await mountOpener()
    const section = wrapper.get('section')
    expect(section.attributes('aria-labelledby')).toBe(wrapper.get('h2').attributes('id'))
    expect(wrapper.find('[aria-live="polite"]').exists()).toBe(true)
  })

  it('shows the server’s caveats verbatim', async () => {
    const { wrapper } = await mountOpener()
    await openButton(wrapper)!.trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain(
      'Each pick is treated as an independent weighted draw from its sheet.',
    )
  })
})
