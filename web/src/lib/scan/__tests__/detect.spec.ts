import { describe, expect, it } from 'vitest'
import {
  detectCardQuad,
  orderCorners,
  quadArea,
  quadIsPlausibleCard,
  solveHomography,
  toGray,
  warpToRect,
  type Point,
  type Quad,
} from '../detect'

/** Apply a 3×3 homography (9 numbers, row-major) to a point. */
function applyH(h: number[], p: Point): Point {
  const d = h[6]! * p.x + h[7]! * p.y + h[8]!
  return { x: (h[0]! * p.x + h[1]! * p.y + h[2]!) / d, y: (h[3]! * p.x + h[4]! * p.y + h[5]!) / d }
}

/** A grayscale frame with a bright axis-aligned card rect on a dark background. */
function frameWithCard(
  width: number,
  height: number,
  rect: { x0: number; y0: number; x1: number; y1: number },
  bg = 20,
  fg = 210,
): Uint8Array {
  const gray = new Uint8Array(width * height).fill(bg)
  for (let y = rect.y0; y < rect.y1; y++) {
    for (let x = rect.x0; x < rect.x1; x++) gray[y * width + x] = fg
  }
  return gray
}

describe('orderCorners', () => {
  it('sorts arbitrary points into [TL, TR, BR, BL]', () => {
    const shuffled: Point[] = [
      { x: 100, y: 200 }, // BL
      { x: 100, y: 10 }, // TL
      { x: 300, y: 200 }, // BR
      { x: 300, y: 10 }, // TR
    ]
    const [tl, tr, br, bl] = orderCorners(shuffled)
    expect(tl).toEqual({ x: 100, y: 10 })
    expect(tr).toEqual({ x: 300, y: 10 })
    expect(br).toEqual({ x: 300, y: 200 })
    expect(bl).toEqual({ x: 100, y: 200 })
  })
})

describe('toGray', () => {
  it('applies the integer Rec.601 luma', () => {
    const rgba = new Uint8Array([255, 255, 255, 255, 0, 0, 0, 255])
    const gray = toGray(rgba, 2, 1)
    expect(gray[0]).toBe(255)
    expect(gray[1]).toBe(0)
  })
})

describe('solveHomography', () => {
  it('is the identity when source and destination match', () => {
    const q: Quad = [
      { x: 0, y: 0 },
      { x: 10, y: 0 },
      { x: 10, y: 14 },
      { x: 0, y: 14 },
    ]
    const h = solveHomography(q, q)!
    expect(h).not.toBeNull()
    for (const corner of q) {
      const out = applyH(h, corner)
      expect(out.x).toBeCloseTo(corner.x, 6)
      expect(out.y).toBeCloseTo(corner.y, 6)
    }
  })

  it('maps each destination corner to its source corner', () => {
    const dst: Quad = [
      { x: 0, y: 0 },
      { x: 61, y: 0 },
      { x: 61, y: 85 },
      { x: 0, y: 85 },
    ]
    const src: Quad = [
      { x: 12, y: 20 },
      { x: 190, y: 8 },
      { x: 205, y: 260 },
      { x: 8, y: 250 },
    ]
    const h = solveHomography(dst, src)!
    for (let i = 0; i < 4; i++) {
      const out = applyH(h, dst[i]!)
      expect(out.x).toBeCloseTo(src[i]!.x, 4)
      expect(out.y).toBeCloseTo(src[i]!.y, 4)
    }
  })
})

describe('warpToRect', () => {
  it('deskews an axis-aligned source rect, preserving its content', () => {
    // Source: a 100×100 RGBA image; inside a 40×60 rect the red channel is the x-coord.
    const srcW = 100
    const srcH = 100
    const rgba = new Uint8ClampedArray(srcW * srcH * 4)
    for (let y = 20; y < 80; y++) {
      for (let x = 20; x < 60; x++) {
        const i = (y * srcW + x) * 4
        rgba[i] = x // R = x
        rgba[i + 3] = 255
      }
    }
    const srcQuad: Quad = [
      { x: 20, y: 20 },
      { x: 60, y: 20 },
      { x: 60, y: 80 },
      { x: 20, y: 80 },
    ]
    const dstW = 40
    const dstH = 60
    const out = warpToRect(rgba, srcW, srcH, srcQuad, dstW, dstH)
    // dst(dx, dy) maps to src(20 + dx, 20 + dy), so R ≈ 20 + dx.
    const at = (dx: number, dy: number) => out[(dy * dstW + dx) * 4]!
    expect(at(10, 30)).toBeCloseTo(30, -1) // within ~a few of 30
    expect(at(30, 10)).toBeCloseTo(50, -1)
    expect(at(0, 0)).toBeCloseTo(20, -1)
  })
})

describe('quadIsPlausibleCard', () => {
  const frameW = 200
  const frameH = 280

  it('accepts a card-shaped quad filling much of the frame', () => {
    const quad: Quad = [
      { x: 30, y: 40 },
      { x: 170, y: 40 },
      { x: 170, y: 240 },
      { x: 30, y: 240 },
    ]
    expect(quadIsPlausibleCard(quad, frameW, frameH)).toBe(true)
  })

  it('rejects a wrong-aspect (square) quad', () => {
    const quad: Quad = [
      { x: 30, y: 30 },
      { x: 190, y: 30 },
      { x: 190, y: 190 },
      { x: 30, y: 190 },
    ]
    expect(quadIsPlausibleCard(quad, frameW, frameH)).toBe(false)
  })

  it('rejects a wedge (non-parallel opposite sides)', () => {
    const quad: Quad = [
      { x: 90, y: 40 },
      { x: 110, y: 40 },
      { x: 170, y: 240 },
      { x: 30, y: 240 },
    ]
    expect(quadIsPlausibleCard(quad, frameW, frameH)).toBe(false)
  })

  it('measures polygon area with the shoelace formula', () => {
    const quad: Quad = [
      { x: 0, y: 0 },
      { x: 10, y: 0 },
      { x: 10, y: 20 },
      { x: 0, y: 20 },
    ]
    expect(quadArea(quad)).toBe(200)
  })
})

describe('detectCardQuad', () => {
  it('finds the corners of a bright card on a dark background', () => {
    const width = 200
    const height = 280
    const rect = { x0: 30, y0: 40, x1: 170, y1: 240 }
    const gray = frameWithCard(width, height, rect)
    const quad = detectCardQuad(gray, width, height)
    expect(quad).not.toBeNull()
    const [tl, tr, br, bl] = quad!
    // Corners land on the rect (inclusive extrema, so x1/y1 map to x1-1/y1-1).
    expect(tl.x).toBeCloseTo(30, -1)
    expect(tl.y).toBeCloseTo(40, -1)
    expect(tr.x).toBeCloseTo(169, -1)
    expect(br.y).toBeCloseTo(239, -1)
    expect(bl.x).toBeCloseTo(30, -1)
  })

  it('returns null when there is no foreground (blank frame)', () => {
    const gray = new Uint8Array(200 * 280).fill(20)
    expect(detectCardQuad(gray, 200, 280)).toBeNull()
  })

  it('returns null when the whole frame is foreground (no background to segment)', () => {
    const gray = new Uint8Array(200 * 280).fill(210)
    expect(detectCardQuad(gray, 200, 280)).toBeNull()
  })

  it('returns null for a non-card-shaped bright blob (fails plausibility)', () => {
    // A wide, short bright band — wrong aspect, so no card is reported.
    const gray = frameWithCard(200, 280, { x0: 10, y0: 130, x1: 190, y1: 160 })
    expect(detectCardQuad(gray, 200, 280)).toBeNull()
  })
})
