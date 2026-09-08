import { beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import DeckPricing from '@/components/decks/DeckPricing.vue'
import type { DeckPricing as DeckPricingPayload, DeckPricingLine } from '@/lib/api'
import { hasSaving, TOP_EXPENSIVE } from '@/lib/deckPricing'
import { makeCard } from '@/test/fixtures'

// The numbers are the server's (issue #672), so what's under test is that the panel keeps
// the response's two honesties — an unpriced value is a dash, never $0.00, and a swap is
// offered only for a row the server stated a saving on — and that both swap paths hand the
// EXISTING printing write exactly the rows it should: the per-row button opens the shared
// printing dialog with the cheapest printing suggested, and "swap all" batches every
// swappable row and nothing else. The public view gets the breakdown and no buttons.

const query = vi.hoisted(() => ({
  pending: false,
  fetching: false,
  loadingError: false,
  refetchError: false,
  data: null as DeckPricingPayload | null,
  authedCalls: 0,
  publicCalls: 0,
}))

const mocks = vi.hoisted(() => ({
  swapAll: vi.fn<(vars: unknown) => Promise<{ swapped: number }>>(),
}))

vi.mock('@/composables/useDeckAnalysis', async () => {
  const { computed } = await import('vue')
  const result = () => ({
    data: computed(() => (query.pending ? undefined : (query.data ?? undefined))),
    isPending: computed(() => query.pending),
    isFetching: computed(() => query.fetching || query.pending),
    isLoadingError: computed(() => query.loadingError),
    isRefetchError: computed(() => query.refetchError),
  })
  return {
    useDeckPricingQuery: () => {
      query.authedCalls += 1
      return result()
    },
    usePublicDeckPricingQuery: () => {
      query.publicCalls += 1
      return result()
    },
  }
})

vi.mock('@/composables/useDecks', async () => {
  const { ref } = await import('vue')
  return {
    useChangeDeckCardPrintingsMutation: () => ({
      mutateAsync: mocks.swapAll,
      isPending: ref(false),
    }),
  }
})

vi.mock('@/composables/useDetailModalLink', () => ({
  useDetailModalLink: () => ({
    hrefFor: (_kind: string, game: string, id: string) => `/cards/${game}/cards/${id}`,
    onActivate: () => {},
    warm: () => {},
  }),
}))

vi.mock('@/composables/useCurrency', () => ({
  useCurrency: () => ({
    formatUsd: (raw: string | null | undefined) => (raw == null ? null : `$${raw}`),
  }),
}))

const PassThrough = defineComponent({ template: '<div><slot /></div>' })
const ButtonStub = defineComponent({
  inheritAttrs: false,
  template: '<button v-bind="$attrs"><slot /></button>',
})
/** The printing dialog is its own spec's subject; here it only has to prove what it was
 * opened with. */
const PrintingDialogStub = defineComponent({
  props: {
    open: Boolean,
    card: { type: Object, required: true },
    sectionId: { type: Number, required: true },
    suggested: { type: Object, default: null },
  },
  template: `
    <div
      data-testid="printing-dialog"
      :data-open="open"
      :data-card="card.id"
      :data-section="sectionId"
      :data-suggested="suggested?.card.id ?? ''"
      :data-suggested-price="suggested?.priceUsd ?? ''"
    />
  `,
})

function line(
  id: string,
  over: Partial<DeckPricingLine> & { cheapestId?: string; cheapestPrice?: string } = {},
): DeckPricingLine {
  const { cheapestId, cheapestPrice, ...rest } = over
  return {
    card: makeCard(id, { name: `Card ${id}`, set_code: 'one', collector_number: id }),
    section_id: 7,
    quantity: 1,
    foil_quantity: 0,
    price_usd: '10.00',
    cheapest: cheapestId
      ? {
          card: makeCard(cheapestId, {
            name: `Card ${id}`,
            set_code: 'two',
            collector_number: cheapestId,
          }),
          price_usd: cheapestPrice ?? '1.00',
        }
      : null,
    saving_usd: cheapestId ? '9.00' : null,
    ...rest,
  }
}

function payload(lines: DeckPricingLine[], over: Partial<DeckPricingPayload> = {}) {
  return {
    lines,
    total_usd: '40.00',
    cheapest_total_usd: '22.00',
    saving_usd: '18.00',
    unpriced_count: 1,
    swappable_count: 2,
    ...over,
  }
}

function mountPanel(props: { handle?: string } = {}) {
  return mount(DeckPricing, {
    props: { game: 'mtg', deckId: 3, ...props },
    global: {
      stubs: {
        Button: ButtonStub,
        Dialog: PassThrough,
        DialogClose: ButtonStub,
        DialogContent: PassThrough,
        DialogDescription: PassThrough,
        DialogTitle: PassThrough,
        DeckPrintingDialog: PrintingDialogStub,
        StaleNotice: defineComponent({ props: ['label'], template: '<p>{{ label }}</p>' }),
        UpdatingCue: defineComponent({ props: ['label'], template: '<span>{{ label }}</span>' }),
        Skeleton: defineComponent({ template: '<div data-testid="skeleton" />' }),
      },
    },
  })
}

async function expand(wrapper: ReturnType<typeof mountPanel>) {
  await wrapper.get('button[aria-expanded]').trigger('click')
}

beforeEach(() => {
  query.pending = false
  query.fetching = false
  query.loadingError = false
  query.refetchError = false
  query.data = null
  query.authedCalls = 0
  query.publicCalls = 0
  mocks.swapAll.mockReset().mockResolvedValue({ swapped: 2 })
})

describe('DeckPricing', () => {
  it('rests on the totals and the most expensive cards, and opens to every row', async () => {
    query.data = payload([
      line('a', {
        price_usd: '20.00',
        cheapestId: 'a2',
        cheapestPrice: '5.00',
        saving_usd: '15.00',
      }),
      line('b', {
        price_usd: '12.00',
        cheapestId: 'b2',
        cheapestPrice: '9.00',
        saving_usd: '3.00',
      }),
      line('c', { price_usd: '8.00', cheapestId: 'c', saving_usd: '0.00' }),
      line('d', { price_usd: null, saving_usd: null }),
    ])
    const wrapper = mountPanel()
    expect(query.authedCalls).toBe(1)
    expect(query.publicCalls).toBe(0)

    const header = wrapper.text()
    expect(header).toContain('$40.00 as held')
    expect(header).toContain('$22.00 at cheapest printings')
    expect(header).toContain('save $18.00 across 2 cards')
    expect(header).toContain('1 unpriced')

    // Collapsed: the priced rows as chips, unpriced ones never among them, no table yet.
    const chips = wrapper.get('[data-testid="top-expensive"]').findAll('li')
    expect(chips.map((chip) => chip.text())).toEqual([
      'Card a$20.00',
      'Card b$12.00',
      'Card c$8.00',
    ])
    expect(wrapper.findAll('[data-testid="pricing-line"]')).toHaveLength(0)

    await expand(wrapper)
    const rows = wrapper.findAll('[data-testid="pricing-line"]')
    expect(rows).toHaveLength(4)
    expect(rows[2]!.text()).toContain('Already the cheapest')
    expect(rows[3]!.text()).toContain('No priced printing')
  })

  it('never renders an unpriced value as money, and offers a swap only where a saving was stated', async () => {
    query.data = payload([
      // Priced with a saving: swappable.
      line('a', { cheapestId: 'a2' }),
      // A cheapest printing but no saving (held printing unpriced in a held finish): not.
      line('b', { price_usd: '4.00', cheapestId: 'b2', saving_usd: null }),
      // Unpriced altogether.
      line('c', { price_usd: null, saving_usd: null }),
    ])
    const wrapper = mountPanel()
    await expand(wrapper)
    const rows = wrapper.findAll('[data-testid="pricing-line"]')

    expect(rows[2]!.get('[data-testid="line-price"]').text()).toBe('—')
    expect(rows[1]!.get('[data-testid="line-saving"]').text()).toBe('—')
    expect(rows[2]!.get('[data-testid="line-saving"]').text()).toBe('—')
    expect(wrapper.text()).not.toContain('$0.00')

    const swapButtons = rows.map((row) => row.find('button[aria-label^="Swap"]').exists())
    expect(swapButtons).toEqual([true, false, false])
  })

  it('opens the shared printing dialog for a row with its cheapest printing suggested', async () => {
    query.data = payload([line('a', { section_id: 11, cheapestId: 'a2', cheapestPrice: '2.50' })])
    const wrapper = mountPanel()
    await expand(wrapper)
    expect(wrapper.find('[data-testid="printing-dialog"]').exists()).toBe(false)

    await wrapper.get('button[aria-label="Swap Card a to its cheapest printing"]').trigger('click')
    const dialog = wrapper.get('[data-testid="printing-dialog"]')
    expect(dialog.attributes('data-open')).toBe('true')
    expect(dialog.attributes('data-card')).toBe('a')
    expect(dialog.attributes('data-section')).toBe('11')
    expect(dialog.attributes('data-suggested')).toBe('a2')
    expect(dialog.attributes('data-suggested-price')).toBe('2.50')
  })

  it('swaps every swappable row, and only those, through one batched write', async () => {
    query.data = payload([
      line('a', { section_id: 1, cheapestId: 'a2' }),
      line('b', { section_id: 2, cheapestId: 'b', saving_usd: '0.00' }),
      line('c', { section_id: 3, price_usd: null, cheapestId: 'c2', saving_usd: null }),
      line('d', { section_id: 4, cheapestId: 'd2', saving_usd: '0.25' }),
    ])
    const wrapper = mountPanel()
    const swapAll = wrapper.findAll('button').find((button) => button.text().startsWith('Swap all'))
    if (!swapAll) throw new Error('missing swap-all button')
    expect(swapAll.text()).toBe('Swap all (2)')

    await swapAll.trigger('click')
    const confirm = wrapper.findAll('button').find((button) => button.text() === 'Swap all')
    if (!confirm) throw new Error('missing confirmation button')
    await confirm.trigger('click')
    await flushPromises()

    expect(mocks.swapAll).toHaveBeenCalledTimes(1)
    expect(mocks.swapAll).toHaveBeenCalledWith({
      game: 'mtg',
      deckId: 3,
      swaps: [
        { id: 'a', sectionId: 1, newCardId: 'a2' },
        { id: 'd', sectionId: 4, newCardId: 'd2' },
      ],
    })
  })

  it('keeps the rows already swapped when the batch stops on an error', async () => {
    query.data = payload([line('a', { cheapestId: 'a2' })])
    mocks.swapAll.mockRejectedValue(new Error('boom'))
    const wrapper = mountPanel()
    await wrapper
      .findAll('button')
      .find((button) => button.text().startsWith('Swap all'))!
      .trigger('click')
    await wrapper
      .findAll('button')
      .find((button) => button.text() === 'Swap all')!
      .trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('Rows swapped so far are kept.')
  })

  it('is read-only on a shared deck: the breakdown, no swaps', async () => {
    query.data = payload([line('a', { cheapestId: 'a2' })])
    const wrapper = mountPanel({ handle: 'alice-0001' })
    expect(query.publicCalls).toBe(1)
    expect(query.authedCalls).toBe(0)
    expect(wrapper.text()).toContain('save $18.00')
    expect(wrapper.findAll('button').some((b) => b.text().startsWith('Swap all'))).toBe(false)
    await expand(wrapper)
    expect(wrapper.find('button[aria-label^="Swap"]').exists()).toBe(false)
    expect(wrapper.text()).toContain('TWO · #a2')
  })

  it('says when nothing is priced, and keeps a stale table on a failed refetch', () => {
    query.data = payload([line('a', { price_usd: null, saving_usd: null })], {
      total_usd: null,
      cheapest_total_usd: null,
      saving_usd: null,
      unpriced_count: 1,
      swappable_count: 0,
    })
    const wrapper = mountPanel()
    expect(wrapper.text()).toContain('Nothing in this deck is priced yet.')
    expect(wrapper.text()).not.toContain('$')

    query.refetchError = true
    query.data = payload([line('a')])
    const stale = mountPanel()
    expect(stale.text()).toContain('showing the prices as they last loaded')
    expect(stale.text()).toContain('$40.00 as held')
  })

  it('shows a skeleton while pending and a message when the first load fails', () => {
    query.pending = true
    expect(mountPanel().findAll('[data-testid="skeleton"]').length).toBeGreaterThan(0)
    query.pending = false
    query.loadingError = true
    expect(mountPanel().text()).toContain("Couldn't price this deck.")
  })
})

describe('deckPricing helpers', () => {
  it('counts a saving only when the server stated one above zero', () => {
    expect(hasSaving(line('a', { cheapestId: 'a2', saving_usd: '0.01' }))).toBe(true)
    expect(hasSaving(line('a', { cheapestId: 'a', saving_usd: '0.00' }))).toBe(false)
    expect(hasSaving(line('a', { cheapestId: 'a2', saving_usd: null }))).toBe(false)
    expect(hasSaving(line('a', { saving_usd: '5.00' }))).toBe(false)
  })

  it('names a handful of rows at rest', () => {
    expect(TOP_EXPENSIVE).toBeGreaterThan(0)
    expect(TOP_EXPENSIVE).toBeLessThanOrEqual(8)
  })
})
