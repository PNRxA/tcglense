import {
  useMutation,
  useQuery,
  type UseMutationOptions,
  type UseQueryOptions,
} from '@tanstack/vue-query'
import type { ApiError } from '@/lib/api'
import { useAuthStore } from '@/stores/auth'

/**
 * `useQuery` for authenticated endpoints — the canonical way to read server state.
 *
 * The `queryFn` is handed a valid access token; the auth store's `authFetch`
 * transparently restores/refreshes it and retries once on a 401, so individual
 * queries never deal with token expiry. Failures surface as {@link ApiError}, and
 * the global retry policy (see `queryClient.ts`) skips 4xx.
 *
 * Reactivity reminder: put reactive params *inside* `queryKey` as refs/computed
 * (e.g. `['prices', productId, range]`), never `productId.value` — otherwise the
 * value is baked in and refetch-on-change silently breaks.
 *
 * ```ts
 * const { data, isPending, error } = useAuthedQuery({
 *   queryKey: ['prices', productId, range],
 *   queryFn: (token) => api.priceHistory(token, productId.value, range.value),
 * })
 * ```
 */
export function useAuthedQuery<TData>(
  options: Omit<UseQueryOptions<TData, ApiError, TData>, 'queryFn'> & {
    queryFn: (token: string) => Promise<TData>
  },
) {
  const auth = useAuthStore()
  const { queryFn } = options
  // The merged object is a valid UseQueryOptions at runtime, but vue-query's deeply
  // reactive option types defeat TS inference through the spread (it can't see the
  // required queryKey), so bridge via `unknown`. Caller-facing typing on `options`
  // (incl. the required queryKey) is unaffected.
  return useQuery<TData, ApiError, TData>({
    ...options,
    queryFn: () => auth.authFetch(queryFn),
  } as unknown as UseQueryOptions<TData, ApiError, TData>)
}

/**
 * `useMutation` for authenticated writes (e.g. collection edits). The `mutationFn`
 * is handed a valid access token plus the call's variables. Pair with
 * `queryClient.invalidateQueries` in `onSettled` to refresh dependent views
 * (collection list, set-completion %, valuation) after a write.
 *
 * ```ts
 * const qc = useQueryClient()
 * const toggle = useAuthedMutation({
 *   mutationFn: (token, vars: ToggleVars) => api.toggleOwned(token, vars),
 *   onSettled: () => qc.invalidateQueries({ queryKey: ['collection'] }),
 * })
 * ```
 */
export function useAuthedMutation<TData, TVariables = void>(
  options: Omit<UseMutationOptions<TData, ApiError, TVariables>, 'mutationFn'> & {
    mutationFn: (token: string, variables: TVariables) => Promise<TData>
  },
) {
  const auth = useAuthStore()
  const { mutationFn } = options
  return useMutation<TData, ApiError, TVariables>({
    ...options,
    mutationFn: (variables) => auth.authFetch((token) => mutationFn(token, variables)),
  } as UseMutationOptions<TData, ApiError, TVariables>)
}
