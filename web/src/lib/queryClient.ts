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
