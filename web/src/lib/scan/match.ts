import type { Card } from '@/lib/api'
import { normalizeCollectorNumber, type SetHint } from './ocr'
import { canonical, levenshtein } from './similarity'

// Pick which printing a scan's set/collector hint points at. Pure + unit-tested so the
// (tiered) matching rules are verifiable without a camera. The caller falls back to its
// own default (the newest printing) when this returns null.

/** Most edits we'll forgive in an OCR'd set code before refusing to guess. Set codes are
 * 3-5 chars, so a single wrong glyph (O/0, I/1, S/5…) is distance 1; more is too risky. */
const MAX_SET_CODE_EDITS = 1

/**
 * The set code among *these* printings that the OCR'd `code` most likely is, or null. The
 * candidate set is tiny and closed (one card's own printings), so a near-match is safe to
 * trust — but only when it's unambiguous: if two codes tie for closest, we don't guess.
 * Returns the printing's actual (uppercased) code so the caller can re-key exactly.
 */
function nearestSetCode(code: string, prints: Card[]): string | null {
  const target = canonical(code)
  let best: string | null = null
  let bestDist = MAX_SET_CODE_EDITS + 1
  let tied = false
  const seen = new Set<string>()
  for (const card of prints) {
    const actual = card.set_code.toUpperCase()
    if (seen.has(actual)) continue
    seen.add(actual)
    const dist = levenshtein(target, canonical(actual))
    if (dist < bestDist) {
      bestDist = dist
      best = actual
      tied = false
    } else if (dist === bestDist) {
      tied = true
    }
  }
  return best && bestDist <= MAX_SET_CODE_EDITS && !tied ? best : null
}

/**
 * The printing a scan hint identifies, or null to fall back to the caller's default.
 *
 * - Set code **and** collector number is an exact key (the collector number is unique
 *   within a set), so it wins outright.
 * - Set code alone takes the newest printing in that set — `prints` is newest-first, so
 *   the first match is newest.
 * - A set code that's one glyph off (NE0 -> NEO) is rescued the same way, but only when
 *   exactly one of this card's printings' codes is that close — a near-miss on the tiny
 *   closed set of real printings, not a guess across the whole catalog.
 * - Anything less (a collector number with no set, or no hint) is too ambiguous to auto
 *   pick — many sets share a collector number — so it returns null.
 */
export function matchPrinting(prints: Card[], hint: SetHint): Card | null {
  if (!prints.length) return null
  const code = hint.setCode?.toUpperCase()
  const number = hint.collectorNumber ? normalizeCollectorNumber(hint.collectorNumber) : undefined
  if (!code) return null

  // The set code to key against: the exact one if a printing has it, else the closest
  // unambiguous near-match (an OCR glyph slip), else give up.
  const hasExact = prints.some((card) => card.set_code.toUpperCase() === code)
  const set = hasExact ? code : nearestSetCode(code, prints)
  if (!set) return null

  if (number) {
    const exact = prints.find(
      (card) =>
        card.set_code.toUpperCase() === set &&
        normalizeCollectorNumber(card.collector_number) === number,
    )
    if (exact) return exact
  }

  return prints.find((card) => card.set_code.toUpperCase() === set) ?? null
}
