// OpenCV.js card detection: find the card's four corners in a camera frame robustly,
// for the live outline and a tight, deskewed capture crop. OpenCV.js is a ~13 MB WASM
// payload, so it's lazily imported when the dedicated scanner page mounts (never at app
// load). That overlaps its download with the user's reading/camera-permission time.
//
// In the scanner's guided modes (acquisition / tracking / capture) the detector is an
// escalation ladder of edge passes over one shared grayscale+blur, cheapest and most
// conservative first, exiting early only when a winner's edge evidence is convincing:
//
// 1. Median-luma Canny (the classic auto-Canny ±33% band) — the historical pass; still
//    first because it is the best-behaved on ordinary scenes.
// 2. Low fixed Canny — a superset edge map for busy or defocused scenes where the
//    luma-keyed thresholds sit above a soft card edge (a bright region drags the
//    median far past it: empirically, a dark card on dark wood under a bright window
//    has only 22% of its boundary in the median map but 100% in the low map). Its
//    density is safe because the contour gates do the rejecting; the card typically
//    surfaces as the hole its border moat leaves in the closed texture blob.
// 3. Otsu threshold segmentation — polarity-agnostic catch-all for soft-gradient
//    outlines below any usable edge threshold (bimodal scenes only).
//
// Each edge band runs its CLOSED map first (small glare/focus breaks otherwise split
// the outline into fragments that individually fail the size/aspect gates), then the
// RAW map (the 5×5 close can also glue the card outline to background clutter — raw
// preserves the original Canny topology when that gluing is what defeated the closed
// pass). In the default 'plain' mode the ladder is OFF: plain is the historical
// pipeline (median-band closed contours, then Otsu), kept behaviour-compatible.
// Candidate gating lives in opencvContours.ts, mode-aware selection (guide /
// tracking / capture priors, ambiguity rejection) in quadSelect.ts, and search-window
// geometry in guidedDetect.ts.
//
// Corners are returned NORMALISED (0..1 of the full frame) so the same quad drives
// both the on-screen outline (any display size) and the capture warp (any capture
// resolution). The lightweight `detect.ts` detector stays as the explicit fallback when
// OpenCV fails to load; ordinary warm-up waits for this full detector.

import type { Quad } from './detect'
import { fullFrameWindow, type SearchWindow } from './guidedDetect'
import { collectCardQuads, quadEdgeSupport, quadInteriorEdgeDensity } from './opencvContours'
import type { Cv, CvMat } from './opencvTypes'
import {
  crossPassAmbiguous,
  selectCardQuadResult,
  type QuadSelection,
  type SelectedQuad,
} from './quadSelect'

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

/** The fixed low band that catches soft, defocused, or luma-mismatched card edges the
 * adaptive median band sits above. */
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
  /** Whether the fallback passes (in guided modes: the low Canny band and the Otsu
   * segmentation retry; in plain mode: the Otsu retry) may run when the primary
   * median-band pass finds nothing. They are the expensive share of a cardless tick,
   * so the live loop only enables them on a cadence of misses (the outline tracker
   * bridges the added latency) and on region-of-interest crops; a one-off
   * capture-path detect should leave them on (default). */
  fallbackPasses?: boolean
  /** The crop's location within the full detection frame when `imageData` is a
   * region-of-interest crop rather than the whole frame. Gates and output coordinates
   * are always in full-frame terms. */
  window?: SearchWindow
  /** Candidate-selection policy. Guided modes (acquisition/tracking/capture) run the
   * full escalation ladder with evidence arbitration; the default 'plain' mode IS the
   * historical detector — median-band closed contours, then the Otsu retry — kept
   * behaviour-compatible so a plain caller can never see a card-identity change
   * against the classic pipeline. */
  select?: QuadSelection
}

/** Detect the most card-shaped quadrilateral in `imageData`, or null. Corners are
 * returned normalised (0..1 of the full frame) and ordered [TL, TR, BR, BL]. All
 * OpenCV mats are freed, on success and on error alike. */
export function detectCardQuadCv(
  cv: Cv,
  imageData: ImageData,
  options: DetectOptions = {},
): Quad | null {
  const fallbackPasses = options.fallbackPasses ?? true
  const window = options.window ?? fullFrameWindow(imageData.width, imageData.height)
  const select = options.select ?? { mode: 'plain' }
  // Every native allocation happens inside the try and is declared here so the
  // finally can free whatever subset was actually created — an allocation failure
  // halfway through must not leak the earlier mats.
  let src: CvMat | null = null
  let gray: CvMat | null = null
  let blur: CvMat | null = null
  let edges: CvMat | null = null
  let closed: CvMat | null = null
  let kernel: CvMat | null = null
  // Shared edge-evidence map for support/interior scoring (the permissive low band,
  // so soft boundaries still count as evidence). Built lazily: only frames where some
  // pass actually finds a winner pay for it. (Boxed so the closure assignment below
  // isn't flow-narrowed away at the cleanup site.)
  const lazy: { supportEdges: CvMat | null } = { supportEdges: null }
  try {
    src = cv.matFromImageData(imageData)
    gray = new cv.Mat()
    blur = new cv.Mat()
    edges = new cv.Mat()
    closed = new cv.Mat()
    kernel = cv.getStructuringElement(cv.MORPH_RECT, new cv.Size(5, 5))
    cv.cvtColor(src, gray, cv.COLOR_RGBA2GRAY)
    cv.GaussianBlur(gray, blur, new cv.Size(5, 5), 0)
    // Non-null locals for the closures below (the outer lets stay nullable for the
    // finally).
    const blurMat = blur
    const edgesMat = edges
    const closedMat = closed
    const kernelMat = kernel

    const primary = medianBand(blurMat)

    // ——— Plain mode: the historical pipeline, verbatim. Median-band closed contours,
    // then the Otsu retry — no raw maps, no extra bands, no evidence arbitration —
    // so a plain caller's selected card can never differ from the classic detector's.
    if (select.mode === 'plain') {
      cv.Canny(blurMat, edgesMat, primary.lower, primary.upper)
      cv.morphologyEx(edgesMat, closedMat, cv.MORPH_CLOSE, kernelMat)
      const closedPick = selectCardQuadResult(collectCardQuads(cv, closedMat, window), select)
      if (closedPick.selected) return closedPick.selected.quad
      if (!fallbackPasses) return null
      cv.threshold(blurMat, edgesMat, 0, 255, cv.THRESH_BINARY + cv.THRESH_OTSU)
      const otsuPick = selectCardQuadResult(collectCardQuads(cv, edgesMat, window), select)
      return otsuPick.selected?.quad ?? null
    }

    // ——— Guided modes: the escalation ladder with evidence arbitration.
    const provisional: Array<{ pick: SelectedQuad; rank: number }> = []
    let sawAmbiguity = false
    // Run one mask's collect+select; returns the winner only when the edge evidence
    // makes it confident, else banks it for the end-of-ladder arbitration. An
    // ambiguous pass poisons the whole frame — a banked winner from another pass must
    // not shadow ambiguity a more inclusive map discovered.
    const consider = (mask: CvMat): Quad | null => {
      const result = selectCardQuadResult(collectCardQuads(cv, mask, window), select)
      if (result.ambiguous) {
        sawAmbiguity = true
        return null
      }
      const pick = result.selected
      if (!pick) return null
      if (!lazy.supportEdges) {
        lazy.supportEdges = new cv.Mat()
        cv.Canny(blurMat, lazy.supportEdges, LOW_BAND.lower, LOW_BAND.upper)
      }
      const support = quadEdgeSupport(lazy.supportEdges, window, pick.quad)
      const interior = quadInteriorEdgeDensity(lazy.supportEdges, window, pick.quad)
      provisional.push({
        pick,
        rank: pick.score * supportWeight(support) * interiorWeight(interior),
      })
      const confident = support >= CONFIDENT_SUPPORT && interior <= CONFIDENT_MAX_INTERIOR
      return confident ? pick.quad : null
    }

    const bands: CannyBand[] = [primary]
    if (fallbackPasses && !similarBand(primary, LOW_BAND)) {
      // The low map is a superset of edge evidence: when a soft card boundary exists
      // at all, the low pass finds it hugging the true outline (usually as the hole
      // its border moat leaves in the closed texture blob).
      bands.push(LOW_BAND)
    }

    for (const band of bands) {
      cv.Canny(blurMat, edgesMat, band.lower, band.upper)
      cv.morphologyEx(edgesMat, closedMat, cv.MORPH_CLOSE, kernelMat)
      let confident = consider(closedMat)
      if (!confident && !sawAmbiguity) confident = consider(edgesMat)
      if (sawAmbiguity) return null
      if (confident) return confident
    }

    if (fallbackPasses) {
      // Otsu segmentation — catches soft-gradient outlines where edges are too weak,
      // and is polarity-agnostic (a dark card region is a hole contour, which
      // RETR_LIST also traces).
      cv.threshold(blurMat, edgesMat, 0, 255, cv.THRESH_BINARY + cv.THRESH_OTSU)
      const confident = consider(edgesMat)
      if (sawAmbiguity) return null
      if (confident) return confident
    }

    // End-of-ladder arbitration. Two distinct near-tied winners that only ever
    // surfaced in DIFFERENT maps are just as ambiguous as an in-map near-tie.
    if (
      crossPassAmbiguous(
        provisional.map((entry) => entry.pick),
        select,
      )
    ) {
      return null
    }
    let top: { pick: SelectedQuad; rank: number } | null = null
    for (const entry of provisional) {
      if (!top || entry.rank > top.rank) top = entry
    }
    return top?.pick.quad ?? null
  } finally {
    src?.delete()
    gray?.delete()
    blur?.delete()
    edges?.delete()
    closed?.delete()
    kernel?.delete()
    lazy.supportEdges?.delete()
  }
}
