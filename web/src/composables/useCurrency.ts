import { computed } from 'vue'
import { useQuery } from '@tanstack/vue-query'
import { defineStore, storeToRefs } from 'pinia'
import { getCurrencyRates, setCurrency } from '@/lib/api'
import {
  convertUsd as convertUsdValue,
  formatConvertedUsd,
  isSupportedCurrency,
  type SupportedCurrency,
} from '@/lib/currency'
import { useAuthStore } from '@/stores/auth'
import { useAuthedMutation } from '@/lib/queries'
import { persistedRef } from '@/lib/persistedRef'
import type { User } from '@/lib/api'

const RATES_STALE_MS = 12 * 60 * 60 * 1000
const GUEST_CURRENCY_KEY = 'tcglense_currency'

/** One rates observer per Pinia instance. Card grids can mount dozens of money displays;
 * centralising the observer avoids creating one vue-query subscription per tile while its
 * selected code remains derived from the account or guest preference. */
const useCurrencyState = defineStore('currency-display', () => {
  const auth = useAuthStore()
  const guestCurrency = persistedRef<SupportedCurrency>(
    GUEST_CURRENCY_KEY,
    'USD',
    isSupportedCurrency,
  )
  const currency = computed<SupportedCurrency>(() =>
    isSupportedCurrency(auth.user?.currency) ? auth.user.currency : guestCurrency.value,
  )

  const ratesQuery = useQuery({
    queryKey: ['currency-rates'],
    queryFn: getCurrencyRates,
    enabled: computed(() => currency.value !== 'USD'),
    staleTime: RATES_STALE_MS,
  })

  const rate = computed<number | null>(() => {
    if (currency.value === 'USD') return 1
    const value = ratesQuery.data.value?.rates[currency.value]
    return typeof value === 'number' && Number.isFinite(value) && value > 0 ? value : null
  })

  // If the rate feed is unavailable, monetary values stay visibly USD. This is the label
  // charts use alongside the formatter's equivalent USD fallback.
  const displayCurrency = computed<SupportedCurrency>(() =>
    currency.value === 'USD' || rate.value != null ? currency.value : 'USD',
  )

  function formatUsd(raw: string | null | undefined): string | null {
    return formatConvertedUsd(raw, currency.value, rate.value)
  }

  function convertUsd(raw: string | null): string | null {
    return convertUsdValue(raw, currency.value, rate.value)
  }

  function setGuestCurrency(next: SupportedCurrency) {
    guestCurrency.value = next
  }

  return { currency, displayCurrency, rate, formatUsd, convertUsd, setGuestCurrency }
})

/** The signed-in account's server-persisted display currency, or a device-local preference
 * for signed-out visitors. Account preferences always take precedence while signed in. */
export function useCurrency() {
  const state = useCurrencyState()
  const { currency, displayCurrency, rate } = storeToRefs(state)
  return {
    currency,
    displayCurrency,
    rate,
    formatUsd: state.formatUsd,
    convertUsd: state.convertUsd,
    setGuestCurrency: state.setGuestCurrency,
  }
}

/** Persist the account preference and replace the auth store's user with the returned row,
 * making every mounted money display repaint immediately. */
export function useSetCurrencyMutation() {
  const auth = useAuthStore()
  const options = {
    mutationFn: (token: string, vars: { currency: SupportedCurrency }) =>
      setCurrency(token, vars.currency),
    onSuccess: (user: User) => auth.setUser(user),
  }
  return useAuthedMutation<User, { currency: SupportedCurrency }>(options)
}
