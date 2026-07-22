// Contour-based card-quad candidate extraction for the OpenCV detector: binary mask →
// contours → convex hull → escalating approxPolyDP → gated, scored card-shaped quads.
// Split out of opencvDetect.ts so the detector file owns preprocessing/orchestration and
// this file owns one concern: turning a mask into trustworthy quad candidates.
//
// Robustness constraints preserved from the original in-detector implementation (each
// has a discriminating case in __tests__/opencvDetect.spec.ts):
// - Corners come from each contour's CONVEX HULL: a finger overlapping the edge or a
//   wide glare gap leaves a notched or C-shaped raw contour that never simplifies to a
//   clean 4-gon, but whose hull is still the card's outer quad.
// - The hull is simplified with an escalating approxPolyDP epsilon ladder until it
//   yields exactly 4 corners, and the quad must cover most of the hull — the coverage
//   gate is what keeps a card-aspect ellipse-ish blob from reading as a card.
// - A frame-touching card-like contour registers as a clipped-card blocker that
//   suppresses candidates nested inside it (a printed inner frame must not replace a
//   clipped outer edge as a too-tight crop). Blockers are per-mask (source-local): a
//   merged blob in one edge map must not veto a clean candidate found in another.
//
// The search area may be a crop of the full frame (a SearchWindow). Candidates come
// back in FULL-FRAME normalised coordinates, and every gate keeps full-frame meaning:
// area fractions are of the full frame, clearance is from the physical camera edges.
// A contour touching an ARTIFICIAL crop edge has unknown geometry — it is skipped
// entirely (neither candidate nor blocker), so a crop can never invent a corner or a
// clipped-card veto that the full frame would not have produced.

import { cardFrameInset, orderCorners, quadArea, type Point, type Quad } from './detect'
import { physicalSides, type SearchWindow } from './guidedDetect'
import type { Cv, CvMat, CvMatVector } from './opencvTypes'
import { CARD_ASPECT } from './regions'

/** Relative tolerance on the card aspect ratio (perspective foreshortens it). */
export const ASPECT_TOLERANCE = 0.28

/** A candidate must fill at least this fraction of the frame (the user is told to fill
 * it), and at most {@link MAX_AREA_FRACTION} — a near-full-frame blob is the viewport,
 * not a card (on a portrait phone the frame itself is card-shaped, so aspect alone
 * can't reject it). */
export const MIN_AREA_FRACTION = 0.1
export const MAX_AREA_FRACTION = 0.95

/** approxPolyDP epsilons (fractions of the hull perimeter), tried in order until the
 * hull simplifies to exactly 4 corners. Rounded card corners and residual edge noise
 * often survive the fine epsilon but collapse at a coarser one. */
const APPROX_EPSILONS = [0.02, 0.03, 0.045, 0.065]

/** The 4-corner simplification must cover at least this share of its hull's area —
 * rejects a quad that cut a real corner off rather than absorbing it. */
const MIN_HULL_COVERAGE = 0.8

/** A printed inner frame aligned with a clipped outer contour is not a second card.
 * Suppress it rather than crop away the card border and collector line. */
const MAX_NESTED_CENTER_OFFSET = 0.12

/** How close (px) a contour's bounds may come to an artificial crop edge before its
 * geometry counts as cut off by the crop rather than observed. */
const ARTIFICIAL_EDGE_MARGIN = 2

/** One gated card-shaped quad found in a mask. `quad` is full-frame normalised and
 * ordered [TL, TR, BR, BL]; `areaFraction` is its share of the full frame's area;
 * `shapeError` is its aspect deviation from the true card shape (0 = exact). */
export interface QuadCandidate {
  quad: Quad
  areaFraction: number
  shapeError: number
}

interface Bounds {
  x: number
  y: number
  width: number
  height: number
}

/** Sides of the crop the bounds come within `margin` pixels of. */
function touchedSides(bounds: Bounds, w: number, h: number, margin: number) {
  return {
    left: bounds.x < margin,
    top: bounds.y < margin,
    right: bounds.x + bounds.width - 1 > w - 1 - margin,
    bottom: bounds.y + bounds.height - 1 > h - 1 - margin,
  }
}

function cardLikeBounds(bounds: Bounds): boolean {
  return Math.abs(bounds.width / bounds.height - CARD_ASPECT) <= ASPECT_TOLERANCE
}

function nestedInClippedCard(candidate: Bounds, clipped: Bounds): boolean {
  const candidateRight = candidate.x + candidate.width - 1
  const candidateBottom = candidate.y + candidate.height - 1
  const clippedRight = clipped.x + clipped.width - 1
  const clippedBottom = clipped.y + clipped.height - 1
  if (
    candidate.x < clipped.x ||
    candidate.y < clipped.y ||
    candidateRight > clippedRight ||
    candidateBottom > clippedBottom
  ) {
    return false
  }
  const candidateCx = candidate.x + candidate.width / 2
  const candidateCy = candidate.y + candidate.height / 2
  const clippedCx = clipped.x + clipped.width / 2
  const clippedCy = clipped.y + clipped.height / 2
  return (
    Math.abs(candidateCx - clippedCx) <= clipped.width * MAX_NESTED_CENTER_OFFSET &&
    Math.abs(candidateCy - clippedCy) <= clipped.height * MAX_NESTED_CENTER_OFFSET
  )
}

/** How far a normalised quad's aspect sits from the card's 61:85 (0 = exact), or null
 * when it isn't card-shaped at all: degenerate sides, opposite sides too dissimilar, or
 * aspect beyond tolerance — so text boxes / hands / random rectangles are rejected. */
export function cardShapeError(quad: Quad, frameAspect: number): number | null {
  const [tl, tr, br, bl] = quad
  const dist = (a: Point, b: Point) => Math.hypot((a.x - b.x) * frameAspect, a.y - b.y)
  const top = dist(tl, tr)
  const bottom = dist(bl, br)
  const left = dist(tl, bl)
  const right = dist(tr, br)
  if (Math.min(top, bottom, left, right) <= 0) return null
  if (Math.max(top, bottom) / Math.min(top, bottom) > 1.4) return null
  if (Math.max(left, right) / Math.min(left, right) > 1.4) return null
  const aspect = (top + bottom) / 2 / ((left + right) / 2)
  const err = Math.abs(aspect - CARD_ASPECT)
  return err <= ASPECT_TOLERANCE ? err : null
}

/** Simplify a convex hull to exactly 4 corners via the epsilon ladder, or null when it
 * never resolves to a hull-covering quad. Points are crop pixels. */
function quadFromHull(cv: Cv, hull: CvMat, hullArea: number): Point[] | null {
  const peri = cv.arcLength(hull, true)
  const approx = new cv.Mat()
  try {
    for (const eps of APPROX_EPSILONS) {
      cv.approxPolyDP(hull, approx, eps * peri, true)
      // Already below 4 corners — coarser epsilons only simplify further; give up.
      if (approx.rows < 4) return null
      if (approx.rows === 4) {
        const pts: Point[] = []
        for (let j = 0; j < 4; j++) {
          pts.push({ x: approx.data32S[j * 2]!, y: approx.data32S[j * 2 + 1]! })
        }
        return quadArea(orderCorners(pts)) >= hullArea * MIN_HULL_COVERAGE ? pts : null
      }
    }
    return null
  } finally {
    approx.delete()
  }
}

/** Every corner (full-frame pixels) safely inside the full camera frame's inset. */
function quadClearsPhysicalFrame(quad: Quad, window: SearchWindow): boolean {
  const inset = cardFrameInset(window.fullWidth, window.fullHeight)
  const maxX = window.fullWidth - 1 - inset
  const maxY = window.fullHeight - 1 - inset
  return quad.every((point) => {
    const x = window.x + point.x
    const y = window.y + point.y
    return x >= inset && y >= inset && x <= maxX && y <= maxY
  })
}

/** Trim this fraction off each end of a side before support sampling — rounded card
 * corners legitimately leave the straight line there. */
const SUPPORT_END_TRIM = 0.08
/** Sample the side every this many pixels. */
const SUPPORT_STEP = 2
/** A sample counts as supported when an edge pixel lies within this Chebyshev radius —
 * covers the ~2px the morphological close can shift a contour off the raw edge. */
const SUPPORT_RADIUS = 2

/** Fraction of a quad's (corner-trimmed) perimeter that lies on edge evidence in
 * `edges` (a raw, un-closed edge map covering `window`). 1 = every sampled point sits
 * on an edge. This measures what the area/aspect gates cannot: whether the quad's
 * sides trace a real boundary or cut through empty space (the signature of a hull that
 * absorbed background clutter). Callers use it to arbitrate between detection passes —
 * never as a hard gate, because legitimately recovered geometry (a glare-washed edge
 * bridged by the convex hull) can have one genuinely evidence-free side. */
export function quadEdgeSupport(edges: CvMat, window: SearchWindow, quad: Quad): number {
  const w = window.width
  const h = window.height
  const data = edges.data
  let supported = 0
  let total = 0
  for (let i = 0; i < 4; i++) {
    const a = quad[i]!
    const b = quad[(i + 1) % 4]!
    const ax = a.x * window.fullWidth - window.x
    const ay = a.y * window.fullHeight - window.y
    const bx = b.x * window.fullWidth - window.x
    const by = b.y * window.fullHeight - window.y
    const length = Math.hypot(bx - ax, by - ay)
    if (length < 1) continue
    const span = 1 - 2 * SUPPORT_END_TRIM
    const steps = Math.max(1, Math.floor((length * span) / SUPPORT_STEP))
    for (let s = 0; s <= steps; s++) {
      const t = SUPPORT_END_TRIM + (s / steps) * span
      const x = Math.round(ax + (bx - ax) * t)
      const y = Math.round(ay + (by - ay) * t)
      total++
      let hit = false
      for (let dy = -SUPPORT_RADIUS; dy <= SUPPORT_RADIUS && !hit; dy++) {
        const py = y + dy
        if (py < 0 || py >= h) continue
        for (let dx = -SUPPORT_RADIUS; dx <= SUPPORT_RADIUS; dx++) {
          const px = x + dx
          if (px >= 0 && px < w && data[py * w + px]) {
            hit = true
            break
          }
        }
      }
      if (hit) supported++
    }
  }
  return total === 0 ? 0 : supported / total
}

/** How far inside the perimeter the interior ring sits, as a fraction of the quad's
 * smaller pixel dimension. Deliberately shallower than a physical card border (~4.8% of
 * the width on a standard frame), so the ring samples the border moat, not the printed
 * frame line. */
const INTERIOR_RING_INSET = 0.03
/** Tighter neighbourhood than support sampling: the question is whether structure
 * exists just inside the quad at all, so near-misses must not count. */
const INTERIOR_RADIUS = 1

/** Fraction of an inset interior ring that sits on edge evidence. A genuine card lock
 * has a near-empty ring — the physical card border is an edge-free moat just inside
 * the outline. A quad that leaked outward past the true boundary (a closed texture
 * blob whose card "hole" bled through gaps into neighbouring tiles) contains the real
 * boundary edges INSIDE it and scores high. Perimeter support cannot make this
 * distinction: the leaked quad's sides also lie on real (texture) edges. */
export function quadInteriorEdgeDensity(edges: CvMat, window: SearchWindow, quad: Quad): number {
  const w = window.width
  const h = window.height
  const data = edges.data
  const px = quad.map((p) => ({
    x: p.x * window.fullWidth - window.x,
    y: p.y * window.fullHeight - window.y,
  }))
  let cx = 0
  let cy = 0
  for (const p of px) {
    cx += p.x / 4
    cy += p.y / 4
  }
  let minSide = Infinity
  for (let i = 0; i < 4; i++) {
    const a = px[i]!
    const b = px[(i + 1) % 4]!
    minSide = Math.min(minSide, Math.hypot(b.x - a.x, b.y - a.y))
  }
  const inset = Math.max(3, INTERIOR_RING_INSET * minSide)
  let hits = 0
  let total = 0
  for (let i = 0; i < 4; i++) {
    const a = px[i]!
    const b = px[(i + 1) % 4]!
    const dxs = b.x - a.x
    const dys = b.y - a.y
    const length = Math.hypot(dxs, dys)
    if (length < 1) continue
    // Unit normal pointing toward the centroid (the interior).
    let nx = -dys / length
    let ny = dxs / length
    const mx = (a.x + b.x) / 2
    const my = (a.y + b.y) / 2
    if (nx * (cx - mx) + ny * (cy - my) < 0) {
      nx = -nx
      ny = -ny
    }
    const span = 1 - 2 * SUPPORT_END_TRIM
    const steps = Math.max(1, Math.floor((length * span) / SUPPORT_STEP))
    for (let s = 0; s <= steps; s++) {
      const t = SUPPORT_END_TRIM + (s / steps) * span
      const x = Math.round(a.x + dxs * t + nx * inset)
      const y = Math.round(a.y + dys * t + ny * inset)
      total++
      let hit = false
      for (let dy = -INTERIOR_RADIUS; dy <= INTERIOR_RADIUS && !hit; dy++) {
        const pyr = y + dy
        if (pyr < 0 || pyr >= h) continue
        for (let dx = -INTERIOR_RADIUS; dx <= INTERIOR_RADIUS; dx++) {
          const pxr = x + dx
          if (pxr >= 0 && pxr < w && data[pyr * w + pxr]) {
            hit = true
            break
          }
        }
      }
      if (hit) hits++
    }
  }
  return total === 0 ? 1 : hits / total
}

/** All gated card-shaped quad candidates among the contours of a binary mask (edge map
 * or threshold mask) covering `window`. Candidates are full-frame normalised; the
 * clipped-card blocker suppression is applied within this mask only. */
export function collectCardQuads(cv: Cv, mask: CvMat, window: SearchWindow): QuadCandidate[] {
  const w = window.width
  const h = window.height
  const frameArea = window.fullWidth * window.fullHeight
  const frameAspect = window.fullWidth / window.fullHeight
  const physical = physicalSides(window)
  const inset = cardFrameInset(window.fullWidth, window.fullHeight)
  const candidates: QuadCandidate[] = []
  let contours: CvMatVector | null = null
  let hierarchy: CvMat | null = null
  try {
    contours = new cv.MatVector()
    hierarchy = new cv.Mat()
    cv.findContours(mask, contours, hierarchy, cv.RETR_LIST, cv.CHAIN_APPROX_SIMPLE)
    const candidateContours: Array<{ index: number; bounds: Bounds }> = []
    const clippedCardBounds: Bounds[] = []
    for (let index = 0; index < contours.size(); index++) {
      const contour = contours.get(index)
      try {
        const bounds = cv.boundingRect(contour)
        const area = bounds.width * bounds.height
        // A noisy mask can yield thousands of specks. Only retain contours whose
        // bounds could contain a card, so the hull pass never revisits cardless noise.
        if (area < frameArea * MIN_AREA_FRACTION) continue
        const atPhysical = touchedSides(bounds, w, h, inset)
        const atArtificial = touchedSides(bounds, w, h, ARTIFICIAL_EDGE_MARGIN)
        // Touching an artificial crop edge = geometry cut by the crop: skip silently.
        // Checked FIRST, so cropped geometry can become neither a candidate nor a
        // clipped-card blocker — a contour truncated by the crop has unreliable
        // bounds, and a blocker derived from them could veto a valid nested card the
        // full frame would have kept.
        const touchesArtificial =
          (!physical.left && atArtificial.left) ||
          (!physical.top && atArtificial.top) ||
          (!physical.right && atArtificial.right) ||
          (!physical.bottom && atArtificial.bottom)
        if (touchesArtificial) continue
        // Touching a physical camera edge = possibly clipped card: register the
        // blocker that suppresses its printed inner frame.
        const touchesPhysical =
          (physical.left && atPhysical.left) ||
          (physical.top && atPhysical.top) ||
          (physical.right && atPhysical.right) ||
          (physical.bottom && atPhysical.bottom)
        if (touchesPhysical) {
          if (area <= frameArea * MAX_AREA_FRACTION && cardLikeBounds(bounds)) {
            clippedCardBounds.push(bounds)
          }
          continue
        }
        candidateContours.push({ index, bounds })
      } finally {
        contour.delete()
      }
    }

    for (const { index, bounds } of candidateContours) {
      const cnt = contours.get(index)
      let hull: CvMat | null = null
      try {
        if (clippedCardBounds.some((clipped) => nestedInClippedCard(bounds, clipped))) continue
        // Hull, not the raw contour: a glare-broken (C-shaped) or finger-notched
        // contour has a tiny / non-convex raw outline but a card-shaped hull, and the
        // hull's area is the right size gate for it.
        hull = new cv.Mat()
        cv.convexHull(cnt, hull)
        const hullArea = cv.contourArea(hull)
        if (hullArea < frameArea * MIN_AREA_FRACTION) continue
        if (hullArea > frameArea * MAX_AREA_FRACTION) continue
        const pts = quadFromHull(cv, hull, hullArea)
        if (!pts) continue
        const pixelQuad = orderCorners(pts)
        if (!quadClearsPhysicalFrame(pixelQuad, window)) continue
        const area = quadArea(pixelQuad)
        if (area < frameArea * MIN_AREA_FRACTION) continue
        if (area > frameArea * MAX_AREA_FRACTION) continue
        const quad = pixelQuad.map((p) => ({
          x: (window.x + p.x) / window.fullWidth,
          y: (window.y + p.y) / window.fullHeight,
        })) as Quad
        const err = cardShapeError(quad, frameAspect)
        if (err === null) continue
        candidates.push({ quad, areaFraction: area / frameArea, shapeError: err })
      } finally {
        hull?.delete()
        cnt.delete()
      }
    }
    return candidates
  } finally {
    contours?.delete()
    hierarchy?.delete()
  }
}
