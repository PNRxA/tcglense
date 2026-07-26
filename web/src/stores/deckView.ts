import { defineStore } from 'pinia'
import { DEFAULT_DECK_VIEW_MODE, isDeckViewMode, type DeckViewMode } from '@/lib/deckView'
import { persistedRef } from '@/lib/persistedRef'

// How the deck pages render their card lists (issue #570) — images, a compact list, or
// plain text. A personal display preference like the card size, so it lives in
// localStorage and applies to every deck page, not in the URL like per-list state.
const STORAGE_KEY = 'tcglense_deck_view_mode'

export const useDeckViewStore = defineStore('deckView', () => {
  const mode = persistedRef<DeckViewMode>(STORAGE_KEY, DEFAULT_DECK_VIEW_MODE, isDeckViewMode)

  function setMode(next: DeckViewMode) {
    mode.value = next
  }

  return { mode, setMode }
})
