import { useQueryClient } from '@tanstack/vue-query'
import { createApiKey, listApiKeys, revokeApiKey, type CreateApiKeyArgs } from '@/lib/api'
import type { ApiKeyList, CreatedApiKey } from '@/lib/api/generated'
import { useAuthedMutation, useAuthedQuery } from '@/lib/queries'

// Server state for the API-key management UI (issue #284). All three calls require a
// real session; the `useAuthed*` wrappers thread the access token and disable the
// query while signed out. Mutations invalidate the list so the manager re-renders.

const API_KEYS_QUERY_KEY = ['api-keys'] as const

/** The signed-in user's active API keys, newest first (metadata only). */
export function useApiKeysQuery() {
  const options = {
    queryKey: API_KEYS_QUERY_KEY,
    queryFn: (token: string) => listApiKeys(token),
    // Keys change only when the user mints/revokes one (both invalidate below), so a
    // short staleness window is plenty and avoids a refetch on every profile visit.
    staleTime: 60_000,
  }
  return useAuthedQuery<ApiKeyList>(options)
}

/** Mint a new key; invalidates the list. Resolves to the one-time plaintext response. */
export function useCreateApiKeyMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, args: CreateApiKeyArgs) => createApiKey(token, args),
    onSettled: () => qc.invalidateQueries({ queryKey: API_KEYS_QUERY_KEY }),
  }
  return useAuthedMutation<CreatedApiKey, CreateApiKeyArgs>(options)
}

/** Revoke a key by id; invalidates the list. */
export function useRevokeApiKeyMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, id: number) => revokeApiKey(token, id),
    onSettled: () => qc.invalidateQueries({ queryKey: API_KEYS_QUERY_KEY }),
  }
  return useAuthedMutation<void, number>(options)
}
