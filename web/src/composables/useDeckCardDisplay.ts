import { computed, ref, type Ref } from 'vue'
import type { DeckCardEntry, DeckSection } from '@/lib/api'
import { filterDeckEntries, type DeckFilterColor } from '@/lib/deckFilter'

// The card-display pipeline shared by the owner and public deck views (issue #562):
// filter the loaded entries (text + colour pips), group them by section, and decide
// which sections show. Owner-only concerns (mutations, export, sharing) stay in
// useDeckEditor, which layers this engine; the public view uses it directly.

interface DeckCardDisplayOptions {
  cards: Ref<DeckCardEntry[]>
  sections: Ref<DeckSection[]>
  /** The owner view's "show empty sections" toggle; omitted on the public view. */
  showEmpty?: Ref<boolean>
}

export function useDeckCardDisplay({ cards, sections, showEmpty }: DeckCardDisplayOptions) {
  const filterQuery = ref('')
  const filterColors = ref<DeckFilterColor[]>([])
  const filterActive = computed(
    () => filterQuery.value.trim().length > 0 || filterColors.value.length > 0,
  )
  function clearFilters() {
    filterQuery.value = ''
    filterColors.value = []
  }

  const filteredCards = computed(() =>
    filterDeckEntries(cards.value, filterQuery.value, filterColors.value),
  )
  const cardsBySection = computed(() => {
    const map = new Map<number, DeckCardEntry[]>()
    for (const section of sections.value) map.set(section.id, [])
    for (const entry of filteredCards.value) map.get(entry.section_id)?.push(entry)
    return map
  })

  // While a filter is active, only sections with a match show (even under "show empty" — a
  // page of blank sections reads as a bug); otherwise empty sections stay hidden unless the
  // owner's toggle reveals them.
  const visibleSections = computed(() => {
    if (!filterActive.value && showEmpty?.value) return sections.value
    return sections.value.filter(
      (section) => (cardsBySection.value.get(section.id)?.length ?? 0) > 0,
    )
  })
  const sectionNavItems = computed(() =>
    visibleSections.value.map((section) => ({
      id: section.id,
      name: section.name,
      count: cardsBySection.value.get(section.id)?.length ?? 0,
    })),
  )

  const matchCount = computed(() => filteredCards.value.length)
  const totalCount = computed(() => cards.value.length)

  return {
    filterQuery,
    filterColors,
    filterActive,
    clearFilters,
    filteredCards,
    cardsBySection,
    visibleSections,
    sectionNavItems,
    matchCount,
    totalCount,
  }
}
