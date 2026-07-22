import { describe, expect, it } from 'vitest'
import {
  CARD_SIZE_GRID_CLASS,
  CARD_SIZE_OPTIONS,
  DECK_CARD_SIZE_GRID_CLASS,
  DETAIL_CARD_SIZE_GRID_CLASS,
} from '../cardSize'

describe('card size grid maps', () => {
  it('covers every offered size in every density map', () => {
    for (const map of [
      CARD_SIZE_GRID_CLASS,
      DETAIL_CARD_SIZE_GRID_CLASS,
      DECK_CARD_SIZE_GRID_CLASS,
    ]) {
      for (const option of CARD_SIZE_OPTIONS) {
        expect(map[option.value]).toBeTruthy()
      }
    }
  })

  it('keeps the deck medium density on the historical deck-grid layout', () => {
    // The pre-#562 deck grid, so the default size is a zero-visual-change rollout.
    expect(DECK_CARD_SIZE_GRID_CLASS.medium).toBe(
      'grid-cols-3 sm:grid-cols-4 md:grid-cols-5 lg:grid-cols-6',
    )
  })

  it('keeps the catalog medium density on the historical catalog layout', () => {
    expect(CARD_SIZE_GRID_CLASS.medium).toBe(
      'grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6',
    )
  })
})
