// OpenCV.js card detection: find the card's four corners in a camera frame robustly
// (adaptive Canny edges → contours → convex hull → best card-shaped quadrilateral), for
// the live outline and a tight, deskewed capture crop. OpenCV.js is a ~13 MB WASM
// payload, so it's lazily imported the first time the scanner starts (never at app
// load), like tesseract.
//
// Robustness choices (each covers a real hand-held failure mode):
// - Canny thresholds adapt to the frame's median luma, so a dim room or a low-contrast
//   card/table pairing still yields edges (fixed thresholds went blind there).
// - The edge map is morphologically CLOSED (not just dilated), bridging small glare /
//   focus breaks in the card outline without fattening it outward.
// - Corners come from each contour's CONVEX HULL, so a finger overlapping the edge or a
//   glare gap that leaves a C-shaped contour still resolves to the card's outer quad —
//   the raw contour of either is nowhere near a clean 4-gon.
// - The hull is simplified with an escalating approxPolyDP epsilon ladder until it gives
//   exactly 4 corners (rounded corners / edge noise often need a coarser epsilon), and
//   the quad must cover most of the hull so a degenerate collapse can't stand in for it.
// - When the edge pass finds nothing, an Otsu-threshold segmentation pass retries —
//   catching soft-gradient outlines Canny under-fires on (uniform light, matte sleeves).
// - Among candidates, the score is area-dominant but nudged toward the truer card
//   aspect, so the outer card edge beats an inner frame rectangle of similar size.
//
// Corners are returned NORMALISED (0..1 of the frame) so the same quad drives both the
// on-screen outline (any display size) and the capture warp (any capture resolution).
// The lightweight `detect.ts` detector stays as the fallback when OpenCV isn't loaded.

import { orderCorners, quadArea, type Point, type Quad } from './detect'
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
    let best: { quad: Quad; score: number } | null = null
    for (let i = 0; i < contours.size(); i++) {
      const cnt = contours.get(i)
      const hull = new cv.Mat()
      try {
        // Hull first: a glare-broken (C-shaped) or finger-notched contour has a tiny /
        // non-convex raw outline but a card-shaped hull, and the hull's area is the
        // right size gate for it.
        cv.convexHull(cnt, hull)
        const hullArea = cv.contourArea(hull)
        if (hullArea < frameArea * MIN_AREA_FRACTION) continue
        if (hullArea > frameArea * MAX_AREA_FRACTION) continue
        const pts = quadFromHull(cv, hull, hullArea)
        if (!pts) continue
        const quad = orderCorners(pts.map((p) => ({ x: p.x / w, y: p.y / h })))
        const err = cardShapeError(quad, frameAspect)
        if (err === null) continue
        const score = hullArea * (1 - 0.5 * (err / ASPECT_TOLERANCE))
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

/** Detect the most card-shaped quadrilateral in `imageData`, or null. Corners are
 * returned normalised (0..1) and ordered [TL, TR, BR, BL]. All OpenCV mats are freed. */
export function detectCardQuadCv(cv: Cv, imageData: ImageData): Quad | null {
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

    // Pass 2 (edge pass empty): Otsu segmentation — catches soft-gradient outlines
    // where edges are too weak, and is polarity-agnostic (a dark card region is a hole
    // contour, which RETR_LIST also traces). Only runs when needed, so the common case
    // stays one pass per tick.
    if (!best) {
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
