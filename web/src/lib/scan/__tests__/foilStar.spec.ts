// @vitest-environment node
//
// Foil-star detector spec. The pure radial-signature maths is tested directly on synthetic
// point sets; the end-to-end `detectFoilStar` runs the REAL OpenCV.js WASM runtime (Node
// build) against synthetic card crops with a star / bullet / nothing drawn into the info-line
// region — so the geometric discriminator is exercised, not just type-checked. Mirrors
// `opencvDetect.spec.ts`. The runtime is a ~13 MB payload loaded once for the file.

import { createRequire } from 'node:module'
import { beforeAll, describe, expect, it } from 'vitest'
import { detectFoilStar, isStarSignature, radialSignature, STAR_REGION } from '../foilStar'

type Cv = Parameters<typeof detectFoilStar>[0]

let cv: Cv

beforeAll(async () => {
  const mod = createRequire(import.meta.url)('@techstark/opencv-js') as PromiseLike<unknown>
  cv = (await mod) as Cv
}, 60_000)

interface Pt {
  x: number
  y: number
}

/** The 10 vertices of a 5-pointed star (outer radius `R`, inner `r`), point-up + `rot`. */
function starVertices(cx: number, cy: number, R: number, r: number, rot = 0): Pt[] {
  const pts: Pt[] = []
  for (let i = 0; i < 10; i++) {
    const ang = -Math.PI / 2 + rot + (i * Math.PI) / 5
    const rad = i % 2 === 0 ? R : r
    pts.push({ x: cx + rad * Math.cos(ang), y: cy + rad * Math.sin(ang) })
  }
  return pts
}

/** Densely sample a polygon's boundary (for the pure signature tests). */
function sampleBoundary(pts: Pt[], perEdge = 24): { xs: number[]; ys: number[] } {
  const xs: number[] = []
  const ys: number[] = []
  for (let i = 0; i < pts.length; i++) {
    const a = pts[i]!
    const b = pts[(i + 1) % pts.length]!
    for (let t = 0; t < perEdge; t++) {
      const f = t / perEdge
      xs.push(a.x + f * (b.x - a.x))
      ys.push(a.y + f * (b.y - a.y))
    }
  }
  return { xs, ys }
}

/** A synthetic grayscale RGBA card crop (61:85), with an even-odd polygon fill. */
class Crop {
  data: Uint8ClampedArray
  constructor(
    public width: number,
    public height: number,
    background: number,
  ) {
    this.data = new Uint8ClampedArray(width * height * 4)
    for (let i = 0; i < width * height; i++) {
      this.data[i * 4] = background
      this.data[i * 4 + 1] = background
      this.data[i * 4 + 2] = background
      this.data[i * 4 + 3] = 255
    }
  }

  private set(x: number, y: number, v: number) {
    if (x < 0 || y < 0 || x >= this.width || y >= this.height) return
    const i = (y * this.width + x) * 4
    this.data[i] = v
    this.data[i + 1] = v
    this.data[i + 2] = v
  }

  fillPolygon(pts: Pt[], value: number) {
    const minY = Math.max(0, Math.floor(Math.min(...pts.map((p) => p.y))))
    const maxY = Math.min(this.height - 1, Math.ceil(Math.max(...pts.map((p) => p.y))))
    for (let y = minY; y <= maxY; y++) {
      const crossings: number[] = []
      for (let i = 0; i < pts.length; i++) {
        const a = pts[i]!
        const b = pts[(i + 1) % pts.length]!
        if ((a.y <= y && b.y > y) || (b.y <= y && a.y > y)) {
          crossings.push(a.x + ((y - a.y) / (b.y - a.y)) * (b.x - a.x))
        }
      }
      crossings.sort((p, q) => p - q)
      for (let k = 0; k + 1 < crossings.length; k += 2) {
        for (let x = Math.round(crossings[k]!); x <= Math.round(crossings[k + 1]!); x++) {
          this.set(x, y, value)
        }
      }
    }
  }

  fillDisk(cx: number, cy: number, radius: number, value: number) {
    for (let y = Math.floor(cy - radius); y <= cy + radius; y++) {
      for (let x = Math.floor(cx - radius); x <= cx + radius; x++) {
        if ((x - cx) ** 2 + (y - cy) ** 2 <= radius * radius) this.set(x, y, value)
      }
    }
  }

  /** Centre of the star region in crop pixels, and the region's pixel height. */
  regionCentre(): { cx: number; cy: number; regionHeight: number } {
    return {
      cx: (STAR_REGION.left + STAR_REGION.width * 0.3) * this.width,
      cy: (STAR_REGION.top + STAR_REGION.height * 0.5) * this.height,
      regionHeight: STAR_REGION.height * this.height,
    }
  }

  imageData(): ImageData {
    return { data: this.data, width: this.width, height: this.height } as unknown as ImageData
  }
}

/** A card crop at the scanner's warp aspect, with a shape drawn in the info-line region. */
function cropWith(draw: (crop: Crop, c: ReturnType<Crop['regionCentre']>) => void, bg = 20): Crop {
  const crop = new Crop(1220, 1700, bg)
  draw(crop, crop.regionCentre())
  return crop
}

describe('radialSignature / isStarSignature', () => {
  it('recognises a five-pointed star: 5 even peaks over deep valleys', () => {
    const { xs, ys } = sampleBoundary(starVertices(100, 100, 40, 20))
    const sig = radialSignature(xs, ys, xs.length, 100, 100)
    expect(sig.peaks).toBe(5)
    expect(sig.gapRatio).toBeLessThan(1.3)
    expect(sig.valleyRatio).toBeLessThan(0.62)
    expect(isStarSignature(sig)).toBe(true)
  })

  it('recognises a rotated star (the peak test is rotation-invariant)', () => {
    const { xs, ys } = sampleBoundary(starVertices(100, 100, 40, 20, 0.6))
    expect(isStarSignature(radialSignature(xs, ys, xs.length, 100, 100))).toBe(true)
  })

  it('rejects a circle (no distinct peaks)', () => {
    const xs: number[] = []
    const ys: number[] = []
    for (let a = 0; a < 240; a++) {
      xs.push(100 + 40 * Math.cos((a / 240) * 2 * Math.PI))
      ys.push(100 + 40 * Math.sin((a / 240) * 2 * Math.PI))
    }
    expect(isStarSignature(radialSignature(xs, ys, xs.length, 100, 100))).toBe(false)
  })

  it('rejects a lopsided 5-spike shape (uneven gaps)', () => {
    // Five points crammed into a half-turn, then a long empty arc: 5 peaks but wildly uneven.
    const pts: Pt[] = []
    for (let i = 0; i < 5; i++) {
      const ang = (i / 5) * Math.PI // spikes over only 180°
      pts.push({ x: 100 + 40 * Math.cos(ang), y: 100 + 40 * Math.sin(ang) })
      pts.push({ x: 100 + 12 * Math.cos(ang + 0.3), y: 100 + 12 * Math.sin(ang + 0.3) })
    }
    const { xs, ys } = sampleBoundary(pts)
    const sig = radialSignature(xs, ys, xs.length, 100, 100)
    // Either not five clean peaks, or five peaks that fail the even-spacing gate — never a star.
    expect(isStarSignature(sig)).toBe(false)
  })
})

describe('detectFoilStar', () => {
  it('detects a light star on a dark border (the common foil layout)', () => {
    const crop = cropWith((c, { cx, cy, regionHeight }) => {
      c.fillPolygon(starVertices(cx, cy, regionHeight * 0.42, regionHeight * 0.21), 230)
    }, 20)
    expect(detectFoilStar(cv, crop.imageData())).toBe(true)
  })

  it('detects a dark star on a light border (inverse Otsu polarity)', () => {
    const crop = cropWith((c, { cx, cy, regionHeight }) => {
      c.fillPolygon(starVertices(cx, cy, regionHeight * 0.42, regionHeight * 0.21), 30)
    }, 235)
    expect(detectFoilStar(cv, crop.imageData())).toBe(true)
  })

  it('rejects a bullet (nonfoil: a solid dot prints in the star’s place)', () => {
    const crop = cropWith((c, { cx, cy, regionHeight }) => {
      c.fillDisk(cx, cy, regionHeight * 0.28, 230)
    }, 20)
    expect(detectFoilStar(cv, crop.imageData())).toBe(false)
  })

  it('rejects a blank crop (no info line resolved)', () => {
    expect(detectFoilStar(cv, cropWith(() => {}, 20).imageData())).toBe(false)
  })

  it('does not fire on a star drawn OUTSIDE the info-line region (art, top of card)', () => {
    const crop = new Crop(1220, 1700, 20)
    // A star up in the art box must not be read as a foil marker.
    crop.fillPolygon(starVertices(crop.width * 0.5, crop.height * 0.4, 60, 30), 230)
    expect(detectFoilStar(cv, crop.imageData())).toBe(false)
  })
})
