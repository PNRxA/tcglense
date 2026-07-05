import type { Card } from '@/lib/api'
import { normalizeCollectorNumber, type SetHint } from './ocr'

// Pick which printing a scan's set/collector hint points at. Pure + unit-tested so the
// (tiered) matching rules are verifiable without a camera. The caller falls back to its
// own default (the newest printing) when this returns null.

/**
 * The printing a scan hint identifies, or null to fall back to the caller's default.
 *
 * - Set code **and** collector number is an exact key (the collector number is unique
 *   within a set), so it wins outright.
 * - Set code alone takes the newest printing in that set — `prints` is newest-first, so
 *   the first match is newest.
 * - Anything less (a collector number with no set, or no hint) is too ambiguous to auto
 *   pick — many sets share a collector number — so it returns null.
 */
export function matchPrinting(prints: Card[], hint: SetHint): Card | null {
  if (!prints.length) return null
  const code = hint.setCode?.toUpperCase()
  const number = hint.collectorNumber ? normalizeCollectorNumber(hint.collectorNumber) : undefined

  if (code && number) {
    const exact = prints.find(
      (card) =>
        card.set_code.toUpperCase() === code &&
        normalizeCollectorNumber(card.collector_number) === number,
    )
    if (exact) return exact
  }

  if (code) {
    const bySet = prints.find((card) => card.set_code.toUpperCase() === code)
    if (bySet) return bySet
  }

  return null
}
