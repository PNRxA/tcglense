import { request } from './client'
import type { ScanResponse } from './generated'

// ---------- Visual card scanner (authed) ----------
//
// Identify a photographed card from its client-computed 256-bit perceptual hash. Only
// the 32-byte fingerprint is sent — never the image — so the photo never leaves the
// device (see `lib/scan/phash.ts`). The wire types are generated from the Rust DTOs.

export type { ScanMatch, ScanResponse } from './generated'

/**
 * Identify a card from its fingerprint. Returns the ranked matches, nearest first
 * (`distance` = Hamming distance; smaller is closer). An empty `data` means nothing
 * was within the confidence radius (card not recognised); a `404` means this instance
 * has no fingerprint index built/imported yet.
 */
export function scanCard(
  token: string,
  game: string,
  fingerprint: Uint8Array,
  topK?: number,
): Promise<ScanResponse> {
  const body: { fingerprint: number[]; top_k?: number } = {
    fingerprint: Array.from(fingerprint),
  }
  if (topK !== undefined) body.top_k = topK
  return request<ScanResponse>(`/api/games/${encodeURIComponent(game)}/scan`, {
    method: 'POST',
    body,
    token,
  })
}
