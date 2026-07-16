import { QueryClient } from '@tanstack/vue-query'
import { ApiError } from '@/lib/api'

/**
 * Retry policy for queries. Client errors (4xx) won't fix themselves on a retry —
 * and a 401 is already handled *below* the cache by the auth store's `authFetch`
 * (refresh + retry once + logout), so by the time vue-query sees a 401 the session
 * is genuinely gone. Retry only network/5xx failures, a couple of times.
 */
export function shouldRetryQuery(failureCount: number, error: Error): boolean {
  if (error instanceof ApiError && error.status >= 400 && error.status < 500) {
    return false
  }
  return failureCount < 2
}

/**
 * staleTime for public catalog queries whose payload embeds the daily-refreshed
 * prices (card detail/grids/prints embed `Card.prices`; product payloads carry
 * `price_usd`). Catalog data turns over at most once a day, and — because the API's
 * ETag layer runs after the handler — a client that re-asks costs the server full
 * DB work even on a 304, so not asking is what actually saves the backend (#413).
 * An hour keeps the embedded prices honest to the daily sync while absorbing the
 * tab-refocus/navigation refetch storms the 5-minute default allows.
 */
export const PRICED_CATALOG_STALE_MS = 60 * 60 * 1000

/**
 * staleTime for structural public catalog queries with no price fields at all
 * (the set list / one set's metadata): same daily cadence, nothing that drifts
 * intra-day, so they can stay fresh much longer (#413).
 */
export const STRUCTURAL_CATALOG_STALE_MS = 6 * 60 * 60 * 1000

/**
 * Build the app's QueryClient. A factory (not a module singleton) so tests can spin
 * up an isolated cache per case.
 */
export function createQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        // Price/set data updates at most daily, so treat data as fresh for a few
        // minutes: serve it instantly and skip refetch-on-mount/focus within the
        // window, while refetchOnWindowFocus/Reconnect (on by default) still pick up
        // the trailing point once it goes stale. Override per-query as needed —
        // `staleTime: Infinity` for static set definitions, shorter for live prices.
        staleTime: 5 * 60 * 1000,
        // Keep unused entries an idle-return's worth so warm caches survive a tab away.
        gcTime: 30 * 60 * 1000,
        retry: shouldRetryQuery,
      },
      // Mutations keep the default retry: 0 — a failed collection write should
      // surface immediately so optimistic updates roll back, not silently retry.
    },
  })
}
