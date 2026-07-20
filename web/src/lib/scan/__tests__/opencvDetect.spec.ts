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
    // Glare washes out a stretch of the right edge: a card-bright patch on the
    // background straddling the boundary, killing the contrast there. The raw contour
    // is no longer a clean closed quad — only the hull recovers the card.
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
})
