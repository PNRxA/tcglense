import { scanCard, type ScanResponse } from '@/lib/api'
import { useAuthedMutation } from '@/lib/queries'

// Imperative visual-scan lookup for the scanner loop. It's an authed action (not
// declarative, cacheable server state), so it goes through `useAuthedMutation` — which
// routes the call through the auth store's `authFetch` (token restore/refresh + one
// 401 retry) — and is driven with `mutateAsync` from `useScanSession`.

export interface ScanVars {
  game: string
  /** The 32-byte perceptual hashes of the cropped card (base crop + geometric variants). */
  fingerprints: Uint8Array[]
  /** How many ranked matches to request; omit for the server default. */
  topK?: number
}

/** A mutation that identifies a scanned card from its fingerprint variants. */
export function useScanMutation() {
  return useAuthedMutation<ScanResponse, ScanVars>({
    mutationFn: (token, vars) => scanCard(token, vars.game, vars.fingerprints, vars.topK),
  })
}
