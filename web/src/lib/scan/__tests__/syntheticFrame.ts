// Shared synthetic-camera-frame builder for the scan detector specs. Draws grayscale-ish
// RGBA frames (all channels equal) with known ground-truth geometry: rounded rotated card
// rectangles, blobs, lines, textures — plus the realistic-optics degradations (defocus
// blur, motion smear, soft shading, glare washes, sensor noise) the busy-background cases
// need. Everything is deterministic (seeded LCG noise, integer rasterisation) so the
// specs' accuracy assertions are stable.

import type { Quad } from '../detect'
import { CARD_ASPECT } from '../regions'

export class Frame {
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

  /** Luma at (x, y), clamped to the frame (edge pixels repeat). */
  get(x: number, y: number): number {
    const cx = Math.min(this.width - 1, Math.max(0, x))
    const cy = Math.min(this.height - 1, Math.max(0, y))
    return this.data[(cy * this.width + cx) * 4]!
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

  /** Draw a deterministic integer line, including a square `width` around each point. */
  drawLine(x0: number, y0: number, x1: number, y1: number, value: number, width = 1) {
    const dx = Math.abs(x1 - x0)
    const sx = x0 < x1 ? 1 : -1
    const dy = -Math.abs(y1 - y0)
    const sy = y0 < y1 ? 1 : -1
    let error = dx + dy
    const radius = Math.floor(width / 2)
    while (true) {
      this.fillRect(x0 - radius, y0 - radius, x0 + radius + 1, y0 + radius + 1, value)
      if (x0 === x1 && y0 === y1) break
      const twice = 2 * error
      if (twice >= dy) {
        error += dy
        x0 += sx
      }
      if (twice <= dx) {
        error += dx
        y0 += sy
      }
    }
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

  /** A checkerboard texture over the whole frame — the workhorse busy background: strong
   * internal edges everywhere, in a controllable luma range. */
  checkerboard(tile: number, a: number, b: number) {
    for (let y = 0; y < this.height; y += tile) {
      for (let x = 0; x < this.width; x += tile) {
        const value = ((x / tile) | 0) % 2 === ((y / tile) | 0) % 2 ? a : b
        this.fillRect(x, y, x + tile, y + tile, value)
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

  /** Whole-frame box blur (radius `r`, `passes` iterations ≈ gaussian) — camera defocus.
   * Softens every edge in the scene, card boundary included. */
  blur(r: number, passes = 2) {
    for (let p = 0; p < passes; p++) {
      const snapshot = new Uint8ClampedArray(this.data)
      const at = (x: number, y: number) => {
        const cx = Math.min(this.width - 1, Math.max(0, x))
        const cy = Math.min(this.height - 1, Math.max(0, y))
        return snapshot[(cy * this.width + cx) * 4]!
      }
      for (let y = 0; y < this.height; y++) {
        for (let x = 0; x < this.width; x++) {
          let sum = 0
          let n = 0
          for (let dy = -r; dy <= r; dy++) {
            for (let dx = -r; dx <= r; dx++) {
              sum += at(x + dx, y + dy)
              n++
            }
          }
          this.set(x, y, Math.round(sum / n))
        }
      }
    }
  }

  /** Horizontal motion smear averaging `len` neighbouring pixels — a moving hand. */
  motionBlurX(len: number) {
    const snapshot = new Uint8ClampedArray(this.data)
    const at = (x: number, y: number) => {
      const cx = Math.min(this.width - 1, Math.max(0, x))
      return snapshot[(y * this.width + cx) * 4]!
    }
    for (let y = 0; y < this.height; y++) {
      for (let x = 0; x < this.width; x++) {
        let sum = 0
        for (let k = 0; k < len; k++) sum += at(x + k - Math.floor(len / 2), y)
        this.set(x, y, Math.round(sum / len))
      }
    }
  }

  /** Multiply a rect's luma by `factor`, feathered to nothing at the rect border — a soft
   * cast shadow that darkens what's under it without a hard boundary of its own. */
  shade(x0: number, y0: number, x1: number, y1: number, factor: number, feather = 8) {
    const smooth = (t: number) => {
      const c = Math.min(1, Math.max(0, t))
      return c * c * (3 - 2 * c)
    }
    for (let y = y0; y < y1; y++) {
      for (let x = x0; x < x1; x++) {
        const w =
          smooth(Math.min(x - x0, x1 - 1 - x) / feather) *
          smooth(Math.min(y - y0, y1 - 1 - y) / feather)
        const v = this.get(x, y)
        this.set(x, y, Math.round(v * (1 - w) + v * factor * w))
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
export function maxCornerError(detected: Quad | null, truth: Quad): number {
  if (!detected) return Infinity
  let max = 0
  for (let i = 0; i < 4; i++) {
    max = Math.max(max, Math.hypot(detected[i]!.x - truth[i]!.x, detected[i]!.y - truth[i]!.y))
  }
  return max
}

/** A centred card `heightPx` tall at the standard aspect. */
export function cardDims(heightPx: number): { w: number; h: number } {
  return { w: Math.round(heightPx * CARD_ASPECT), h: heightPx }
}
