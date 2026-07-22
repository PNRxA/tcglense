// Pure candidate-selection policy for the card detector: given the gated quad
// candidates a detection pass produced, pick the one the scanner should trust — or
// none. Selection is mode-aware, which is what makes lock-on resilient on busy
// backgrounds without ever loosening the geometric gates themselves:
//
// - 'plain' reproduces the classic detector exactly: area-dominant, aspect-weighted.
// - 'acquisition' adds a spatial prior for the first lock: the user aims the card at
//   the centred guide box, so a card-aspect distractor elsewhere in a cluttered scene
//   (a book, a phone) must not outrank the card at the guide. Far-off candidates are
//   ineligible outright, near-guide candidates are up-weighted, and a near-tie between
//   spatially distinct candidates yields null — an ambiguous frame should delay the
//   green lock, not gamble it (a wrong lock invites a wrong capture).
// - 'tracking' associates against the current lock: only candidates that plausibly ARE
//   the same card between two adjacent ticks are eligible, so the green outline can
//   never snap to a different object across the room; everything else is a miss the
//   tracker's hold bridges.
// - 'capture' is the tracking association at the capture path's stricter thresholds
//   (the crop that gets hashed must be the card the user saw locked).
//
// No OpenCV and no DOM here — plain math over normalised quads, unit-tested without
// the WASM runtime.

import { quadArea, type Quad } from './detect'
import { ASPECT_TOLERANCE, type QuadCandidate } from './opencvContours'
import { cornerMetrics } from './quadTracker'
import { guideRect } from './regions'

/** Where the user is told to aim the card, in full-frame normalised coordinates. */
export interface GuideTarget {
  cx: number
  cy: number
  halfW: number
  halfH: number
}

/** The on-screen guide box as a normalised acquisition target for a frame. */
export function guideTarget(fullWidth: number, fullHeight: number): GuideTarget {
  const rect = guideRect(fullWidth, fullHeight)
  return {
    cx: (rect.left + rect.width / 2) / fullWidth,
    cy: (rect.top + rect.height / 2) / fullHeight,
    halfW: rect.width / 2 / fullWidth,
    halfH: rect.height / 2 / fullHeight,
  }
}

export type QuadSelection =
  | { mode: 'plain' }
  | { mode: 'acquisition'; guide: GuideTarget }
  | { mode: 'tracking'; prior: Quad }
  | { mode: 'capture'; prior: Quad }

/** Candidates beyond this guide-normalised centre distance cannot acquire a lock —
 * the card the user is scanning is at the guide, anything this far off is scenery. */
const GUIDE_MAX_DISTANCE = 1.35
/** Gaussian falloff scale of the acquisition guide weight. */
const GUIDE_SIGMA = 0.65

/** Association gates: how far a candidate may sit from the prior and still be the same
 * physical card one tick later (tracking) or between the live lock and the capture
 * re-detection (capture, stricter — mirrors the historical capture agreement gate). */
const TRACKING_MAX_MEAN = 0.075
const TRACKING_MAX_CORNER = 0.12
const TRACKING_AREA_RATIO = [0.88, 1.15] as const
const CAPTURE_MAX_MEAN = 0.05
const CAPTURE_MAX_CORNER = 0.08
const CAPTURE_AREA_RATIO = [0.9, 1.1] as const

/** Continuity/scale falloff scales for prior-associated scoring. */
const CONTINUITY_SIGMA = 0.04
const SCALE_SIGMA = 0.12

/** Same-card tolerance for deduplicating candidates before the ambiguity check. Both
 * one mask and different edge maps legitimately estimate the same physical card with
 * corners several hundredths apart (a closed blob's hull dilates, a dense map's hole
 * hugs the inner boundary), and those variants are agreement, not an ambiguous pair.
 * Two genuinely distinct side-by-side cards differ by at least a card width (~0.3),
 * far beyond the corner radius; a printed inner frame nested in its card stays
 * distinct through the area-ratio bound (0.8² = 0.64 < 0.7) and is instead dominated
 * on score, preserving the outer-edge preference. */
const SAME_CARD_MAX_CORNER = 0.1
const SAME_CARD_AREA_RATIO = [0.7, 1.4] as const

/** A spatially distinct runner-up scoring at least this share of the top score makes
 * the frame ambiguous: refuse to pick rather than lock onto an arbitrary rectangle.
 * Acquisition is the cautious end (no prior to arbitrate); with a prior the
 * association gate has already done the arbitration, so near-ties are rarer and the
 * bar sits higher. */
const ACQUISITION_AMBIGUITY_RATIO = 0.9
const ASSOCIATED_AMBIGUITY_RATIO = 0.96

interface Scored {
  candidate: QuadCandidate
  score: number
}

function baseScore(candidate: QuadCandidate): number {
  return candidate.areaFraction * (1 - 0.5 * (candidate.shapeError / ASPECT_TOLERANCE))
}

function quadCenter(quad: Quad): { x: number; y: number } {
  let x = 0
  let y = 0
  for (const p of quad) {
    x += p.x / 4
    y += p.y / 4
  }
  return { x, y }
}

function gaussian(deviation: number, sigma: number): number {
  return Math.exp(-0.5 * (deviation / sigma) ** 2)
}

function scoreAgainstPrior(
  candidate: QuadCandidate,
  prior: Quad,
  maxMean: number,
  maxCorner: number,
  areaRatioRange: readonly [number, number],
): number | null {
  const priorArea = quadArea(prior)
  if (priorArea <= 0) return null
  const metrics = cornerMetrics(prior, candidate.quad)
  if (metrics.mean > maxMean || metrics.max > maxCorner) return null
  const areaRatio = quadArea(candidate.quad) / priorArea
  if (areaRatio < areaRatioRange[0] || areaRatio > areaRatioRange[1]) return null
  const continuity = gaussian(metrics.mean, CONTINUITY_SIGMA)
  const scaleFit = gaussian(Math.log(areaRatio), SCALE_SIGMA)
  return baseScore(candidate) * (0.15 + 0.85 * continuity * scaleFit)
}

function scoreCandidate(candidate: QuadCandidate, selection: QuadSelection): number | null {
  switch (selection.mode) {
    case 'plain':
      return baseScore(candidate)
    case 'acquisition': {
      const { guide } = selection
      const center = quadCenter(candidate.quad)
      const d = Math.hypot((center.x - guide.cx) / guide.halfW, (center.y - guide.cy) / guide.halfH)
      if (d > GUIDE_MAX_DISTANCE) return null
      return baseScore(candidate) * (0.4 + 0.6 * gaussian(d, GUIDE_SIGMA))
    }
    case 'tracking':
      return scoreAgainstPrior(
        candidate,
        selection.prior,
        TRACKING_MAX_MEAN,
        TRACKING_MAX_CORNER,
        TRACKING_AREA_RATIO,
      )
    case 'capture':
      return scoreAgainstPrior(
        candidate,
        selection.prior,
        CAPTURE_MAX_MEAN,
        CAPTURE_MAX_CORNER,
        CAPTURE_AREA_RATIO,
      )
  }
}

function sameDetection(a: Quad, b: Quad): boolean {
  if (cornerMetrics(a, b).max > SAME_CARD_MAX_CORNER) return false
  const areaA = quadArea(a)
  const areaB = quadArea(b)
  if (areaA <= 0 || areaB <= 0) return false
  const ratio = areaB / areaA
  return ratio >= SAME_CARD_AREA_RATIO[0] && ratio <= SAME_CARD_AREA_RATIO[1]
}

/** A selected quad with the mode score that chose it, so the caller can arbitrate
 * between selections made from different detection passes. */
export interface SelectedQuad {
  quad: Quad
  score: number
}

/** One pass's selection outcome. `ambiguous` is distinct from "nothing eligible": an
 * ambiguous pass SAW spatially distinct near-tied candidates, and the caller must
 * refuse the whole frame rather than fall back to a winner some other pass banked —
 * conflating the two would let an earlier pass's provisional candidate survive
 * ambiguity a later, more inclusive pass discovered. */
export interface QuadSelectionResult {
  selected: SelectedQuad | null
  ambiguous: boolean
}

/** The mode's ambiguity bar: how close (as a score ratio) a spatially distinct
 * runner-up must come to the winner before the frame is refused. */
function ambiguityRatioFor(selection: QuadSelection): number {
  return selection.mode === 'acquisition' ? ACQUISITION_AMBIGUITY_RATIO : ASSOCIATED_AMBIGUITY_RATIO
}

/** Pick the quad the scanner should trust from a pass's candidates. `selected` is null
 * when no candidate is eligible for the mode or the frame is ambiguous (see
 * {@link QuadSelectionResult}). */
export function selectCardQuadResult(
  candidates: QuadCandidate[],
  selection: QuadSelection,
): QuadSelectionResult {
  const scored: Scored[] = []
  for (const candidate of candidates) {
    const score = scoreCandidate(candidate, selection)
    if (score !== null) scored.push({ candidate, score })
  }
  if (scored.length === 0) return { selected: null, ambiguous: false }
  scored.sort((a, b) => b.score - a.score)

  // Same-mechanism duplicates (two passes seeing one card) collapse to the best one.
  const distinct: Scored[] = []
  for (const entry of scored) {
    if (!distinct.some((kept) => sameDetection(kept.candidate.quad, entry.candidate.quad))) {
      distinct.push(entry)
    }
  }

  const top = distinct[0]!
  if (selection.mode !== 'plain' && distinct.length > 1) {
    if (distinct[1]!.score >= top.score * ambiguityRatioFor(selection)) {
      return { selected: null, ambiguous: true }
    }
  }
  return { selected: { quad: top.candidate.quad, score: top.score }, ambiguous: false }
}

/** Whether winners banked by different detection passes are near-tied distinct quads —
 * the cross-pass counterpart of the in-pass ambiguity refusal, for two cards that only
 * ever surface in different edge maps. Winners are merged under the same-card
 * tolerance first: several maps estimating one card differently is agreement, not
 * ambiguity. */
export function crossPassAmbiguous(winners: SelectedQuad[], selection: QuadSelection): boolean {
  if (selection.mode === 'plain' || winners.length < 2) return false
  const sorted = [...winners].sort((a, b) => b.score - a.score)
  const distinct: SelectedQuad[] = []
  for (const entry of sorted) {
    if (!distinct.some((kept) => sameDetection(kept.quad, entry.quad))) distinct.push(entry)
  }
  if (distinct.length < 2) return false
  return distinct[1]!.score >= distinct[0]!.score * ambiguityRatioFor(selection)
}

/** {@link selectCardQuadResult}'s winner alone, for callers that only display. */
export function selectCardQuad(candidates: QuadCandidate[], selection: QuadSelection): Quad | null {
  return selectCardQuadResult(candidates, selection).selected?.quad ?? null
}
