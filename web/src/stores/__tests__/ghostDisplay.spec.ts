import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { nextTick } from 'vue'
import { useGhostDisplayStore } from '../ghostDisplay'

describe('ghost display store', () => {
  beforeEach(() => {
    localStorage.clear()
    setActivePinia(createPinia())
  })

  describe('ghost style', () => {
    it('defaults to grayscale when nothing is stored', () => {
      expect(useGhostDisplayStore().style).toBe('grayscale')
    })

    it('reads a persisted choice', () => {
      localStorage.setItem('tcglense_ghost_style', 'color')
      expect(useGhostDisplayStore().style).toBe('color')
    })

    it('ignores an invalid stored value', () => {
      localStorage.setItem('tcglense_ghost_style', 'sepia')
      expect(useGhostDisplayStore().style).toBe('grayscale')
    })

    it('persists a new choice to localStorage', async () => {
      const store = useGhostDisplayStore()
      store.setStyle('color')
      await nextTick()
      expect(store.style).toBe('color')
      expect(localStorage.getItem('tcglense_ghost_style')).toBe('color')
    })
  })

  describe('show owned', () => {
    it('defaults to off when nothing is stored', () => {
      expect(useGhostDisplayStore().showOwned).toBe(false)
    })

    it('reads a persisted true ("1") and false ("0")', () => {
      localStorage.setItem('tcglense_ghost_show_owned', '1')
      expect(useGhostDisplayStore().showOwned).toBe(true)
      setActivePinia(createPinia())
      localStorage.setItem('tcglense_ghost_show_owned', '0')
      expect(useGhostDisplayStore().showOwned).toBe(false)
    })

    it('persists the flag as "1"/"0"', async () => {
      const store = useGhostDisplayStore()
      store.setShowOwned(true)
      await nextTick()
      expect(store.showOwned).toBe(true)
      expect(localStorage.getItem('tcglense_ghost_show_owned')).toBe('1')
      store.setShowOwned(false)
      await nextTick()
      expect(localStorage.getItem('tcglense_ghost_show_owned')).toBe('0')
    })
  })
})
