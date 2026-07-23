import { defineStore } from 'pinia'
import { persistedBoolRef } from '@/lib/persistedRef'

// Personal behaviour preferences for the card scanner (like the card size / theme), so they
// live in localStorage rather than server state and follow the browser they were set in.
const AUTO_SCROLL_TO_REVIEW_KEY = 'tcglense_scan_auto_scroll_review'

export const useScanPreferencesStore = defineStore('scanPreferences', () => {
  // On the single-column (mobile/tablet) layout the review panel sits below the camera, so a
  // fresh match lands off-screen. When on (the default), a successful scan scrolls the review
  // into view automatically; turn it off to keep the camera framed and review on demand.
  const autoScrollToReview = persistedBoolRef(AUTO_SCROLL_TO_REVIEW_KEY, true)

  function setAutoScrollToReview(next: boolean) {
    autoScrollToReview.value = next
  }

  return { autoScrollToReview, setAutoScrollToReview }
})
