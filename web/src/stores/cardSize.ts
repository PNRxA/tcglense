import { defineStore } from 'pinia'
import { DEFAULT_CARD_SIZE, isCardSize, type CardSize } from '@/lib/cardSize'
import { persistedRef } from '@/lib/persistedRef'

// The chosen card-grid size is a personal display preference (like the theme), so
// it lives in localStorage and applies everywhere the grid is shown — not in the
// URL like the per-list page/search/sort state.
const STORAGE_KEY = 'tcglense_card_size'

export const useCardSizeStore = defineStore('cardSize', () => {
  const size = persistedRef<CardSize>(STORAGE_KEY, DEFAULT_CARD_SIZE, isCardSize)

  function setSize(next: CardSize) {
    size.value = next
  }

  return { size, setSize }
})
