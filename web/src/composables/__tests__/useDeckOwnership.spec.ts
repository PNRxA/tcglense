import { describe, expect, it, vi } from 'vitest'
import { computed, ref } from 'vue'
import { makeCard } from '@/test/fixtures'
import type { DeckCardEntry, OwnedCountsMap } from '@/lib/api'

// The two batched holdings reads, each answering a fixed map; `entriesSeen` records the card
// lists handed to them so the spec can pin what the composable asks about.
const owned = vi.hoisted(() => ({ value: {} as OwnedCountsMap }))
const wanted = vi.hoisted(() => ({ value: {} as OwnedCountsMap }))
const seen = vi.hoisted(() => ({ collection: [] as unknown[], wishlist: [] as unknown[] }))

vi.mock('@/composables/useCollection', async () => {
  const { computed: vueComputed } = await import('vue')
  return {
    useOwnedCounts: (_game: unknown, cards: { value: unknown }) => {
      seen.collection.push(cards)
      return { ownership: vueComputed(() => owned.value) }
    },
  }
})
vi.mock('@/composables/useWishlist', async () => {
  const { computed: vueComputed } = await import('vue')
  return {
    useWishlistCounts: (_game: unknown, cards: { value: unknown }) => {
      seen.wishlist.push(cards)
      return { ownership: vueComputed(() => wanted.value) }
    },
  }
})

import { useDeckOwnership } from '../useDeckOwnership'

function entry(cardId: string, sectionId = 1): DeckCardEntry {
  return { card: makeCard(cardId), section_id: sectionId, quantity: 1, foil_quantity: 0 }
}

describe('useDeckOwnership', () => {
  it('folds both finishes into one owned and one wanted count per card', () => {
    owned.value = { c1: { quantity: 2, foil_quantity: 1 } }
    wanted.value = { c2: { quantity: 0, foil_quantity: 3 } }
    const { ownedInCollection, wantedInWishlist } = useDeckOwnership(
      ref('mtg'),
      computed(() => [entry('c1'), entry('c2')]),
    )

    expect(ownedInCollection('c1')).toBe(3)
    expect(wantedInWishlist('c1')).toBe(0)
    expect(ownedInCollection('c2')).toBe(0)
    expect(wantedInWishlist('c2')).toBe(3)
  })

  // The holdings seam returns an empty map while signed out; a card absent from it is a
  // zero, not an error — which is what lets a public page render the chips ungated.
  it('answers zero for a card the maps do not hold', () => {
    owned.value = {}
    wanted.value = {}
    const { ownedInCollection, wantedInWishlist } = useDeckOwnership(
      ref('mtg'),
      computed(() => [entry('c1')]),
    )

    expect(ownedInCollection('c1')).toBe(0)
    expect(wantedInWishlist('c1')).toBe(0)
  })

  it('hands both reads the catalog cards behind the entries, and tracks the list', () => {
    seen.collection.length = 0
    seen.wishlist.length = 0
    const entries = ref<DeckCardEntry[]>([entry('c1'), entry('c1', 2)])
    useDeckOwnership(ref('mtg'), entries)

    const cards = seen.collection[0] as { value: { id: string }[] }
    expect(cards.value.map((card) => card.id)).toEqual(['c1', 'c1'])
    expect((seen.wishlist[0] as { value: unknown }).value).toBe(cards.value)

    entries.value = [entry('c9')]
    expect(cards.value.map((card) => card.id)).toEqual(['c9'])
  })
})
