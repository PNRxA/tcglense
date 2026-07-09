import { defineStore } from 'pinia'
import { DEFAULT_BULK_THRESHOLD_CENTS, clampBulkThresholdCents } from '@/lib/bulkThreshold'
import { persistedNumberRef } from '@/lib/persistedRef'

// The chosen bulk threshold is a personal display preference (like the card size / theme),
// so it lives in localStorage and applies everywhere a collection's bulk value is shown.
// It's sent to the collection value endpoints so the server splits the bulk subtotal at
// the user's cutoff (issue #289) — held here in whole USD cents.
const STORAGE_KEY = 'tcglense_bulk_threshold_cents'

export const useBulkThresholdStore = defineStore('bulkThreshold', () => {
  const cents = persistedNumberRef(
    STORAGE_KEY,
    DEFAULT_BULK_THRESHOLD_CENTS,
    clampBulkThresholdCents,
  )

  function setCents(next: number) {
    cents.value = clampBulkThresholdCents(next)
  }

  return { cents, setCents }
})
