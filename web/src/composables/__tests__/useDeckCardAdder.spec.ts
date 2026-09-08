import { beforeEach, describe, expect, it, vi } from 'vitest'
import { computed, nextTick, ref } from 'vue'
import { useDeckCardAdder } from '@/composables/useDeckCardAdder'
import type { DeckCardEntry, DeckSection } from '@/lib/api'
import { makeCard } from '@/test/fixtures'

// The additive add engine both the add-cards box and the suggestions panel file through.
// What's pinned is the bookkeeping around the deck's absolute-count write, since the write
// itself is the mutation's: the automatic-by-type filing and its refusal for a type with no
// safe bucket, the optimistic stacking that lets four clicks build a playset off a stale
// cache, the rollback that keeps a failed write from being counted, the catch-up that drops
// an optimistic count once the deck holds it, and the pinned target snapping back to
// automatic when its section disappears.

const mocks = vi.hoisted(() => ({
  setCard: vi.fn<(vars: unknown) => Promise<unknown>>(),
}))

vi.mock('@/composables/useDecks', async () => {
  const { ref } = await import('vue')
  return {
    useSetDeckCardMutation: () => ({ mutateAsync: mocks.setCard, isPending: ref(false) }),
  }
})

const SECTIONS: DeckSection[] = [
  { id: 2, name: 'Creatures', position: 0, is_maybeboard: false },
  { id: 3, name: 'Instants', position: 1, is_maybeboard: false },
]
const bear = makeCard('bear', { type_line: 'Creature — Bear' })
const battle = makeCard('battle', { type_line: 'Battle — Siege' })

function setup(cards: DeckCardEntry[] = [], sections = SECTIONS) {
  const sectionsRef = ref(sections)
  const cardsRef = ref(cards)
  const adder = useDeckCardAdder({
    game: computed(() => 'mtg'),
    deckId: computed(() => 3),
    sections: computed(() => sectionsRef.value),
    cards: computed(() => cardsRef.value),
  })
  return { adder, sectionsRef, cardsRef }
}

beforeEach(() => {
  mocks.setCard.mockReset().mockResolvedValue({ quantity: 1, foil_quantity: 0 })
})

describe('useDeckCardAdder', () => {
  it('files by type under the automatic target, one more than the deck holds, keeping the foil count', async () => {
    const { adder } = setup([{ card: bear, section_id: 2, quantity: 1, foil_quantity: 2 }])
    expect(adder.inTargetCount(bear)).toBe(3)
    await adder.add(bear)
    expect(mocks.setCard).toHaveBeenCalledWith({
      game: 'mtg',
      deckId: 3,
      sectionId: 2,
      id: 'bear',
      quantity: 2,
      foil_quantity: 2,
    })
  })

  it('refuses a type with no safe bucket, and files it once a section is pinned', async () => {
    const { adder } = setup()
    expect(adder.needsExplicitSection(battle)).toBe(true)
    await adder.add(battle)
    expect(mocks.setCard).not.toHaveBeenCalled()
    adder.target.value = '3'
    expect(adder.needsExplicitSection(battle)).toBe(false)
    await adder.add(battle)
    expect(mocks.setCard).toHaveBeenCalledWith(
      expect.objectContaining({ id: 'battle', sectionId: 3, quantity: 1 }),
    )
  })

  it('stacks rapid re-adds off the optimistic count, not the stale deck cache', async () => {
    const { adder } = setup()
    await adder.add(bear)
    await adder.add(bear)
    await adder.add(bear)
    const quantities = mocks.setCard.mock.calls.map(
      (call) => (call[0] as { quantity: number }).quantity,
    )
    expect(quantities).toEqual([1, 2, 3])
  })

  it('rolls a failed write back, so a retry adds one copy and not two', async () => {
    const { adder } = setup()
    mocks.setCard.mockRejectedValueOnce(new Error('boom'))
    await expect(adder.add(bear)).rejects.toThrow('boom')
    await adder.add(bear)
    const quantities = mocks.setCard.mock.calls.map(
      (call) => (call[0] as { quantity: number }).quantity,
    )
    expect(quantities).toEqual([1, 1])
    expect(adder.isPending(bear)).toBe(false)
  })

  it('drops an optimistic count once the deck has caught up with it', async () => {
    const { adder, cardsRef } = setup()
    await adder.add(bear)
    cardsRef.value = [{ card: bear, section_id: 2, quantity: 1, foil_quantity: 0 }]
    await nextTick()
    await adder.add(bear)
    const quantities = mocks.setCard.mock.calls.map(
      (call) => (call[0] as { quantity: number }).quantity,
    )
    expect(quantities).toEqual([1, 2])
  })

  it('snaps a pinned target back to automatic when its section disappears', async () => {
    const { adder, sectionsRef } = setup()
    adder.target.value = '3'
    sectionsRef.value = [SECTIONS[0]!]
    await nextTick()
    expect(adder.target.value).toBe('auto')
  })
})
