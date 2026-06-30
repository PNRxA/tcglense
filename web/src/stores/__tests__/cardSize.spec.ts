import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { nextTick } from 'vue'
import { useCardSizeStore } from '../cardSize'

describe('card size store', () => {
  beforeEach(() => {
    localStorage.clear()
    setActivePinia(createPinia())
  })

  it('defaults to medium when nothing is stored', () => {
    const cardSize = useCardSizeStore()
    expect(cardSize.size).toBe('medium')
  })

  it('reads a persisted choice', () => {
    localStorage.setItem('tcglense_card_size', 'large')
    const cardSize = useCardSizeStore()
    expect(cardSize.size).toBe('large')
  })

  it('ignores an invalid stored value', () => {
    localStorage.setItem('tcglense_card_size', 'enormous')
    const cardSize = useCardSizeStore()
    expect(cardSize.size).toBe('medium')
  })

  it('persists a new choice to localStorage', async () => {
    const cardSize = useCardSizeStore()
    cardSize.setSize('small')
    await nextTick()
    expect(cardSize.size).toBe('small')
    expect(localStorage.getItem('tcglense_card_size')).toBe('small')
  })
})
