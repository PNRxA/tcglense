// OpenCV.js card detection: find the card's four corners in a camera frame robustly,
// for the live outline and a tight, deskewed capture crop. OpenCV.js is a ~13 MB WASM
// payload, so it's lazily imported the first time the scanner starts (never at app
// load), like tesseract.
//
// The detector is an escalation ladder of edge passes over one shared grayscale+blur,
// cheapest and most conservative first, returning at the first pass whose gated
// candidates yield a selectable quad:
//
// 1. Median-luma Canny (the classic auto-Canny ±33% band) — the historical pass; still
//    first because it is the best-behaved on ordinary scenes.
// 2. Gradient-percentile Canny — thresholds from the Sobel-magnitude distribution
//    instead of luma. A busy or partly bright scene drags the luma median far above a
//    weak card edge (the edge never enters the map at all); percentile thresholds
//    follow the scene's actual edge strengths. (Empirically: a dark card on dark wood
//    under a bright window goes from 22% of its boundary in the edge map to 100%.)
// 3. Low fixed Canny — the floor for defocused scenes where even gradient percentiles
//    sit above a soft card edge because strong background texture dominates the
//    distribution. The dense map this produces is safe because the contour gates do
//    the rejecting; the card typically surfaces as the hole its border moat leaves in
//    the closed texture blob.
// 4. Otsu threshold segmentation — polarity-agnostic catch-all for soft-gradient
//    outlines below any usable edge threshold (bimodal scenes only).
//
// Each edge band runs its CLOSED map first (small glare/focus breaks otherwise split
// the outline into fragments that individually fail the size/aspect gates), then the
// RAW map (the 5×5 close can also glue the card outline to background clutter — raw
// preserves the original Canny topology when that gluing is what defeated the closed
// pass). Candidate gating lives in opencvContours.ts, mode-aware selection (guide /
// tracking / capture priors, ambiguity rejection) in quadSelect.ts, and search-window
// geometry in guidedDetect.ts.
//
// Corners are returned NORMALISED (0..1 of the full frame) so the same quad drives
// both the on-screen outline (any display size) and the capture warp (any capture
// resolution). The lightweight `detect.ts` detector stays as the fallback when OpenCV
// isn't loaded.

import type { Quad } from './detect'
import { fullFrameWindow, type SearchWindow } from './guidedDetect'
import { collectCardQuads, quadEdgeSupport, quadInteriorEdgeDensity } from './opencvContours'
import type { Cv, CvMat } from './opencvTypes'
import { selectScoredCardQuad, type QuadSelection } from './quadSelect'

export type { Cv } from './opencvTypes'

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

interface CannyBand {
  lower: number
  upper: number
}

/** The classic auto-Canny band: thresholds scaled to the blurred frame's median luma so
 * dim or low-contrast scenes still produce edges. */
function medianBand(blur: CvMat): CannyBand {
  const median = medianLuma(blur.data)
  const lower = Math.max(10, Math.round(median * 0.67))
  const upper = Math.max(lower + 20, Math.min(255, Math.round(median * 1.33)))
  return { lower, upper }
}

/** Percentiles of the Sobel gradient-magnitude (|gx|+|gy|) distribution the band's
 * Canny thresholds come from. */
const GRADIENT_LOW_PERCENTILE = 0.8
const GRADIENT_HIGH_PERCENTILE = 0.95

/** Canny thresholds from the frame's gradient-magnitude distribution: the lower/upper
 * thresholds land at fixed percentiles of |gx|+|gy|, so a weak-but-real card edge in a
 * scene whose *luma* median is dragged up by unrelated bright regions still clears the
 * threshold. */
function gradientBand(cv: Cv, blur: CvMat): CannyBand {
  const gx = new cv.Mat()
  const gy = new cv.Mat()
  try {
    cv.Sobel(blur, gx, cv.CV_16S, 1, 0)
    cv.Sobel(blur, gy, cv.CV_16S, 0, 1)
    const dx = gx.data16S
    const dy = gy.data16S
    const n = dx.length
    // |gx|+|gy| of an 8-bit image under 3×3 Sobel is bounded by 8×255; a 1021-bin
    // histogram (cap 1020) is plenty for percentile picking.
    const hist = new Uint32Array(1021)
    for (let i = 0; i < n; i++) {
      const m = Math.min(1020, Math.abs(dx[i]!) + Math.abs(dy[i]!))
      hist[m] = (hist[m] ?? 0) + 1
    }
    const pick = (fraction: number): number => {
      const target = n * fraction
      let acc = 0
      for (let v = 0; v < 1021; v++) {
        acc += hist[v]!
        if (acc >= target) return v
      }
      return 1020
    }
    const lower = Math.max(10, pick(GRADIENT_LOW_PERCENTILE))
    const upper = Math.max(lower + 20, pick(GRADIENT_HIGH_PERCENTILE))
    return { lower, upper }
  } finally {
    gx.delete()
    gy.delete()
  }
}

/** The fixed low band that catches soft, defocused card edges the adaptive bands sit
 * above. Kept last among the edge bands: its dense map costs the most to gate. */
const LOW_BAND: CannyBand = { lower: 10, upper: 30 }

/** Two bands this close would re-run an equivalent edge map — skip the duplicate. */
function similarBand(a: CannyBand, b: CannyBand): boolean {
  return Math.abs(a.lower - b.lower) <= 3 && Math.abs(a.upper - b.upper) <= 5
}

/** A pass winner stops the escalation only when BOTH evidence checks agree: its sides
 * trace real edges (support) and there is card-like empty space just inside them
 * (interior). Each check covers the other's blind spot — support alone inverts on
 * crisp grid textures (a phantom aligned to texture lines out-scores a true quad
 * whose sides bow a pixel or two), while interior alone happily confirms a slightly
 * misplaced quad on a defocused scene whose evidence is too soft to object. A winner
 * below the bar stays provisional: a later band frequently produces a tighter quad. */
const CONFIDENT_SUPPORT = 0.85
const CONFIDENT_MAX_INTERIOR = 0.25

/** Support-to-weight floor/slope for arbitration: never disqualifying — legitimately
 * hull-recovered geometry (a glare-washed edge) can have an evidence-free side, and
 * if it is the only winner it must still win. */
function supportWeight(support: number): number {
  return 0.35 + 0.65 * support
}

/** Interior-density penalty for arbitration, deliberately steeper than the support
 * slope: a leaked quad containing the true boundary must lose to the true quad even
 * when texture alignment hands it the better perimeter support. A weight, never a
 * gate — a borderless card's mild art texture only slightly discounts it, and if the
 * only winner has a busy interior ring it still wins. */
function interiorWeight(density: number): number {
  return Math.exp(-2.5 * density)
}

export interface DetectOptions {
  /** Whether the fallback passes (the gradient-percentile and low Canny bands, and the
   * Otsu segmentation retry) may run when the primary median-band pass finds nothing.
   * They are the expensive share of a cardless tick, so the live loop only enables
   * them on a cadence of misses (the outline tracker bridges the added latency) and on
   * region-of-interest crops; a one-off capture-path detect should leave them on
   * (default). */
  fallbackPasses?: boolean
  /** The crop's location within the full detection frame when `imageData` is a
   * region-of-interest crop rather than the whole frame. Gates and output coordinates
   * are always in full-frame terms. */
  window?: SearchWindow
  /** Candidate-selection policy (guide-weighted acquisition, prior-associated tracking
   * or capture). Defaults to the classic area-dominant, aspect-weighted selection. */
  select?: QuadSelection
}

/** Detect the most card-shaped quadrilateral in `imageData`, or null. Corners are
 * returned normalised (0..1 of the full frame) and ordered [TL, TR, BR, BL]. All
 * OpenCV mats are freed. */
export function detectCardQuadCv(
  cv: Cv,
  imageData: ImageData,
  options: DetectOptions = {},
): Quad | null {
  const fallbackPasses = options.fallbackPasses ?? true
  const window = options.window ?? fullFrameWindow(imageData.width, imageData.height)
  const select = options.select ?? { mode: 'plain' }
  const src = cv.matFromImageData(imageData)
  const gray = new cv.Mat()
  const blur = new cv.Mat()
  const edges = new cv.Mat()
  const closed = new cv.Mat()
  const kernel = cv.getStructuringElement(cv.MORPH_RECT, new cv.Size(5, 5))
  // Shared edge-evidence map for support scoring (the permissive low band, so soft
  // boundaries still count as evidence). Built lazily: only frames where some pass
  // actually finds a winner pay for it. (Boxed so the closure assignment below isn't
  // flow-narrowed away at the cleanup site.)
  const lazy: { supportEdges: CvMat | null } = { supportEdges: null }
  try {
    cv.cvtColor(src, gray, cv.COLOR_RGBA2GRAY)
    cv.GaussianBlur(gray, blur, new cv.Size(5, 5), 0)

    const provisional: Array<{ quad: Quad; rank: number }> = []
    // Run one mask's collect+select; returns the winner only when the edge evidence
    // makes it confident, else banks it for the end-of-ladder arbitration.
    const consider = (mask: CvMat): Quad | null => {
      const pick = selectScoredCardQuad(collectCardQuads(cv, mask, window), select)
      if (!pick) return null
      if (!lazy.supportEdges) {
        lazy.supportEdges = new cv.Mat()
        cv.Canny(blur, lazy.supportEdges, LOW_BAND.lower, LOW_BAND.upper)
      }
      const support = quadEdgeSupport(lazy.supportEdges, window, pick.quad)
      const interior = quadInteriorEdgeDensity(lazy.supportEdges, window, pick.quad)
      provisional.push({
        quad: pick.quad,
        rank: pick.score * supportWeight(support) * interiorWeight(interior),
      })
      const confident = support >= CONFIDENT_SUPPORT && interior <= CONFIDENT_MAX_INTERIOR
      return confident ? pick.quad : null
    }
    const best = (): Quad | null => {
      let top: { quad: Quad; rank: number } | null = null
      for (const entry of provisional) {
        if (!top || entry.rank > top.rank) top = entry
      }
      return top?.quad ?? null
    }

    const bands: CannyBand[] = [medianBand(blur)]
    if (fallbackPasses) {
      // Low before gradient: the low map is a superset of edge evidence, so when a
      // soft card boundary exists at all, the low pass finds it hugging the true
      // outline (usually as the hole its border moat leaves in the closed texture
      // blob). The gradient band still catches scenes whose dense low map fuses into
      // a frame-touching blob with no usable hole.
      if (!bands.some((band) => similarBand(band, LOW_BAND))) bands.push(LOW_BAND)
      const gradient = gradientBand(cv, blur)
      if (!bands.some((band) => similarBand(band, gradient))) bands.push(gradient)
    }

    for (const band of bands) {
      cv.Canny(blur, edges, band.lower, band.upper)
      cv.morphologyEx(edges, closed, cv.MORPH_CLOSE, kernel)
      const confident = consider(closed) ?? consider(edges)
      if (confident) return confident
    }

    if (fallbackPasses) {
      // Otsu segmentation — catches soft-gradient outlines where edges are too weak,
      // and is polarity-agnostic (a dark card region is a hole contour, which
      // RETR_LIST also traces).
      cv.threshold(blur, edges, 0, 255, cv.THRESH_BINARY + cv.THRESH_OTSU)
      const confident = consider(edges)
      if (confident) return confident
    }
    return best()
  } finally {
    src.delete()
    gray.delete()
    blur.delete()
    edges.delete()
    closed.delete()
    kernel.delete()
    lazy.supportEdges?.delete()
  }
}
