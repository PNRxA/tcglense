import { describe, expect, it } from 'vitest'
import { ref } from 'vue'
import { makeCard } from '@/test/fixtures'
import type { Card, DeckCardEntry, DeckSection } from '@/lib/api'
import { useDeckCardDisplay } from '../useDeckCardDisplay'

function entry(
  id: string,
  sectionId: number,
  card: Partial<Card>,
  copies: { quantity?: number; foil_quantity?: number } = {},
): DeckCardEntry {
  return {
    section_id: sectionId,
    quantity: copies.quantity ?? 1,
    foil_quantity: copies.foil_quantity ?? 0,
    card: makeCard(id, card),
  }
}

const sections: DeckSection[] = [
  { id: 1, name: 'Creatures', position: 0, is_maybeboard: false },
  { id: 2, name: 'Lands', position: 1, is_maybeboard: false },
  { id: 3, name: 'Sideboard', position: 2, is_maybeboard: false },
]
// 5 copies across 3 entries: the island entry is multi-copy so the copy-weighted
// counts are distinguishable from entry counts.
const cards = [
  entry('goblin', 1, {
    name: 'Goblin Guide',
    type_line: 'Creature — Goblin Scout',
    color_identity: ['R'],
  }),
  entry('bear', 1, {
    name: 'Grizzly Bears',
    type_line: 'Creature — Bear',
    color_identity: ['G'],
  }),
  entry('island', 2, { name: 'Island', color_identity: ['U'] }, { quantity: 2, foil_quantity: 1 }),
]

function make(showEmpty = false) {
  return useDeckCardDisplay({
    cards: ref(cards),
    sections: ref(sections),
    showEmpty: ref(showEmpty),
  })
}

describe('useDeckCardDisplay', () => {
  it('groups cards by section and hides empty sections by default', () => {
    const display = make()
    expect(display.cardsBySection.value.get(1)?.map((e) => e.card.id)).toEqual(['goblin', 'bear'])
    expect(display.visibleSections.value.map((s) => s.id)).toEqual([1, 2])
    expect(display.sectionNavItems.value).toEqual([
      { id: 1, name: 'Creatures', count: 2 },
      { id: 2, name: 'Lands', count: 1 },
    ])
  })

  it('shows empty sections when the toggle is on', () => {
    const display = make(true)
    expect(display.visibleSections.value.map((s) => s.id)).toEqual([1, 2, 3])
  })

  it('narrows sections and counts copies while a text filter is active, even with empties shown', () => {
    const display = make(true)
    display.filterQuery.value = 'goblin'
    expect(display.filterActive.value).toBe(true)
    expect(display.visibleSections.value.map((s) => s.id)).toEqual([1])
    expect(display.sectionNavItems.value).toEqual([{ id: 1, name: 'Creatures', count: 1 }])
    expect(display.matchCount.value).toBe(1)
    expect(display.totalCount.value).toBe(5)
  })

  it('weights the match totals by copies (regular + foil), like the page header', () => {
    const display = make()
    display.filterQuery.value = 'island'
    expect(display.matchCount.value).toBe(3)
    expect(display.totalCount.value).toBe(5)
  })

  it('filters by colour pips and clears both filters at once', () => {
    const display = make()
    display.filterQuery.value = 'g'
    display.filterColors.value = ['U']
    expect(display.matchCount.value).toBe(0)
    expect(display.visibleSections.value).toEqual([])
    display.clearFilters()
    expect(display.filterQuery.value).toBe('')
    expect(display.filterColors.value).toEqual([])
    expect(display.filterActive.value).toBe(false)
    expect(display.matchCount.value).toBe(5)
  })

  it('recomputes when the cards and sections refs resolve after setup', () => {
    const lateCards = ref<DeckCardEntry[]>([])
    const lateSections = ref<DeckSection[]>([])
    const display = useDeckCardDisplay({ cards: lateCards, sections: lateSections })
    expect(display.visibleSections.value).toEqual([])
    expect(display.totalCount.value).toBe(0)
    lateSections.value = sections
    lateCards.value = cards
    expect(display.visibleSections.value.map((s) => s.id)).toEqual([1, 2])
    expect(display.cardsBySection.value.get(2)?.map((e) => e.card.id)).toEqual(['island'])
    expect(display.sectionNavItems.value.map((item) => item.count)).toEqual([2, 1])
    expect(display.totalCount.value).toBe(5)
  })
  it('splits the deck proper from the maybeboard without hiding either', () => {
    const withMaybeboard: DeckSection[] = [
      { id: 1, name: 'Creatures', position: 0, is_maybeboard: false },
      { id: 2, name: 'Lands', position: 1, is_maybeboard: false },
      { id: 3, name: 'Cut candidates', position: 2, is_maybeboard: true },
    ]
    const display = useDeckCardDisplay({
      cards: ref([...cards, entry('cut', 3, { name: 'Shock' })]),
      sections: ref(withMaybeboard),
      showEmpty: ref(false),
    })

    // The maybeboard card is out of the deck proper (what legality + analytics judge)...
    expect(display.deckCards.value.map((e) => e.card.id)).toEqual(['goblin', 'bear', 'island'])
    expect(display.maybeboardCards.value.map((e) => e.card.id)).toEqual(['cut'])
    // ...but it is still grouped, still counted in the visible list, and its section still
    // renders — a maybeboard is shown, just not counted.
    expect(display.cardsBySection.value.get(3)?.map((e) => e.card.id)).toEqual(['cut'])
    expect(display.visibleSections.value.map((s) => s.id)).toEqual([1, 2, 3])
    expect(display.totalCount.value).toBe(6)
  })

  it('keeps the deck/maybeboard split independent of the filter box', () => {
    const withMaybeboard: DeckSection[] = [
      { id: 1, name: 'Creatures', position: 0, is_maybeboard: false },
      { id: 2, name: 'Lands', position: 1, is_maybeboard: false },
      { id: 3, name: 'Cut candidates', position: 2, is_maybeboard: true },
    ]
    const display = useDeckCardDisplay({
      cards: ref([...cards, entry('cut', 3, { name: 'Shock' })]),
      sections: ref(withMaybeboard),
      showEmpty: ref(false),
    })
    display.filterQuery.value = 'Island'

    // Typing in the filter must not change what counts as "the deck", or the legality
    // banner and analytics would follow the filter.
    expect(display.deckCards.value.map((e) => e.card.id)).toEqual(['goblin', 'bear', 'island'])
    expect(display.maybeboardCards.value.map((e) => e.card.id)).toEqual(['cut'])
  })
})
