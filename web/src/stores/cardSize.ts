import { ref, watch } from 'vue'
import { defineStore } from 'pinia'
import { DEFAULT_CARD_SIZE, isCardSize, type CardSize } from '@/lib/cardSize'

// The chosen card-grid size is a personal display preference (like the theme), so
// it lives in localStorage and applies everywhere the grid is shown — not in the
// URL like the per-list page/search/sort state.
const STORAGE_KEY = 'tcglense_card_size'

function readStored(): CardSize {
  try {
    const stored = localStorage.getItem(STORAGE_KEY)
    return isCardSize(stored) ? stored : DEFAULT_CARD_SIZE
  } catch {
    // Storage unavailable (private mode, blocked): fall back to the default.
    return DEFAULT_CARD_SIZE
  }
}

export const useCardSizeStore = defineStore('cardSize', () => {
  const size = ref<CardSize>(readStored())

  function setSize(next: CardSize) {
    size.value = next
  }

  watch(size, (value) => {
    try {
      localStorage.setItem(STORAGE_KEY, value)
    } catch {
      // Storage unavailable: still honour the choice for this session.
    }
  })

  return { size, setSize }
})
