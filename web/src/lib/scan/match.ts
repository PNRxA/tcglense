import type { Card } from '@/lib/api'
import { normalizeCollectorNumber, type SetHint } from './ocr'
import { canonical, levenshtein } from './similarity'

// Pick which printing a scan resolved to. Pure + unit-tested so the (tiered) matching
// rules are verifiable without a camera. The caller falls back to its own default (the
// newest printing) when this returns null.
//
// Two independent signals arrive per capture, and they are *not* interchangeable:
//
// * the **visual** ranking — the fingerprint match's ranked printings, which is the only
//   signal that can tell a card's treatments apart (normal vs borderless vs full-art);
// * the **OCR'd set line** — the only signal that can tell two printings with the *same*
//   art apart (a reprint carries identical art in a different set).
//
// So the visual ranking decides which artwork, and the set line only ever disambiguates
// among printings the fingerprint could not separate. Ranking the other way round is what
// used to surface a wildly different art as the top pick: a set-code-only hint resolved to
// "the newest printing in that set", and printings within one set share a release date and
// a name, so that ordering tiebreaks on the row id — i.e. an arbitrary treatment.

/** Most edits we'll forgive in an OCR'd set code before refusing to guess. Set codes are
 * 3-5 chars, so a single wrong glyph (O/0, I/1, S/5…) is distance 1; more is too risky. */
const MAX_SET_CODE_EDITS = 1

/**
 * How much worse (in Hamming bits of the 256-bit fingerprint) a printing may rank than the
 * closest visual match and still count as "the fingerprint can't separate these".
 *
 * A photo's distance to every reference is dominated by capture noise (glare, focus,
 * residual skew) that is common to all candidates, so the *gap* between two candidates is
 * the part that carries information. Printings of the same artwork sit within a few bits
 * of each other (they differ only in the set symbol and info line, which barely register in
 * a 16×16 DCT), while a genuinely different artwork lands tens of bits away. This threshold
 * is well above the former but far below the latter, so the set line still arbitrates
 * between same-art reprints while it can no longer overrule a clear visual verdict.
 */
const VISUAL_TIE_BITS = 16

/** One printing the visual scan ranked: its catalog id and Hamming distance to the scanned
 * card (smaller is closer). The narrow shape — rather than the API's `ScanMatch` — keeps
 * these rules testable without building whole card payloads. Callers pass these **in the
 * scan's own nearest-first order**, which is what settles exact distance ties. */
export interface VisualRank {
  id: string
  distance: number
}

/** A printing paired with the distance the scan ranked it at. */
interface RankedPrinting {
  card: Card
  distance: number
}

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

/** The set code to key an OCR hint against: the exact one if a printing has it, else the
 * closest unambiguous near-match (an OCR glyph slip), else null. */
function resolveSet(code: string, prints: Card[]): string | null {
  const hasExact = prints.some((card) => card.set_code.toUpperCase() === code)
  return hasExact ? code : nearestSetCode(code, prints)
}

/** These printings that the scan actually ranked, closest first. Printings the scan did not
 * rank are absent (not "infinitely far"): a missing fingerprint is no evidence either way.
 *
 * Walks `visual`, not `prints` — with a stable sort, equal distances then keep the scan's
 * order rather than the printings listing's. That matters precisely for the treatments this
 * whole module exists to separate: two printings of one artwork routinely tie exactly, and
 * resolving that tie by listing position would hand the pick straight back to the arbitrary
 * row order. */
function rankVisually(prints: Card[], visual: readonly VisualRank[]): RankedPrinting[] {
  if (!visual.length) return []
  const byId = new Map(prints.map((card) => [card.id, card]))
  return visual
    .flatMap((rank) => {
      const card = byId.get(rank.id)
      return card ? [{ card, distance: rank.distance }] : []
    })
    .sort((a, b) => a.distance - b.distance)
}

/** Whether `candidate` is close enough to the best-ranked printing that the fingerprint
 * can't be said to prefer one over the other. */
function visuallyTied(candidate: RankedPrinting, best: RankedPrinting): boolean {
  return candidate.distance - best.distance <= VISUAL_TIE_BITS
}

/**
 * The printing a scan resolved to, or null to fall back to the caller's default.
 *
 * `visual` is the capture's ranked fingerprint matches (any that aren't printings of this
 * card are simply ignored); omit it and the rules degrade to the OCR-only behaviour.
 *
 * - Set code **and** collector number is an exact key (the collector number is unique
 *   within a set, and distinguishes a set's treatments from each other), so it wins — the
 *   one exception being a printing the fingerprint ranked *far* worse than another of this
 *   card's printings, which is a misread digit rather than the card in front of the lens.
 * - Otherwise the visually closest printing wins, with the set code breaking ties: among
 *   printings the fingerprint can't separate, one from the OCR'd set is preferred. A set
 *   code can no longer promote a printing the fingerprint clearly ranked worse.
 * - A set code whose printings the scan never ranked keeps its old meaning (the first
 *   printing in that set): there's no visual evidence to weigh it against — the index can
 *   legitimately not carry that printing yet — so the direct read of the card stands.
 * - A set code that's one glyph off (NE0 -> NEO) is rescued the same way, but only when
 *   exactly one of this card's printings' codes is that close — a near-miss on the tiny
 *   closed set of real printings, not a guess across the whole catalog.
 * - With neither signal (no hint and nothing ranked) it returns null: too ambiguous to
 *   auto-pick, so the caller's newest-printing default stands.
 */
export function matchPrinting(
  prints: Card[],
  hint: SetHint,
  visual: readonly VisualRank[] = [],
): Card | null {
  if (!prints.length) return null
  const code = hint.setCode?.toUpperCase()
  const number = hint.collectorNumber ? normalizeCollectorNumber(hint.collectorNumber) : undefined
  const set = code ? resolveSet(code, prints) : null

  const ranked = rankVisually(prints, visual)
  const best = ranked[0] ?? null

  // An exact set + collector number keys one printing outright. It only loses to the
  // fingerprint when that keyed printing was itself ranked and lost by more than a tie —
  // the scanned card can't look markedly less like its own reference than like another
  // printing's, so a digit was misread.
  if (set && number) {
    const exact = prints.find(
      (card) =>
        card.set_code.toUpperCase() === set &&
        normalizeCollectorNumber(card.collector_number) === number,
    )
    if (exact) {
      const keyed = ranked.find((entry) => entry.card.id === exact.id)
      if (!best || !keyed || visuallyTied(keyed, best)) return exact
      return best.card
    }
  }

  if (!best) {
    // Nothing ranked: the OCR'd set line is all there is (its original behaviour).
    return set ? (prints.find((card) => card.set_code.toUpperCase() === set) ?? null) : null
  }

  if (set) {
    const inSet = ranked.find((entry) => entry.card.set_code.toUpperCase() === set)
    // The set line arbitrates only among printings the fingerprint couldn't separate.
    if (inSet) return visuallyTied(inSet, best) ? inSet.card : best.card
    // Nothing from the hinted set was ranked at all — no visual evidence for or against it,
    // so keep trusting the read of the card itself.
    return prints.find((card) => card.set_code.toUpperCase() === set) ?? best.card
  }

  return best.card
}
