import { ref, watch, type Ref } from 'vue'
import { useSetDeckCardMutation } from '@/composables/useDecks'
import type { Card, DeckCardEntry, DeckSection } from '@/lib/api'
import { automaticDeckSection } from '@/lib/deckCategories'

// The deck builder's **additive add** engine — "one more copy of this printing, in that
// section" — shared by the add-cards box (`DeckAddCard`) and the from-your-collection
// suggestions panel (`DeckSuggestions`, issue #684), so both file a card the same way and
// stack rapid re-adds the same way. It writes through the deck's existing absolute-count
// `PUT …/cards/{id}` (never a new bulk write); what it adds is the bookkeeping around it.
//
// * **The target section** is `'auto'` — each printing files into its type preset through
//   `automaticDeckSection`, and a type with no safe bucket (a Battle, an unknown line) asks
//   for an explicit pick rather than guessing — or a section id, which an owner pins for
//   functional categorisation. A pinned section that stops existing snaps back to `'auto'`.
// * **Optimistic per-(card, section) counts** so clicking the same printing four times builds
//   a playset instead of writing `1` four times off a deck cache that only refreshes after
//   each write's refetch lands; an entry is dropped once the deck has caught up with it, and
//   **rolled back when the write rejects**, so a retry adds one copy and not the one the
//   server never received plus another.
// * **In-flight adds** are a reactive `Set` keyed the same way, so each tile or row spins
//   its own "+" while its own write is outstanding — a shared mutation's `isPending` can't
//   tell them apart.

export interface DeckCardAdderInput {
  game: Ref<string>
  deckId: Ref<number>
  sections: Ref<DeckSection[]>
  /** The deck's loaded rows — the counts a write starts from and the refetch catches up to. */
  cards: Ref<DeckCardEntry[]>
}

export function useDeckCardAdder(input: DeckCardAdderInput) {
  const target = ref('auto')
  watch(
    input.sections,
    (sections) => {
      if (target.value === 'auto') return
      if (!sections.some((section) => String(section.id) === target.value)) target.value = 'auto'
    },
    { immediate: true },
  )

  const setCard = useSetDeckCardMutation()

  const optimistic = new Map<string, number>()
  const keyOf = (cardId: string, sectionId: number) => `${cardId}:${sectionId}`
  const pending = ref(new Set<string>())
  watch(input.cards, (cards) => {
    for (const [k, v] of optimistic) {
      const [cardId, sec] = k.split(':')
      const entry = cards.find((c) => c.card.id === cardId && c.section_id === Number(sec))
      if ((entry?.quantity ?? 0) >= v) optimistic.delete(k)
    }
  })

  function currentCounts(cardId: string, sectionId: number): { quantity: number; foil: number } {
    const entry = input.cards.value.find((c) => c.card.id === cardId && c.section_id === sectionId)
    return { quantity: entry?.quantity ?? 0, foil: entry?.foil_quantity ?? 0 }
  }

  /** Where this printing would be filed under the current target, or `null` when the
   * automatic filing has no safe bucket for it. */
  function targetSectionId(card: Pick<Card, 'type_line'>): number | null {
    if (target.value === 'auto') {
      return automaticDeckSection(card, input.sections.value)?.id ?? null
    }
    const sectionId = Number(target.value)
    return Number.isFinite(sectionId) ? sectionId : null
  }

  function needsExplicitSection(card: Pick<Card, 'type_line'>): boolean {
    return target.value === 'auto' && targetSectionId(card) == null
  }

  /** Copies (regular + foil) of this printing already in its target section. */
  function inTargetCount(card: Card): number {
    const sectionId = targetSectionId(card)
    if (sectionId == null) return 0
    const { quantity, foil } = currentCounts(card.id, sectionId)
    return quantity + foil
  }

  function isPending(card: Card): boolean {
    const sectionId = targetSectionId(card)
    if (sectionId == null) return false
    return pending.value.has(keyOf(card.id, sectionId))
  }

  /** Add one regular copy of `card` to its target section. A no-op when the automatic
   * filing has nowhere safe to put it. Rejects like the mutation does. */
  async function add(card: Card): Promise<void> {
    const sectionId = targetSectionId(card)
    if (sectionId == null) return
    const k = keyOf(card.id, sectionId)
    const server = currentCounts(card.id, sectionId)
    const previous = optimistic.get(k)
    const next = (previous ?? server.quantity) + 1
    optimistic.set(k, next)
    pending.value.add(k)
    try {
      await setCard.mutateAsync({
        game: input.game.value,
        deckId: input.deckId.value,
        sectionId,
        id: card.id,
        quantity: next,
        foil_quantity: server.foil,
      })
    } catch (error) {
      // The server never took this count: forget it, so the next attempt starts from what
      // the deck actually holds rather than stacking on a write that failed.
      if (previous === undefined) optimistic.delete(k)
      else optimistic.set(k, previous)
      throw error
    } finally {
      pending.value.delete(k)
    }
  }

  return {
    /** `'auto'` or a section id as a string — what a `<select>` binds to. */
    target,
    needsExplicitSection,
    inTargetCount,
    isPending,
    add,
  }
}
