import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { nextTick } from 'vue'
import { useBulkThresholdStore } from '../bulkThreshold'

const KEY = 'tcglense_bulk_threshold_cents'

describe('bulk threshold store', () => {
  beforeEach(() => {
    localStorage.clear()
    setActivePinia(createPinia())
  })

  it('defaults to $1 (100 cents) when nothing is stored', () => {
    expect(useBulkThresholdStore().cents).toBe(100)
  })

  it('reads a persisted choice', () => {
    localStorage.setItem(KEY, '250')
    expect(useBulkThresholdStore().cents).toBe(250)
  })

  it('clamps a non-numeric or out-of-range stored value on read', () => {
    localStorage.setItem(KEY, 'not-a-number')
    expect(useBulkThresholdStore().cents).toBe(100) // falls back to the default

    setActivePinia(createPinia())
    localStorage.setItem(KEY, '-40')
    expect(useBulkThresholdStore().cents).toBe(0) // clamps up to the floor

    setActivePinia(createPinia())
    localStorage.setItem(KEY, '99999999')
    expect(useBulkThresholdStore().cents).toBe(1_000_000) // clamps down to the cap
  })

  it('persists and clamps a new choice', async () => {
    const store = useBulkThresholdStore()
    store.setCents(500)
    await nextTick()
    expect(store.cents).toBe(500)
    expect(localStorage.getItem(KEY)).toBe('500')

    store.setCents(-10)
    await nextTick()
    expect(store.cents).toBe(0)
  })
})
