import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { nextTick } from 'vue'
import { useScanPreferencesStore } from '../scanPreferences'

describe('scan preferences store', () => {
  beforeEach(() => {
    localStorage.clear()
    setActivePinia(createPinia())
  })

  it('defaults auto-scroll to review on', () => {
    const prefs = useScanPreferencesStore()
    expect(prefs.autoScrollToReview).toBe(true)
  })

  it('reads a persisted off choice', () => {
    localStorage.setItem('tcglense_scan_auto_scroll_review', '0')
    const prefs = useScanPreferencesStore()
    expect(prefs.autoScrollToReview).toBe(false)
  })

  it('reads a persisted on choice', () => {
    localStorage.setItem('tcglense_scan_auto_scroll_review', '1')
    const prefs = useScanPreferencesStore()
    expect(prefs.autoScrollToReview).toBe(true)
  })

  it('persists a new choice to localStorage', async () => {
    const prefs = useScanPreferencesStore()
    prefs.setAutoScrollToReview(false)
    await nextTick()
    expect(prefs.autoScrollToReview).toBe(false)
    expect(localStorage.getItem('tcglense_scan_auto_scroll_review')).toBe('0')

    prefs.setAutoScrollToReview(true)
    await nextTick()
    expect(localStorage.getItem('tcglense_scan_auto_scroll_review')).toBe('1')
  })
})
