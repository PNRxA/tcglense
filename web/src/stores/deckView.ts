import { defineStore } from 'pinia'
import { DEFAULT_DECK_VIEW_MODE, isDeckViewMode, type DeckViewMode } from '@/lib/deckView'
import { persistedBoolRef, persistedRef } from '@/lib/persistedRef'

// How the deck pages render their card lists (issue #570) — images, a compact list, or
// plain text. A personal display preference like the card size, so it lives in
// localStorage and applies to every deck page, not in the URL like per-list state.
const STORAGE_KEY = 'tcglense_deck_view_mode'

// Whether the deck page's overview strip is opened out into the full analysis stack
// (`components/decks/DeckOverview.vue`). Collapsed by default: the page's subject is the
// card list, and a dozen panels stood between the header and it. Persisted, unlike the
// panels' own per-mount disclosures, because this is a page-layout choice rather than a
// per-deck question — someone who wants the whole picture shouldn't re-open it on every deck.
const OVERVIEW_KEY = 'tcglense_deck_overview_expanded'

export const useDeckViewStore = defineStore('deckView', () => {
  const mode = persistedRef<DeckViewMode>(STORAGE_KEY, DEFAULT_DECK_VIEW_MODE, isDeckViewMode)

  const overviewExpanded = persistedBoolRef(OVERVIEW_KEY, false)

  function setMode(next: DeckViewMode) {
    mode.value = next
  }

  function setOverviewExpanded(next: boolean) {
    overviewExpanded.value = next
  }

  return { mode, setMode, overviewExpanded, setOverviewExpanded }
})
