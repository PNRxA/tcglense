import { computed, ref, type Ref } from 'vue'
import type { DeckCardEntry, DeckRole, DeckRoles, DeckSection } from '@/lib/api'
import { filterDeckEntries, type DeckFilterColor } from '@/lib/deckFilter'

// The card-display pipeline shared by the owner and public deck views (issue #562):
// filter the loaded entries (text + colour pips), group them by section, and decide
// which sections show. Owner-only concerns (mutations, export, sharing) stay in
// useDeckEditor, which layers this engine; the public view uses it directly.
//
// The third filter is the card-role one (issue #671): the roles panel's bars are selectable,
// and picking one narrows the list below to the cards filling it. The roles themselves are
// the server's — this engine only takes the response and turns the selected role into a set
// of card ids for `filterDeckEntries`.
//
// It also owns the maybeboard split (issue #570): `deckCards` is the deck proper — every
// card outside a section flagged `is_maybeboard` — which is what legality and the analytics
// panel judge, matching the API's `summary`. `filteredCards` and the per-section grouping
// stay whole, because the maybeboard is still *shown*; it just isn't counted.

interface DeckCardDisplayOptions {
  cards: Ref<DeckCardEntry[]>
  sections: Ref<DeckSection[]>
  /** The owner view's "show empty sections" toggle; omitted on the public view. */
  showEmpty?: Ref<boolean>
  /** The deck's roles response, for the role filter. Omitted (or still in flight) simply
   *  means no role can be selected — see `roleCardIds`. */
  roles?: Ref<DeckRoles | undefined>
}

export function useDeckCardDisplay({ cards, sections, showEmpty, roles }: DeckCardDisplayOptions) {
  const filterQuery = ref('')
  const filterColors = ref<DeckFilterColor[]>([])
  const filterRole = ref<DeckRole | null>(null)
  const filterActive = computed(
    () =>
      filterQuery.value.trim().length > 0 ||
      filterColors.value.length > 0 ||
      filterRole.value !== null,
  )
  function clearFilters() {
    filterQuery.value = ''
    filterColors.value = []
    filterRole.value = null
  }

  // The selected role as a set of external card ids, or `null` for "no role selected" —
  // which `filterDeckEntries` reads as no constraint at all. With a role selected but no
  // roles data (still in flight, or the read failed) this is deliberately an EMPTY set:
  // the list narrows to nothing rather than silently showing everything, which would look
  // like the filter had been applied and matched every card.
  const roleCardIds = computed<ReadonlySet<string> | null>(() => {
    const role = filterRole.value
    if (role === null) return null
    const ids = new Set<string>()
    for (const [cardId, cardRoles] of Object.entries(roles?.value?.card_roles ?? {})) {
      if (cardRoles.includes(role)) ids.add(cardId)
    }
    return ids
  })

  // Which sections sit outside the deck proper, and the deck's own cards (issue #570).
  // Unfiltered on purpose: what counts as "the deck" must not change when the user types
  // in the filter box, or the legality banner and analytics would follow the filter.
  const maybeboardSectionIds = computed(
    () => new Set(sections.value.filter((section) => section.is_maybeboard).map((s) => s.id)),
  )
  const deckCards = computed(() =>
    cards.value.filter((entry) => !maybeboardSectionIds.value.has(entry.section_id)),
  )
  const maybeboardCards = computed(() =>
    cards.value.filter((entry) => maybeboardSectionIds.value.has(entry.section_id)),
  )

  const filteredCards = computed(() =>
    filterDeckEntries(cards.value, filterQuery.value, filterColors.value, roleCardIds.value),
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

  // Copy-weighted (regular + foil), matching the header's `summary.total_cards` — an
  // entry-count "6 cards" beside a header's "10 cards" would read as a contradiction.
  const countCopies = (entries: DeckCardEntry[]) =>
    entries.reduce((sum, entry) => sum + entry.quantity + entry.foil_quantity, 0)
  const matchCount = computed(() => countCopies(filteredCards.value))
  const totalCount = computed(() => countCopies(cards.value))

  return {
    filterQuery,
    filterColors,
    filterRole,
    filterActive,
    clearFilters,
    filteredCards,
    deckCards,
    maybeboardCards,
    cardsBySection,
    visibleSections,
    sectionNavItems,
    matchCount,
    totalCount,
  }
}
