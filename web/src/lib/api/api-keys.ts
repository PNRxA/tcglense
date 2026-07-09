import { request } from './client'
import type { ApiKeyList, CreatedApiKey } from './generated'

// ---------- API keys (per-user, session-authenticated) ----------
//
// The management surface for the long-lived keys that authenticate a user's
// programmatic access to the public API (issue #284), under `/api/auth/api-keys`.
// These calls require a real session (an access `token` from the auth store's
// `authFetch`) — an API key can *use* the API but can never manage keys — so they go
// through the `useAuthed*` composables like every other authenticated call. The
// created key's plaintext comes back exactly once (in `CreatedApiKey.key`); list
// only ever returns metadata.

/** A key's scope: read-only, or read + write. */
export type ApiKeyScope = 'read' | 'read_write'

/** The signed-in user's active API keys, newest first (metadata only). */
export function listApiKeys(token: string): Promise<ApiKeyList> {
  return request<ApiKeyList>('/api/auth/api-keys', { token })
}

/** Arguments for minting a key. `expiresInDays` omitted / null = never expires. */
export interface CreateApiKeyArgs {
  name: string
  scope: ApiKeyScope
  expiresInDays?: number | null
}

/**
 * Mint a new key. The response carries the plaintext `key` **once** — the caller must
 * surface it for copying immediately, as it is unrecoverable afterwards.
 */
export function createApiKey(token: string, args: CreateApiKeyArgs): Promise<CreatedApiKey> {
  return request<CreatedApiKey>('/api/auth/api-keys', {
    method: 'POST',
    token,
    body: {
      name: args.name,
      scope: args.scope,
      expires_in_days: args.expiresInDays ?? null,
    },
  })
}

/** Revoke one of the user's keys by id (idempotent; `404` if it isn't theirs). */
export function revokeApiKey(token: string, id: number): Promise<void> {
  return request<void>(`/api/auth/api-keys/${id}`, { method: 'DELETE', token })
}
