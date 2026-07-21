// OpenCV.js card detection: find the card's four corners in a camera frame robustly
// (adaptive Canny edges → contours → convex hull → best card-shaped quadrilateral), for
// the live outline and a tight, deskewed capture crop. OpenCV.js is a ~13 MB WASM
// payload, so it's lazily imported the first time the scanner starts (never at app
// load), like tesseract.
//
// Robustness constraints (each covers a real hand-held failure mode; each has a
// discriminating case in __tests__/opencvDetect.spec.ts):
// - Canny thresholds must scale with the frame's median luma — constant thresholds
//   yield no edges at all in a dim or low-contrast scene.
// - The edge map is morphologically CLOSED: small glare/focus breaks otherwise split
//   the outline into fragments that individually fail the size/aspect gates.
// - Corners come from each contour's CONVEX HULL: a finger overlapping the edge or a
//   wide glare gap leaves a notched or C-shaped raw contour that never simplifies to a
//   clean 4-gon, but whose hull is still the card's outer quad.
// - The hull is simplified with an escalating approxPolyDP epsilon ladder until it
//   yields exactly 4 corners (rounded corners / residual noise survive a fine epsilon),
//   and the quad must cover most of the hull — the coverage gate is what keeps a
//   card-aspect ellipse-ish blob from reading as a card.
// - When the edge pass finds nothing, an Otsu-threshold segmentation retry catches
//   soft-gradient outlines below any usable edge threshold. It re-reads the whole
//   frame, so the live loop gates its cadence (see DetectOptions).
// - Scoring is area-dominant but weighted toward the true card aspect, so the outer
//   card edge outranks an inner frame rectangle and other near-card distractors.
//
// Corners are returned NORMALISED (0..1 of the frame) so the same quad drives both the
// on-screen outline (any display size) and the capture warp (any capture resolution).
// The lightweight `detect.ts` detector stays as the fallback when OpenCV isn't loaded.

import {
  cardFrameInset,
  orderCorners,
  quadArea,
  quadHasFrameClearance,
  type Point,
  type Quad,
} from './detect'
import { CARD_ASPECT } from './regions'

// The OpenCV runtime is untyped-ish (the shipped d.ts lags the build), so `cv` is `any`;
// the exported surface below is fully typed.
type Cv = {
  matFromImageData: (data: ImageData) => CvMat
  Mat: new () => CvMat
  MatVector: new () => CvMatVector
  Size: new (w: number, h: number) => unknown
  cvtColor: (src: CvMat, dst: CvMat, code: number) => void
  GaussianBlur: (src: CvMat, dst: CvMat, ksize: unknown, sigma: number) => void
  Canny: (src: CvMat, dst: CvMat, t1: number, t2: number) => void
  getStructuringElement: (shape: number, ksize: unknown) => CvMat
  morphologyEx: (src: CvMat, dst: CvMat, op: number, kernel: CvMat) => void
  threshold: (src: CvMat, dst: CvMat, thresh: number, maxval: number, type: number) => number
  findContours: (
    img: CvMat,
    contours: CvMatVector,
    hierarchy: CvMat,
    mode: number,
    method: number,
  ) => void
  convexHull: (src: CvMat, dst: CvMat) => void
  boundingRect: (contour: CvMat) => { x: number; y: number; width: number; height: number }
  arcLength: (curve: CvMat, closed: boolean) => number
  approxPolyDP: (curve: CvMat, approx: CvMat, epsilon: number, closed: boolean) => void
  contourArea: (contour: CvMat) => number
  COLOR_RGBA2GRAY: number
  MORPH_RECT: number
  MORPH_CLOSE: number
  THRESH_BINARY: number
  THRESH_OTSU: number
  RETR_LIST: number
  CHAIN_APPROX_SIMPLE: number
}
interface CvMat {
  rows: number
  data: Uint8Array
  data32S: Int32Array
  delete: () => void
}
interface CvMatVector {
  size: () => number
  get: (i: number) => CvMat
  delete: () => void
}

let cvPromise: Promise<Cv> | null = null

/** Lazily load + initialise the OpenCV.js runtime (cached). Rejects (and clears the
 * cache so a later retry can succeed) if the payload fails to load. */
export function loadOpenCv(): Promise<Cv> {
  if (!cvPromise) {
    cvPromise = import('@techstark/opencv-js')
      .then(async (mod): Promise<Cv> => {
        const cvModule = (mod.default ?? mod) as unknown as Cv & {
          onRuntimeInitialized?: () => void
        }
        if (cvModule instanceof Promise) return (await cvModule) as Cv
        if (cvModule.Mat) return cvModule
        await new Promise<void>((resolve) => {
          cvModule.onRuntimeInitialized = () => resolve()
        })
        return cvModule
      })
      .catch((err) => {
        cvPromise = null
        throw err
      })
  }
  return cvPromise
}

/** Relative tolerance on the card aspect ratio (perspective foreshortens it). */
const ASPECT_TOLERANCE = 0.28

/** A candidate must fill at least this fraction of the frame (the user is told to fill
 * it), and at most {@link MAX_AREA_FRACTION} — a near-full-frame blob is the viewport,
 * not a card (on a portrait phone the frame itself is card-shaped, so aspect alone
 * can't reject it). */
const MIN_AREA_FRACTION = 0.1
const MAX_AREA_FRACTION = 0.95

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

interface Bounds {
  x: number
  y: number
  width: number
  height: number
}

function touchesFrameInset(bounds: Bounds, w: number, h: number, inset: number): boolean {
  return (
    bounds.x < inset ||
    bounds.y < inset ||
    bounds.x + bounds.width - 1 > w - 1 - inset ||
    bounds.y + bounds.height - 1 > h - 1 - inset
  )
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

/** Median of an 8-bit grayscale plane, via a 256-bin histogram. Exported for tests. */
export function medianLuma(pixels: Uint8Array): number {
  if (pixels.length === 0) return 127
  const hist = new Uint32Array(256)
  for (let i = 0; i < pixels.length; i++) {
    const v = pixels[i]!
    hist[v] = (hist[v] ?? 0) + 1
  }
  const half = pixels.length / 2
  let acc = 0
  for (let v = 0; v < 256; v++) {
    acc += hist[v]!
    if (acc >= half) return v
  }
  return 127
}

/** How far a normalised quad's aspect sits from the card's 61:85 (0 = exact), or null
 * when it isn't card-shaped at all: degenerate sides, opposite sides too dissimilar, or
 * aspect beyond tolerance — so text boxes / hands / random rectangles are rejected. */
function cardShapeError(quad: Quad, frameAspect: number): number | null {
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
 * never resolves to a hull-covering quad. Points are frame pixels. */
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

/** Best card-shaped quad among the contours of a binary image (edge map or threshold
 * mask), as { quad (normalised, ordered), score }, or null. Score is area × an aspect-
 * fit factor in [0.5, 1] — area-dominant (the card is the biggest card-shaped thing in
 * frame), tie-broken toward the truer card shape so the outer card edge beats an inner
 * frame rectangle. */
function bestCardQuad(
  cv: Cv,
  mask: CvMat,
  w: number,
  h: number,
): { quad: Quad; score: number } | null {
  const frameAspect = w / h
  const contours = new cv.MatVector()
  const hierarchy = new cv.Mat()
  try {
    cv.findContours(mask, contours, hierarchy, cv.RETR_LIST, cv.CHAIN_APPROX_SIMPLE)
    const frameArea = w * h
    const inset = cardFrameInset(w, h)
    const candidateContours: Array<{ index: number; bounds: Bounds }> = []
    const clippedCardBounds: Bounds[] = []
    for (let index = 0; index < contours.size(); index++) {
      const contour = contours.get(index)
      try {
        const bounds = cv.boundingRect(contour)
        const area = bounds.width * bounds.height
        // A noisy Otsu mask can yield thousands of specks. Only retain contours whose
        // bounds could contain a card, so the hull pass never revisits cardless noise.
        if (area < frameArea * MIN_AREA_FRACTION) continue
        if (touchesFrameInset(bounds, w, h, inset)) {
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

    let best: { quad: Quad; score: number } | null = null
    for (const { index, bounds } of candidateContours) {
      const cnt = contours.get(index)
      if (clippedCardBounds.some((clipped) => nestedInClippedCard(bounds, clipped))) {
        cnt.delete()
        continue
      }
      const hull = new cv.Mat()
      try {
        // Hull, not the raw contour: a glare-broken (C-shaped) or finger-notched
        // contour has a tiny / non-convex raw outline but a card-shaped hull, and the
        // hull's area is the right size gate for it.
        cv.convexHull(cnt, hull)
        const hullArea = cv.contourArea(hull)
        if (hullArea < frameArea * MIN_AREA_FRACTION) continue
        if (hullArea > frameArea * MAX_AREA_FRACTION) continue
        const pts = quadFromHull(cv, hull, hullArea)
        if (!pts) continue
        const pixelQuad = orderCorners(pts)
        if (!quadHasFrameClearance(pixelQuad, w, h)) continue
        const area = quadArea(pixelQuad)
        if (area < frameArea * MIN_AREA_FRACTION) continue
        if (area > frameArea * MAX_AREA_FRACTION) continue
        const quad = pixelQuad.map((p) => ({ x: p.x / w, y: p.y / h })) as Quad
        const err = cardShapeError(quad, frameAspect)
        if (err === null) continue
        const score = area * (1 - 0.5 * (err / ASPECT_TOLERANCE))
        if (!best || score > best.score) best = { quad, score }
      } finally {
        hull.delete()
        cnt.delete()
      }
    }
    return best
  } finally {
    contours.delete()
    hierarchy.delete()
  }
}

export interface DetectOptions {
  /** Whether the Otsu segmentation retry may run when the edge pass finds nothing.
   * It reads the whole frame again and is the expensive half of a cardless tick, so
   * the live loop only enables it on a cadence of misses (the outline tracker bridges
   * the added latency); a one-off capture-path detect should leave it on (default). */
  segmentationFallback?: boolean
}

/** Detect the most card-shaped quadrilateral in `imageData`, or null. Corners are
 * returned normalised (0..1) and ordered [TL, TR, BR, BL]. All OpenCV mats are freed. */
export function detectCardQuadCv(
  cv: Cv,
  imageData: ImageData,
  options: DetectOptions = {},
): Quad | null {
  const w = imageData.width
  const h = imageData.height
  const src = cv.matFromImageData(imageData)
  const gray = new cv.Mat()
  const blur = new cv.Mat()
  // Reused for the edge map, then (only if that pass fails) the Otsu mask.
  const mask = new cv.Mat()
  try {
    cv.cvtColor(src, gray, cv.COLOR_RGBA2GRAY)
    cv.GaussianBlur(gray, blur, new cv.Size(5, 5), 0)

    // Pass 1: edges, with Canny thresholds scaled to the frame's median luma (the
    // classic auto-Canny ±33% band) so dim or low-contrast scenes still produce them.
    const median = medianLuma(blur.data)
    const lower = Math.max(10, Math.round(median * 0.67))
    const upper = Math.max(lower + 20, Math.min(255, Math.round(median * 1.33)))
    cv.Canny(blur, mask, lower, upper)
    const kernel = cv.getStructuringElement(cv.MORPH_RECT, new cv.Size(5, 5))
    cv.morphologyEx(mask, mask, cv.MORPH_CLOSE, kernel)
    kernel.delete()
    let best = bestCardQuad(cv, mask, w, h)

    // Pass 2 (edge pass empty, and the caller allows it): Otsu segmentation — catches
    // soft-gradient outlines where edges are too weak, and is polarity-agnostic (a
    // dark card region is a hole contour, which RETR_LIST also traces).
    if (!best && (options.segmentationFallback ?? true)) {
      cv.threshold(blur, mask, 0, 255, cv.THRESH_BINARY + cv.THRESH_OTSU)
      best = bestCardQuad(cv, mask, w, h)
    }
    return best?.quad ?? null
  } finally {
    src.delete()
    gray.delete()
    blur.delete()
    mask.delete()
  }
}
