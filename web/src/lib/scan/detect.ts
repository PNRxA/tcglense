// Single-card detection + perspective rectification for the visual scanner.
//
// Given a camera frame, find the card's four corners and warp them to an upright
// fixed-size crop, so the perceptual hash (and the set-line OCR) see a consistent,
// deskewed image regardless of how the card was held. Everything here is pure and
// unit-tested; the camera plumbing lives in `useCardScanner`, which falls back to the
// on-screen guide box when {@link detectCardQuad} returns null (busy background, heavy
// rotation, low contrast).
//
// The detector is deliberately lightweight (no OpenCV): it segments the card from the
// background by luma difference and takes the diagonal extrema of the foreground as the
// corners. That is exact for an axis-aligned card and good for the small rotations a
// hand-held portrait card actually has; a strongly rotated or low-contrast card fails
// {@link quadIsPlausibleCard} and the caller uses the guide box instead. A rotating-
// calipers / contour upgrade is tracked with the accuracy work.

import { CARD_ASPECT } from './regions'

export interface Point {
  x: number
  y: number
}

/** Four corners in a fixed order: top-left, top-right, bottom-right, bottom-left. */
export type Quad = [Point, Point, Point, Point]

/** Rec. 601 integer luma (matches the hasher's grayscale), for a packed RGBA buffer. */
export function toGray(rgba: Uint8ClampedArray | Uint8Array, width: number, height: number): Uint8Array {
  const n = width * height
  const gray = new Uint8Array(n)
  for (let i = 0; i < n; i++) {
    const r = rgba[i * 4]!
    const g = rgba[i * 4 + 1]!
    const b = rgba[i * 4 + 2]!
    gray[i] = (r * 77 + g * 150 + b * 29) >> 8
  }
  return gray
}

/** Order four arbitrary points as [TL, TR, BR, BL] using the sum/difference trick:
 * TL has the smallest x+y, BR the largest; TR has the largest x−y, BL the smallest. */
export function orderCorners(points: Point[]): Quad {
  let tl = points[0]!
  let br = points[0]!
  let tr = points[0]!
  let bl = points[0]!
  for (const p of points) {
    if (p.x + p.y < tl.x + tl.y) tl = p
    if (p.x + p.y > br.x + br.y) br = p
    if (p.x - p.y > tr.x - tr.y) tr = p
    if (p.x - p.y < bl.x - bl.y) bl = p
  }
  return [
    { x: tl.x, y: tl.y },
    { x: tr.x, y: tr.y },
    { x: br.x, y: br.y },
    { x: bl.x, y: bl.y },
  ]
}

/** Mean luma of the frame's outer border ring — the background estimate the card is
 * segmented against. Samples the outermost `band` fraction on each side. */
function backgroundLuma(gray: Uint8Array, width: number, height: number, band = 0.06): number {
  const bx = Math.max(1, Math.floor(width * band))
  const by = Math.max(1, Math.floor(height * band))
  let sum = 0
  let count = 0
  for (let y = 0; y < height; y++) {
    const inYBand = y < by || y >= height - by
    for (let x = 0; x < width; x++) {
      if (inYBand || x < bx || x >= width - bx) {
        sum += gray[y * width + x]!
        count++
      }
    }
  }
  return count === 0 ? 0 : sum / count
}

/** Minimum luma difference from the background for a pixel to count as card foreground. */
const FOREGROUND_LUMA_DELTA = 36

/** Detect the card's four corners in a grayscale frame, or null when the segmentation is
 * unusable (too little / too much foreground, or the resulting quad isn't card-shaped).
 * The caller falls back to the guide box on null. */
export function detectCardQuad(gray: Uint8Array, width: number, height: number): Quad | null {
  if (width < 8 || height < 8) return null
  const bg = backgroundLuma(gray, width, height)

  let minSum = Infinity
  let maxSum = -Infinity
  let maxDiff = -Infinity
  let minDiff = Infinity
  let tl: Point | null = null
  let br: Point | null = null
  let tr: Point | null = null
  let bl: Point | null = null
  let count = 0

  for (let y = 0; y < height; y++) {
    const row = y * width
    for (let x = 0; x < width; x++) {
      if (Math.abs(gray[row + x]! - bg) <= FOREGROUND_LUMA_DELTA) continue
      count++
      const sum = x + y
      const diff = x - y
      if (sum < minSum) {
        minSum = sum
        tl = { x, y }
      }
      if (sum > maxSum) {
        maxSum = sum
        br = { x, y }
      }
      if (diff > maxDiff) {
        maxDiff = diff
        tr = { x, y }
      }
      if (diff < minDiff) {
        minDiff = diff
        bl = { x, y }
      }
    }
  }

  const total = width * height
  // Too little foreground = no card; nearly-all foreground = no distinct background to
  // segment against (the diagonal extrema would just be the frame corners).
  if (count < total * 0.08 || count > total * 0.97) return null
  if (!tl || !tr || !br || !bl) return null

  const quad = orderCorners([tl, tr, br, bl])
  return quadIsPlausibleCard(quad, width, height) ? quad : null
}

/** Euclidean distance between two points. */
function dist(a: Point, b: Point): number {
  return Math.hypot(a.x - b.x, a.y - b.y)
}

/** Polygon area via the shoelace formula (absolute value). */
export function quadArea(quad: Quad): number {
  let area = 0
  for (let i = 0; i < 4; i++) {
    const a = quad[i]!
    const b = quad[(i + 1) % 4]!
    area += a.x * b.y - b.x * a.y
  }
  return Math.abs(area) / 2
}

/** Whether a detected quad is plausibly a card: card-shaped aspect ratio, roughly
 * parallel opposite sides, a big-enough area, and corners inside the frame. Rejecting
 * here is what makes a bad segmentation fall back to the guide box rather than warp
 * garbage. */
export function quadIsPlausibleCard(quad: Quad, width: number, height: number): boolean {
  const [tl, tr, br, bl] = quad
  const top = dist(tl, tr)
  const bottom = dist(bl, br)
  const left = dist(tl, bl)
  const right = dist(tr, br)
  if (top < 8 || bottom < 8 || left < 8 || right < 8) return false

  // Opposite sides should be within ~35% of each other (a real card, not a wedge).
  if (Math.max(top, bottom) / Math.min(top, bottom) > 1.35) return false
  if (Math.max(left, right) / Math.min(left, right) > 1.35) return false

  // Aspect ratio close to a card's 61:85 (portrait), within a generous tolerance for
  // perspective foreshortening.
  const w = (top + bottom) / 2
  const h = (left + right) / 2
  const aspect = w / h
  if (Math.abs(aspect - CARD_ASPECT) > 0.22) return false

  // The card should fill a meaningful share of the frame.
  if (quadArea(quad) < width * height * 0.1) return false

  // Corners inside the frame (small slack for edge-touching cards).
  const slack = 2
  return quad.every(
    (p) => p.x >= -slack && p.y >= -slack && p.x <= width + slack && p.y <= height + slack,
  )
}

/** Solve `A x = b` for an n×n system by Gaussian elimination with partial pivoting.
 * Returns null if the matrix is singular (no unique solution). */
function solveLinear(a: number[][], b: number[]): number[] | null {
  const n = b.length
  // Work on copies so callers' arrays aren't mutated.
  const m = a.map((row) => row.slice())
  const y = b.slice()
  for (let col = 0; col < n; col++) {
    // Partial pivot: largest magnitude in this column.
    let pivot = col
    for (let r = col + 1; r < n; r++) {
      if (Math.abs(m[r]![col]!) > Math.abs(m[pivot]![col]!)) pivot = r
    }
    if (Math.abs(m[pivot]![col]!) < 1e-12) return null
    ;[m[col], m[pivot]] = [m[pivot]!, m[col]!]
    ;[y[col], y[pivot]] = [y[pivot]!, y[col]!]
    // Eliminate below.
    for (let r = col + 1; r < n; r++) {
      const factor = m[r]![col]! / m[col]![col]!
      for (let c = col; c < n; c++) m[r]![c]! -= factor * m[col]![c]!
      y[r]! -= factor * y[col]!
    }
  }
  // Back-substitution.
  const x: number[] = Array.from({ length: n }, () => 0)
  for (let row = n - 1; row >= 0; row--) {
    let acc = y[row]!
    for (let c = row + 1; c < n; c++) acc -= m[row]![c]! * x[c]!
    x[row] = acc / m[row]![row]!
  }
  return x
}

/** The 3×3 projective homography (row-major, 9 numbers, `h[8] = 1`) mapping the four
 * `dst` corners to the four `src` corners, or null if degenerate. Corners must be in the
 * same order in both. Built to inverse-sample: pass the destination rectangle as `dst`
 * and the detected quad as `src`. */
export function solveHomography(dst: Quad, src: Quad): number[] | null {
  const a: number[][] = []
  const b: number[] = []
  for (let i = 0; i < 4; i++) {
    const { x: u, y: v } = dst[i]!
    const { x, y } = src[i]!
    // x = (h0 u + h1 v + h2) / (h6 u + h7 v + 1)
    a.push([u, v, 1, 0, 0, 0, -x * u, -x * v])
    b.push(x)
    // y = (h3 u + h4 v + h5) / (h6 u + h7 v + 1)
    a.push([0, 0, 0, u, v, 1, -y * u, -y * v])
    b.push(y)
  }
  const h = solveLinear(a, b)
  if (!h) return null
  return [...h, 1]
}

/** Warp the source quad to an upright `dstW`×`dstH` RGBA image by inverse mapping +
 * bilinear sampling. Returns the destination pixels (length `dstW*dstH*4`); a sample
 * that falls outside the source is left transparent-black. */
export function warpToRect(
  srcRgba: Uint8ClampedArray | Uint8Array,
  srcW: number,
  srcH: number,
  srcQuad: Quad,
  dstW: number,
  dstH: number,
): Uint8ClampedArray {
  const dst = new Uint8ClampedArray(dstW * dstH * 4)
  const rect: Quad = [
    { x: 0, y: 0 },
    { x: dstW, y: 0 },
    { x: dstW, y: dstH },
    { x: 0, y: dstH },
  ]
  const h = solveHomography(rect, srcQuad)
  if (!h) return dst
  const [h0, h1, h2, h3, h4, h5, h6, h7, h8] = h as [
    number, number, number, number, number, number, number, number, number,
  ]

  for (let dy = 0; dy < dstH; dy++) {
    for (let dx = 0; dx < dstW; dx++) {
      const denom = h6 * dx + h7 * dy + h8
      const sx = (h0 * dx + h1 * dy + h2) / denom
      const sy = (h3 * dx + h4 * dy + h5) / denom
      const di = (dy * dstW + dx) * 4
      if (sx < 0 || sy < 0 || sx > srcW - 1 || sy > srcH - 1) {
        dst[di + 3] = 255 // opaque black outside the source, so the crop has no holes
        continue
      }
      // Bilinear sample.
      const x0 = Math.floor(sx)
      const y0 = Math.floor(sy)
      const x1 = Math.min(x0 + 1, srcW - 1)
      const y1 = Math.min(y0 + 1, srcH - 1)
      const fx = sx - x0
      const fy = sy - y0
      const i00 = (y0 * srcW + x0) * 4
      const i10 = (y0 * srcW + x1) * 4
      const i01 = (y1 * srcW + x0) * 4
      const i11 = (y1 * srcW + x1) * 4
      for (let c = 0; c < 3; c++) {
        const top = srcRgba[i00 + c]! * (1 - fx) + srcRgba[i10 + c]! * fx
        const bottom = srcRgba[i01 + c]! * (1 - fx) + srcRgba[i11 + c]! * fx
        dst[di + c] = top * (1 - fy) + bottom * fy
      }
      dst[di + 3] = 255
    }
  }
  return dst
}
