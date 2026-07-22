// Foil-star detection: does the card's bottom-left info line carry the printed foil star
// (`★`)? Modern Magic foils ink a star onto that line — `SLD ★ EN`, `TRK ★ EN` — where the
// nonfoil prints a bullet (`SLD • EN`) or nothing; it is the one *printed* foil signal on a
// card, so reading it lets the scanner seed a scanned copy as foil.
//
// Why OpenCV and not OCR: the OCR half of the scanner (tesseract, eng LSTM) has no `★` in its
// character set, so it silently drops the glyph — whitelisting it is a no-op. The star is
// therefore found geometrically instead, as a *shape*, on the same warped card crop the
// fingerprint and OCR already use.
//
// The discriminator (tuned + validated against real Scryfall scans of Secret Lair and
// Universes-Beyond foils, with nonfoil and non-star cards as controls): a filled 5-pointed
// star is the only glyph on that line whose boundary has **five evenly-spaced radius maxima**
// measured from its centroid. Letters that happen to spike five times are lopsided; bullets and
// set symbols are round or solid. So a candidate contour must be near-square, fill ~half its
// convex hull, and have exactly five radial peaks spaced ~72° apart over deep valleys. Convexity
// defects were tried first and rejected — the printed star is *fat* (stubby points), so its
// concavities are too shallow to count reliably at this pixel size, whereas the radial-peak test
// is robust to that and to rotation.
//
// This resolves *finish*, never identity or printing — the visual match and OCR still do those.
// It flags only cards that physically print the star (modern foils); older foils that print no
// star, and any star the crop is too noisy to resolve, simply fall back to a regular copy.

/** The minimal OpenCV.js surface this detector needs. The runtime is fully-featured; each
 * scanner module types just the calls it makes (as `opencvDetect.ts` does). */
interface StarCv {
  matFromImageData: (data: ImageData) => StarMat
  Size: new (w: number, h: number) => unknown
  Rect: new (x: number, y: number, w: number, h: number) => unknown
  MatVector: new () => StarMatVector
  Mat: new () => StarMat
  cvtColor: (src: StarMat, dst: StarMat, code: number) => void
  resize: (
    src: StarMat,
    dst: StarMat,
    dsize: unknown,
    fx: number,
    fy: number,
    interp: number,
  ) => void
  threshold: (src: StarMat, dst: StarMat, thresh: number, maxval: number, type: number) => number
  findContours: (
    img: StarMat,
    contours: StarMatVector,
    hierarchy: StarMat,
    mode: number,
    method: number,
  ) => void
  convexHull: (src: StarMat, dst: StarMat, clockwise: boolean, returnPoints: boolean) => void
  contourArea: (contour: StarMat) => number
  boundingRect: (contour: StarMat) => { x: number; y: number; width: number; height: number }
  moments: (contour: StarMat) => { m00: number; m10: number; m01: number }
  COLOR_RGBA2GRAY: number
  INTER_CUBIC: number
  THRESH_BINARY: number
  THRESH_BINARY_INV: number
  THRESH_OTSU: number
  RETR_EXTERNAL: number
  CHAIN_APPROX_NONE: number
}
interface StarMat {
  rows: number
  cols: number
  data: Uint8Array
  data32S: Int32Array
  roi: (rect: unknown) => StarMat
  delete: () => void
}
interface StarMatVector {
  size: () => number
  get: (i: number) => StarMat
  delete: () => void
}

/** Where the foil star lives, as fractions of the full (upright, deskewed) card crop: the
 * bottom-left info line (set code · finish · language), narrowed to the left half so the set
 * symbol / artist glyph on the right can't contribute a false shape. A sub-region of the wider
 * `SET_REGION` the OCR reads. */
export const STAR_REGION = { left: 0.03, top: 0.94, width: 0.46, height: 0.035 }

/** Height (px) the star strip is scaled to before analysis, so the pixel thresholds below are
 * independent of the input crop's resolution. */
const CANON_HEIGHT = 144

/** Angular bins for the radial profile (5° each) — enough to resolve a star's 5 points. */
const RADIAL_BINS = 72

/** Merge peaks closer than this many bins (30°): one star point can't span two peaks. */
const PEAK_MERGE_BINS = 6

/** Fraction of the (max−min) radius a bin must exceed to count as a peak. */
const PEAK_PROMINENCE = 0.35

/** A real star's 5 gaps are ~equal; the widest may be at most this × the narrowest. Letters
 * that spike 5 times are lopsided and fail here (measured ≥2.3 vs a star's ~1.1). */
const MAX_GAP_RATIO = 1.9

/** The star's inner valleys must sit at most this fraction of its outer points — a genuine
 * inward dip, not a bumpy near-circle. */
const MAX_VALLEY_RATIO = 0.62

// Candidate-contour gates (measured on the canonical-height strip).
const MIN_CONTOUR_AREA = 300
const MIN_HEIGHT_FRACTION = 0.24
const MAX_HEIGHT_FRACTION = 0.95
const MIN_WIDTH = 8
const MIN_ASPECT = 0.75
const MAX_ASPECT = 1.3
const MIN_SOLIDITY = 0.42
const MAX_SOLIDITY = 0.75

export interface RadialSignature {
  /** Number of distinct, mergeable radius maxima around the contour. */
  peaks: number
  /** Widest peak-gap ÷ narrowest (1 = perfectly even); Infinity unless `peaks === 5`. */
  gapRatio: number
  /** min radius ÷ max radius over the profile (valley depth); 1 unless `peaks === 5`. */
  valleyRatio: number
}

const circularNear = (a: number, b: number) =>
  Math.min((a - b + RADIAL_BINS) % RADIAL_BINS, (b - a + RADIAL_BINS) % RADIAL_BINS)

/**
 * The radial signature of a closed contour: sweep its boundary points into {@link RADIAL_BINS}
 * angular bins around the centroid (`cx`,`cy`), keep the max radius per bin, and count prominent,
 * evenly-mergeable peaks. Pure (no OpenCV) so the star-vs-not maths is unit-tested directly on
 * point sets. `count` bounds how many of `xs`/`ys` are valid.
 */
export function radialSignature(
  xs: ArrayLike<number>,
  ys: ArrayLike<number>,
  count: number,
  cx: number,
  cy: number,
): RadialSignature {
  const rad = Array.from({ length: RADIAL_BINS }, () => 0)
  for (let i = 0; i < count; i++) {
    const dx = xs[i]! - cx
    const dy = ys[i]! - cy
    const bin =
      ((Math.round((Math.atan2(dy, dx) / (2 * Math.PI)) * RADIAL_BINS) % RADIAL_BINS) +
        RADIAL_BINS) %
      RADIAL_BINS
    const r = Math.hypot(dx, dy)
    if (r > rad[bin]!) rad[bin] = r
  }
  // Fill empty bins (sparse boundary) from their nearest filled neighbours, circularly.
  for (let i = 0; i < RADIAL_BINS; i++) {
    if (rad[i] !== 0) continue
    let a = i
    let b = i
    while (rad[(a + RADIAL_BINS) % RADIAL_BINS] === 0) a--
    while (rad[b % RADIAL_BINS] === 0) b++
    rad[i] = (rad[(a + RADIAL_BINS) % RADIAL_BINS]! + rad[b % RADIAL_BINS]!) / 2
  }
  // Circular 3-tap smooth to suppress single-bin jitter.
  const sm = rad.map(
    (_, i) =>
      (rad[(i - 1 + RADIAL_BINS) % RADIAL_BINS]! + rad[i]! + rad[(i + 1) % RADIAL_BINS]!) / 3,
  )
  const max = Math.max(...sm)
  const min = Math.min(...sm)
  const threshold = min + PEAK_PROMINENCE * (max - min)

  const raw: number[] = []
  for (let i = 0; i < RADIAL_BINS; i++) {
    const p = sm[i]!
    if (
      p >= sm[(i - 1 + RADIAL_BINS) % RADIAL_BINS]! &&
      p >= sm[(i + 1) % RADIAL_BINS]! &&
      p >= threshold
    ) {
      raw.push(i)
    }
  }
  // Merge peaks within PEAK_MERGE_BINS (including across the 0/2π wrap).
  const merged: number[] = []
  for (const p of raw) {
    if (!merged.length || circularNear(p, merged[merged.length - 1]!) > PEAK_MERGE_BINS) {
      merged.push(p)
    }
  }
  if (
    merged.length > 1 &&
    circularNear(merged[0]!, merged[merged.length - 1]!) <= PEAK_MERGE_BINS
  ) {
    merged.pop()
  }
  if (merged.length !== 5) return { peaks: merged.length, gapRatio: Infinity, valleyRatio: 1 }

  const gaps: number[] = []
  for (let i = 0; i < 5; i++) {
    gaps.push((merged[(i + 1) % 5]! - merged[i]! + RADIAL_BINS) % RADIAL_BINS)
  }
  return {
    peaks: 5,
    gapRatio: Math.max(...gaps) / Math.min(...gaps),
    valleyRatio: max > 0 ? min / max : 1,
  }
}

/** Whether a radial signature is a five-pointed star: exactly 5 evenly-spaced peaks over deep
 * valleys. */
export function isStarSignature(sig: RadialSignature): boolean {
  return sig.peaks === 5 && sig.gapRatio <= MAX_GAP_RATIO && sig.valleyRatio <= MAX_VALLEY_RATIO
}

/** Whether one OpenCV contour (with pre-computed bounds + centroid, on a strip `stripHeight`
 * tall) is a foil star: near-square, half-filled, and radially five-pointed. */
function contourIsStar(
  cv: StarCv,
  contour: StarMat,
  bounds: { width: number; height: number },
  stripHeight: number,
  cx: number,
  cy: number,
): boolean {
  if (bounds.height < stripHeight * MIN_HEIGHT_FRACTION) return false
  if (bounds.height > stripHeight * MAX_HEIGHT_FRACTION) return false
  if (bounds.width < MIN_WIDTH) return false
  const area = cv.contourArea(contour)
  if (area < MIN_CONTOUR_AREA) return false
  const aspect = bounds.width / bounds.height
  if (aspect < MIN_ASPECT || aspect > MAX_ASPECT) return false

  const hull = new cv.Mat()
  try {
    cv.convexHull(contour, hull, false, true)
    const hullArea = cv.contourArea(hull)
    const solidity = hullArea > 0 ? area / hullArea : 1
    if (solidity < MIN_SOLIDITY || solidity > MAX_SOLIDITY) return false
  } finally {
    hull.delete()
  }

  const data = contour.data32S
  const count = contour.rows
  const xs = Array.from({ length: count }, (_, i) => data[i * 2]!)
  const ys = Array.from({ length: count }, (_, i) => data[i * 2 + 1]!)
  return isStarSignature(radialSignature(xs, ys, count, cx, cy))
}

/** Scan one Otsu-thresholded strip for a star contour. `invert` picks the polarity (light text
 * on a dark border vs dark text on a light one). All mats are freed. */
function stripHasStar(cv: StarCv, strip: StarMat, invert: boolean): boolean {
  const bin = new cv.Mat()
  const contours = new cv.MatVector()
  const hierarchy = new cv.Mat()
  try {
    const type = cv.THRESH_OTSU + (invert ? cv.THRESH_BINARY_INV : cv.THRESH_BINARY)
    cv.threshold(strip, bin, 0, 255, type)
    cv.findContours(bin, contours, hierarchy, cv.RETR_EXTERNAL, cv.CHAIN_APPROX_NONE)
    for (let i = 0; i < contours.size(); i++) {
      const contour = contours.get(i)
      try {
        const m = cv.moments(contour)
        if (m.m00 === 0) continue
        const bounds = cv.boundingRect(contour)
        if (contourIsStar(cv, contour, bounds, strip.rows, m.m10 / m.m00, m.m01 / m.m00)) {
          return true
        }
      } finally {
        contour.delete()
      }
    }
    return false
  } finally {
    bin.delete()
    contours.delete()
    hierarchy.delete()
  }
}

/**
 * Whether the warped card crop carries a printed foil star. `crop` is the upright, deskewed
 * card (any resolution — the star strip is canonical-scaled internally). Runs both Otsu
 * polarities so a star on a light border (dark-on-light) is found as well as the common
 * light-on-dark case. All OpenCV mats are freed; returns false on any degenerate input.
 */
export function detectFoilStar(cv: StarCv, crop: ImageData): boolean {
  const src = cv.matFromImageData(crop)
  const gray = new cv.Mat()
  const strip = new cv.Mat()
  try {
    cv.cvtColor(src, gray, cv.COLOR_RGBA2GRAY)
    const rx = Math.round(STAR_REGION.left * gray.cols)
    const ry = Math.round(STAR_REGION.top * gray.rows)
    const rw = Math.round(STAR_REGION.width * gray.cols)
    const rh = Math.round(STAR_REGION.height * gray.rows)
    if (rw < 1 || rh < 1) return false
    const roi = gray.roi(new cv.Rect(rx, ry, rw, rh))
    try {
      const canonWidth = Math.max(1, Math.round((rw * CANON_HEIGHT) / rh))
      cv.resize(roi, strip, new cv.Size(canonWidth, CANON_HEIGHT), 0, 0, cv.INTER_CUBIC)
    } finally {
      roi.delete()
    }
    return stripHasStar(cv, strip, false) || stripHasStar(cv, strip, true)
  } finally {
    src.delete()
    gray.delete()
    strip.delete()
  }
}
