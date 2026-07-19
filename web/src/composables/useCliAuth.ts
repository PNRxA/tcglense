import { cliAuthorize, type CliAuthorizeArgs } from '@/lib/api'
import type { CliAuthorizeResponse } from '@/lib/api/generated'
import { useAuthedMutation } from '@/lib/queries'

// Server state for the CLI browser (loopback) sign-in consent page (`/cli-login`).
// The `useAuthed*` wrapper threads the session access token, so the mint is
// session-only — exactly what the server's `SessionUser` extractor enforces.

/** Mint a one-time CLI authorization code for the signed-in user. */
export function useCliAuthorizeMutation() {
  return useAuthedMutation<CliAuthorizeResponse, CliAuthorizeArgs>({
    mutationFn: (token, args) => cliAuthorize(token, args),
  })
}
