import { beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'

// The "swap all" batch (issue #672) is the one mutation in this module that is a LOOP, and
// the auth boundary must therefore sit per row: an access token expiring at row k retries
// only that row — the rows before it are already committed server-side, and replaying them
// would 404 on a printing that is no longer in the section. So the batch is deliberately
// not built on `useAuthedMutation`, whose single `authFetch` wraps the whole function and
// re-runs it from the top on a 401. These cases pin that boundary and the batch's other two
// promises: the rows are sent in order through the existing per-row write, and the deck is
// invalidated once per batch, not once per row.

const mocks = vi.hoisted(() => ({
  authFetch: vi.fn<(call: (token: string) => Promise<unknown>) => Promise<unknown>>(),
  changeDeckCardPrinting: vi.fn<(...args: unknown[]) => Promise<unknown>>(),
}))

vi.mock('@/stores/auth', () => ({
  useAuthStore: () => ({ authFetch: mocks.authFetch }),
}))

vi.mock('@/lib/api/decks', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api/decks')>('@/lib/api/decks')
  return { ...actual, changeDeckCardPrinting: mocks.changeDeckCardPrinting }
})

import { useChangeDeckCardPrintingsMutation } from '@/composables/useDecks'

function mountMutation() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const invalidate = vi.spyOn(qc, 'invalidateQueries')
  let mutation!: ReturnType<typeof useChangeDeckCardPrintingsMutation>
  mount(
    defineComponent({
      setup() {
        mutation = useChangeDeckCardPrintingsMutation()
        return () => h('div')
      },
    }),
    { global: { plugins: [[VueQueryPlugin, { queryClient: qc }]] } },
  )
  return { mutation, invalidate }
}

const SWAPS = [
  { id: 'a', sectionId: 1, newCardId: 'a2' },
  { id: 'b', sectionId: 2, newCardId: 'b2' },
  { id: 'c', sectionId: 3, newCardId: 'c2' },
]

beforeEach(() => {
  mocks.authFetch.mockReset().mockImplementation((call) => call('tok'))
  mocks.changeDeckCardPrinting.mockReset().mockResolvedValue({ quantity: 1, foil_quantity: 0 })
})

describe('useChangeDeckCardPrintingsMutation', () => {
  it('takes the auth boundary once per row, in order, through the per-row write', async () => {
    const { mutation, invalidate } = mountMutation()
    const result = await mutation.mutateAsync({ game: 'mtg', deckId: 9, swaps: SWAPS })
    await flushPromises()

    expect(result).toEqual({ swapped: 3 })
    // One authFetch per row — never one around the whole loop.
    expect(mocks.authFetch).toHaveBeenCalledTimes(3)
    expect(mocks.changeDeckCardPrinting.mock.calls.map((call) => call.slice(1))).toEqual([
      ['mtg', 9, 'a', { new_card_id: 'a2', section_id: 1 }],
      ['mtg', 9, 'b', { new_card_id: 'b2', section_id: 2 }],
      ['mtg', 9, 'c', { new_card_id: 'c2', section_id: 3 }],
    ])
    // The deck family is invalidated for the batch, not per row.
    const deckInvalidations = invalidate.mock.calls.filter(([filters]) => {
      const key = typeof filters === 'object' && filters ? filters.queryKey : undefined
      return Array.isArray(key) && key[0] === 'deck' && key[2] === 9
    })
    expect(deckInvalidations).toHaveLength(1)
  })

  it('a token rejected mid-batch retries only that row, never the committed ones', async () => {
    // Simulate authFetch's own contract on row b: the first call is rejected, the retry
    // (with a fresh token) succeeds — and only row b's write is sent twice.
    let attempts = 0
    mocks.authFetch.mockImplementation(async (call) => {
      attempts += 1
      if (attempts === 2) {
        await call('stale').catch(() => undefined)
        return call('fresh')
      }
      return call('tok')
    })
    mocks.changeDeckCardPrinting.mockImplementation(async (token) => {
      if (token === 'stale') throw new Error('401')
      return { quantity: 1, foil_quantity: 0 }
    })
    const { mutation } = mountMutation()
    await mutation.mutateAsync({ game: 'mtg', deckId: 9, swaps: SWAPS })

    const sent = mocks.changeDeckCardPrinting.mock.calls.map((call) => [call[0], call[3]])
    expect(sent).toEqual([
      ['tok', 'a'],
      ['stale', 'b'],
      ['fresh', 'b'],
      ['tok', 'c'],
    ])
  })

  it('stops at the first failing row and reports the rows already swapped as kept', async () => {
    mocks.changeDeckCardPrinting.mockImplementation(async (_t, _g, _d, id) => {
      if (id === 'b') throw new Error('boom')
      return { quantity: 1, foil_quantity: 0 }
    })
    const { mutation, invalidate } = mountMutation()
    await expect(mutation.mutateAsync({ game: 'mtg', deckId: 9, swaps: SWAPS })).rejects.toThrow(
      'boom',
    )
    await flushPromises()
    // a was sent, b failed, c never went — and the deck is still refreshed once, so the
    // table shows what actually landed.
    expect(mocks.changeDeckCardPrinting.mock.calls.map((call) => call[3])).toEqual(['a', 'b'])
    expect(invalidate).toHaveBeenCalled()
  })
})
