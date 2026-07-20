// @vitest-environment node
//
// Integration spec for the OpenCV card detector — runs the REAL OpenCV.js WASM runtime
// (Node build, no DOM needed) against synthetic camera frames with known ground-truth
// corners, so the pipeline's accuracy claims (adaptive thresholds, hull recovery, the
// Otsu fallback, outer-edge preference) are actually exercised, not just type-checked.
// The runtime is a ~13 MB payload: loaded once for the whole file (~1-2 s).

import { createRequire } from 'node:module'
import { beforeAll, describe, expect, it } from 'vitest'
import type { Quad } from '../detect'
import { detectCardQuadCv, loadOpenCv, medianLuma } from '../opencvDetect'
import { CARD_ASPECT } from '../regions'

type Cv = Awaited<ReturnType<typeof loadOpenCv>>

let cv: Cv

beforeAll(async () => {
  // opencv.js's CJS export is a thenable that resolves to the ready runtime, which
  // vitest's ESM interop mis-assimilates on a dynamic import — so this spec require()s
  // it directly. `loadOpenCv`'s browser load path is exercised by the app build.
  const mod = createRequire(import.meta.url)('@techstark/opencv-js') as PromiseLike<unknown>
  cv = (await mod) as Cv
}, 60_000)

/** A synthetic grayscale-ish RGBA frame (all channels equal) with helpers to draw
 * rounded, rotated card rectangles and blobs — plus the ground-truth corners. */
class Frame {
  data: Uint8ClampedArray
  constructor(
    public width: number,
    public height: number,
    background: number,
  ) {
    this.data = new Uint8ClampedArray(width * height * 4)
    for (let i = 0; i < width * height; i++) this.setIndex(i, background)
  }

  private setIndex(i: number, v: number) {
    this.data[i * 4] = v
    this.data[i * 4 + 1] = v
    this.data[i * 4 + 2] = v
    this.data[i * 4 + 3] = 255
  }

  set(x: number, y: number, v: number) {
    if (x >= 0 && y >= 0 && x < this.width && y < this.height) this.setIndex(y * this.width + x, v)
  }

  /** Draw a filled, rounded-corner rectangle rotated by `deg` around its centre; returns
   * the (sharp) ground-truth corners ordered [TL, TR, BR, BL], normalised 0..1. */
  drawCard(
    cx: number,
    cy: number,
    w: number,
    h: number,
    deg: number,
    value: number,
    cornerRadius = Math.round(w / 20),
  ): Quad {
    const rad = (deg * Math.PI) / 180
    const cos = Math.cos(rad)
    const sin = Math.sin(rad)
    const hw = w / 2
    const hh = h / 2
    const r = cornerRadius
    for (let y = 0; y < this.height; y++) {
      for (let x = 0; x < this.width; x++) {
        // Into the card's local frame.
        const u = cos * (x - cx) + sin * (y - cy)
        const v = -sin * (x - cx) + cos * (y - cy)
        const au = Math.abs(u)
        const av = Math.abs(v)
        if (au > hw || av > hh) continue
        if (au > hw - r && av > hh - r) {
          const du = au - (hw - r)
          const dv = av - (hh - r)
          if (du * du + dv * dv > r * r) continue
        }
        this.set(x, y, value)
      }
    }
    const corner = (su: number, sv: number) => ({
      x: (cx + cos * su * hw - sin * sv * hh) / this.width,
      y: (cy + sin * su * hw + cos * sv * hh) / this.height,
    })
    return [corner(-1, -1), corner(1, -1), corner(1, 1), corner(-1, 1)]
  }

  drawDisk(cx: number, cy: number, radius: number, value: number) {
    for (let y = Math.floor(cy - radius); y <= cy + radius; y++) {
      for (let x = Math.floor(cx - radius); x <= cx + radius; x++) {
        if ((x - cx) ** 2 + (y - cy) ** 2 <= radius * radius) this.set(x, y, value)
      }
    }
  }

  /** Fill an axis-aligned rectangle. */
  fillRect(x0: number, y0: number, x1: number, y1: number, value: number) {
    for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) this.set(x, y, value)
  }

  /** Draw a filled axis-aligned octagon: a `w`×`h` rectangle centred at (cx, cy) with
   * a straight 45° cut of leg length `cut` at each corner. */
  drawOctagon(cx: number, cy: number, w: number, h: number, cut: number, value: number) {
    const hw = w / 2
    const hh = h / 2
    for (let y = 0; y < this.height; y++) {
      for (let x = 0; x < this.width; x++) {
        const au = Math.abs(x - cx)
        const av = Math.abs(y - cy)
        if (au > hw || av > hh) continue
        if (au - (hw - cut) + (av - (hh - cut)) > cut) continue
        this.set(x, y, value)
      }
    }
  }

  /** Wash out edge response inside a patch the way glare/defocus does: blend each
   * pixel toward a heavily box-blurred copy, with a weight that feathers to zero at
   * the patch border — so the wash kills the local gradient without introducing any
   * sharp boundary of its own. */
  washPatch(x0: number, y0: number, x1: number, y1: number, feather: number) {
    const snapshot = new Uint8ClampedArray(this.data)
    const at = (x: number, y: number) => {
      const cx = Math.min(this.width - 1, Math.max(0, x))
      const cy = Math.min(this.height - 1, Math.max(0, y))
      return snapshot[(cy * this.width + cx) * 4]!
    }
    const R = 14
    const smooth = (t: number) => {
      const c = Math.min(1, Math.max(0, t))
      return c * c * (3 - 2 * c)
    }
    for (let y = y0; y < y1; y++) {
      for (let x = x0; x < x1; x++) {
        let sum = 0
        let n = 0
        for (let dy = -R; dy <= R; dy++) {
          for (let dx = -R; dx <= R; dx++) {
            sum += at(x + dx, y + dy)
            n++
          }
        }
        const target = sum / n
        const w =
          smooth(Math.min(x - x0, x1 - 1 - x) / feather) *
          smooth(Math.min(y - y0, y1 - 1 - y) / feather)
        this.set(x, y, Math.round(at(x, y) * (1 - w) + target * w))
      }
    }
  }

  /** Deterministic per-pixel noise (LCG), ± amplitude. */
  addNoise(amplitude: number, seed = 1) {
    let s = seed
    for (let i = 0; i < this.width * this.height; i++) {
      s = (s * 1103515245 + 12345) & 0x7fffffff
      const n = ((s >> 16) % (2 * amplitude + 1)) - amplitude
      this.setIndex(i, this.data[i * 4]! + n)
    }
  }

  imageData(): ImageData {
    return { data: this.data, width: this.width, height: this.height } as unknown as ImageData
  }
}

/** Worst corner distance from the truth (normalised units) — Infinity when nothing was
 * detected, so `expect(maxCornerError(...)).toBeLessThanOrEqual(tol)` covers both. */
function maxCornerError(detected: Quad | null, truth: Quad): number {
  if (!detected) return Infinity
  let max = 0
  for (let i = 0; i < 4; i++) {
    max = Math.max(max, Math.hypot(detected[i]!.x - truth[i]!.x, detected[i]!.y - truth[i]!.y))
  }
  return max
}

/** A centred card `heightPx` tall at the standard aspect. */
function cardDims(heightPx: number): { w: number; h: number } {
  return { w: Math.round(heightPx * CARD_ASPECT), h: heightPx }
}

/** The dim-scene base several cases share: a dark background with a bright glare band
 * across the top. Edge response in the dark region sits far below fixed thresholds,
 * and the band drags the Otsu split point above a dim card — so a card here is only
 * resolvable by the adaptive edge pass. */
function dimSceneFrame(): Frame {
  const frame = new Frame(320, 240, 40)
  frame.fillRect(0, 0, 320, 60, 200)
  return frame
}

describe('medianLuma', () => {
  it('finds the histogram median', () => {
    expect(medianLuma(new Uint8Array([10, 10, 10, 200, 220]))).toBe(10)
    expect(medianLuma(new Uint8Array([0, 50, 100, 150, 200]))).toBe(100)
    expect(medianLuma(new Uint8Array(0))).toBe(127)
  })
})

describe('detectCardQuadCv', () => {
  it('snaps to a straight card on a contrasting background', () => {
    const frame = new Frame(320, 240, 50)
    const { w, h } = cardDims(190)
    const truth = frame.drawCard(160, 120, w, h, 0, 205)
    expect(maxCornerError(detectCardQuadCv(cv, frame.imageData()), truth)).toBeLessThanOrEqual(0.03)
  })

  it('snaps to a rotated card (rounded corners still resolve to 4)', () => {
    const frame = new Frame(320, 240, 50)
    const { w, h } = cardDims(185)
    const truth = frame.drawCard(160, 120, w, h, 9, 205)
    expect(maxCornerError(detectCardQuadCv(cv, frame.imageData()), truth)).toBeLessThanOrEqual(0.03)
  })

  it('recovers the outer edge when a finger notches into the card boundary', () => {
    const frame = new Frame(320, 240, 50)
    const { w, h } = cardDims(190)
    const truth = frame.drawCard(160, 120, w, h, 0, 205)
    // A fingertip overlapping the left edge: mostly inside the card, protruding a little.
    frame.drawDisk(160 - w / 2 + 15, 120, 20, 120)
    expect(maxCornerError(detectCardQuadCv(cv, frame.imageData()), truth)).toBeLessThanOrEqual(
      0.045,
    )
  })

  it('survives a glare gap that breaks the card outline open', () => {
    const frame = new Frame(320, 240, 50)
    const { w, h } = cardDims(190)
    const truth = frame.drawCard(160, 120, w, h, 0, 205)
    // A card-bright glare patch straddling the right edge merges into the outline as
    // a convex bump, so the contour no longer simplifies to 4 corners at the finest
    // epsilon — the escalating epsilon ladder is what absorbs it.
    frame.drawDisk(160 + w / 2, 150, 16, 205)
    expect(maxCornerError(detectCardQuadCv(cv, frame.imageData()), truth)).toBeLessThanOrEqual(0.06)
  })

  it('still finds a low-contrast card (adaptive Canny / Otsu fallback)', () => {
    const frame = new Frame(320, 240, 90)
    const { w, h } = cardDims(190)
    const truth = frame.drawCard(160, 120, w, h, 0, 115)
    frame.addNoise(2)
    expect(maxCornerError(detectCardQuadCv(cv, frame.imageData()), truth)).toBeLessThanOrEqual(0.03)
  })

  it("prefers the card's outer edge over a strong inner rectangle", () => {
    const frame = new Frame(320, 240, 40)
    const { w, h } = cardDims(200)
    const truth = frame.drawCard(160, 120, w, h, 0, 200)
    // A crisp card-shaped inner frame (like an art box) that also yields a clean quad.
    frame.drawCard(160, 120, Math.round(w * 0.8), Math.round(h * 0.8), 0, 70, 1)
    expect(maxCornerError(detectCardQuadCv(cv, frame.imageData()), truth)).toBeLessThanOrEqual(0.03)
  })

  it('reports no card for a busy but cardless frame', () => {
    const frame = new Frame(320, 240, 100)
    frame.addNoise(40)
    expect(detectCardQuadCv(cv, frame.imageData())).toBeNull()
  })

  it('rejects a near-full-frame blob even on a card-shaped portrait frame', () => {
    // A portrait frame is itself nearly card-aspect, so without the max-area guard a
    // wall/table filling the view would falsely lock as a giant card.
    const frame = new Frame(240, 320, 30)
    frame.drawCard(120, 160, 236, 316, 0, 200, 1)
    expect(detectCardQuadCv(cv, frame.imageData())).toBeNull()
  })

  // ——— Discriminating cases: each fails if its specific mechanism is removed. ———

  it('finds a dim low-contrast card that only median-scaled Canny thresholds can see', () => {
    // A dim card (70 on background 40) whose edge response sits far below the old
    // fixed 60/160 thresholds, in the scene whose glare band also defeats the Otsu
    // retry — only the adaptive edge pass can resolve this frame.
    const frame = dimSceneFrame()
    const { w, h } = cardDims(150)
    const truth = frame.drawCard(160, 150, w, h, 0, 70)
    expect(maxCornerError(detectCardQuadCv(cv, frame.imageData()), truth)).toBeLessThanOrEqual(0.03)
  })

  it('prefers a true-aspect card over a LARGER wrong-aspect rectangle (aspect weighting)', () => {
    // Two clean rectangles: the right one is ~28% bigger by area but its aspect error
    // (~0.2) is near the tolerance edge; pure largest-area scoring picks it, the
    // aspect-weighted score must pick the true card shape.
    const frame = new Frame(320, 240, 50)
    const { w, h } = cardDims(150)
    const truth = frame.drawCard(80, 120, w, h, 0, 205, 1)
    frame.drawCard(240, 120, 138, 150, 0, 205, 1)
    expect(maxCornerError(detectCardQuadCv(cv, frame.imageData()), truth)).toBeLessThanOrEqual(0.03)
  })

  it('bridges small defocus breaks on BOTH side edges (morphological close)', () => {
    // Two small washed-out stretches on opposite edges split the outline into a top
    // and a bottom arc, each individually failing the card gates — only closing the
    // edge map re-joins them into one card contour. The scene is dim with a bright
    // glare band (as in the adaptive-Canny case) so the Otsu retry cannot quietly
    // rescue the frame instead: this must be solved on the edge path.
    const frame = dimSceneFrame()
    const { w, h } = cardDims(150)
    const truth = frame.drawCard(160, 150, w, h, 0, 70)
    const left = 160 - w / 2
    const right = 160 + w / 2
    frame.washPatch(left - 10, 146, left + 10, 154, 3)
    frame.washPatch(right - 10, 146, right + 10, 154, 3)
    expect(maxCornerError(detectCardQuadCv(cv, frame.imageData()), truth)).toBeLessThanOrEqual(0.03)
  })

  it('recovers from one WIDE washed-out edge stretch via the convex hull', () => {
    // A wide defocus/glare wash leaves an open C-shaped edge chain no closing kernel
    // can re-join; its raw contour is a thin band that fails every gate, but its hull
    // is still the card's outer quad. Dim scene again, so Otsu cannot rescue it.
    const frame = dimSceneFrame()
    const { w, h } = cardDims(150)
    const truth = frame.drawCard(160, 150, w, h, 0, 70)
    const right = 160 + w / 2
    frame.washPatch(right - 16, 128, right + 16, 172, 8)
    expect(maxCornerError(detectCardQuadCv(cv, frame.imageData()), truth)).toBeLessThanOrEqual(0.04)
  })

  it('does not report a cut-corner octagon as a card (hull-coverage gate)', () => {
    // A card-aspect blob with big straight-cut corners: the epsilon ladder collapses
    // its 8-point hull to a 4-point parallelogram whose sides sit inside the aspect
    // tolerance — but that quad covers well under the coverage floor of the hull.
    // Without the gate this blob would read (and crop) as a card.
    const frame = new Frame(320, 240, 50)
    frame.drawOctagon(160, 120, 136, 190, 40, 205)
    expect(detectCardQuadCv(cv, frame.imageData())).toBeNull()
  })
})
