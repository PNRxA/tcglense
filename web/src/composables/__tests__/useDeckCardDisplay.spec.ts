import { describe, expect, it } from 'vitest'
import { ref } from 'vue'
import type { Card, DeckCardEntry, DeckSection } from '@/lib/api'
import { useDeckCardDisplay } from '../useDeckCardDisplay'

function entry(
  id: string,
  name: string,
  sectionId: number,
  card: Partial<Card> = {},
): DeckCardEntry {
  return {
    section_id: sectionId,
    quantity: 1,
    foil_quantity: 0,
    card: {
      id,
      name,
      type_line: null,
      oracle_text: null,
      color_identity: [],
      faces: [],
      ...card,
    } as Card,
  }
}

const sections: DeckSection[] = [
  { id: 1, name: 'Creatures', position: 0 },
  { id: 2, name: 'Lands', position: 1 },
  { id: 3, name: 'Sideboard', position: 2 },
]
const cards = [
  entry('goblin', 'Goblin Guide', 1, { color_identity: ['R'] }),
  entry('bear', 'Grizzly Bears', 1, { color_identity: ['G'] }),
  entry('island', 'Island', 2, { color_identity: ['U'] }),
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

  it('narrows sections and counts while a text filter is active, even with empties shown', () => {
    const display = make(true)
    display.filterQuery.value = 'goblin'
    expect(display.filterActive.value).toBe(true)
    expect(display.visibleSections.value.map((s) => s.id)).toEqual([1])
    expect(display.sectionNavItems.value).toEqual([{ id: 1, name: 'Creatures', count: 1 }])
    expect(display.matchCount.value).toBe(1)
    expect(display.totalCount.value).toBe(3)
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
    expect(display.matchCount.value).toBe(3)
  })
})
