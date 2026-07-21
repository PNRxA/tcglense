import { describe, expect, it } from 'vitest'
import type { Quad } from '../detect'
import {
  blendQuads,
  createQuadTracker,
  maxCornerDistance,
  meanCornerDistance,
} from '../quadTracker'

/** An axis-aligned card-ish quad at (x, y), in normalised units. */
function quadAt(x: number, y: number, w = 0.4, h = 0.56): Quad {
  return [
    { x, y },
    { x: x + w, y },
    { x: x + w, y: y + h },
    { x, y: y + h },
  ]
}

describe('corner distances', () => {
  it('measures identical quads and a pure translation', () => {
    const a = quadAt(0.1, 0.1)
    expect(meanCornerDistance(a, a)).toBe(0)
    expect(maxCornerDistance(a, a)).toBe(0)
    const b = quadAt(0.13, 0.14)
    expect(meanCornerDistance(a, b)).toBeCloseTo(Math.hypot(0.03, 0.04), 10)
    expect(maxCornerDistance(a, b)).toBeCloseTo(Math.hypot(0.03, 0.04), 10)
  })
})

describe('blendQuads', () => {
  it('interpolates each corner linearly', () => {
    const a = quadAt(0, 0)
    const b = quadAt(0.2, 0.1)
    const mid = blendQuads(a, b, 0.5)
    expect(mid[0]).toEqual({ x: 0.1, y: 0.05 })
    expect(mid[2]!.x).toBeCloseTo(0.4 + 0.1, 10)
    // t=0 keeps the previous quad, t=1 adopts the new one.
    expect(blendQuads(a, b, 0)).toEqual(a)
    expect(blendQuads(a, b, 1)).toEqual(b)
  })
})

describe('createQuadTracker', () => {
  it('locks on immediately with the first detection', () => {
    const tracker = createQuadTracker()
    const q = quadAt(0.2, 0.15)
    expect(tracker.update(q)).toEqual(q)
  })

  it('smooths small per-frame jitter instead of following it raw', () => {
    const tracker = createQuadTracker({ blend: 0.5 })
    const base = quadAt(0.2, 0.15)
    tracker.update(base)
    const jittered = quadAt(0.21, 0.15) // 0.01 wobble — well inside snapDistance
    const out = tracker.update(jittered)!
    // The displayed quad moves only part-way toward the jittered detection.
    expect(out[0]!.x).toBeCloseTo(0.205, 10)
    expect(out[0]!.y).toBeCloseTo(0.15, 10)
  })

  it('holds instead of blending a single corner that jumps to the frame edge', () => {
    const tracker = createQuadTracker()
    const base = quadAt(0.2, 0.15)
    const malformed: Quad = base.map((point) => ({ ...point })) as Quad
    malformed[0] = { x: 0, y: 0 }

    expect(meanCornerDistance(base, malformed)).toBeLessThan(0.07)
    expect(maxCornerDistance(base, malformed)).toBeGreaterThan(0.08)
    tracker.update(base)
    expect(tracker.update(malformed)).toEqual(base)
    expect(tracker.update(malformed)).toEqual(base)
    expect(tracker.update(malformed)).toEqual(base)
    expect(tracker.update(malformed)).toBeNull()
    // Expiring the display hold must not let the same malformed quad reacquire as new.
    expect(tracker.update(malformed)).toBeNull()
    expect(tracker.update(base)).toEqual(base)
  })

  it('holds when two adjacent corners jump but their mean movement still reads as nearby', () => {
    const tracker = createQuadTracker()
    const base = quadAt(0.2, 0.15)
    const malformed: Quad = base.map((point) => ({ ...point })) as Quad
    malformed[0]!.y += 0.09
    malformed[1]!.y += 0.09

    expect(meanCornerDistance(base, malformed)).toBeLessThan(0.05)
    expect(maxCornerDistance(base, malformed)).toBeGreaterThan(0.08)
    tracker.update(base)
    expect(tracker.update(malformed)).toEqual(base)
  })

  it('converges onto a card that settles at a nearby position', () => {
    const tracker = createQuadTracker({ blend: 0.5 })
    tracker.update(quadAt(0.2, 0.15))
    const settled = quadAt(0.22, 0.15)
    let out: Quad | null = null
    for (let i = 0; i < 12; i++) out = tracker.update(settled)
    expect(out![0]!.x).toBeCloseTo(0.22, 3)
  })

  it('snaps instantly when the card moves far (a genuinely new position)', () => {
    const tracker = createQuadTracker({ snapDistance: 0.05 })
    tracker.update(quadAt(0.1, 0.1))
    const far = quadAt(0.4, 0.3)
    expect(tracker.update(far)).toEqual(far)
  })

  it('holds the last quad across up to holdTicks misses, then lets go', () => {
    const tracker = createQuadTracker({ holdTicks: 2 })
    const q = quadAt(0.2, 0.15)
    tracker.update(q)
    expect(tracker.update(null)).toEqual(q) // miss 1 — held
    expect(tracker.update(null)).toEqual(q) // miss 2 — held
    expect(tracker.update(null)).toBeNull() // beyond the hold — track lost
    // Once lost, it stays lost until a real detection.
    expect(tracker.update(null)).toBeNull()
  })

  it('a detection during the hold re-arms the full hold window', () => {
    const tracker = createQuadTracker({ holdTicks: 2 })
    const q = quadAt(0.2, 0.15)
    tracker.update(q)
    tracker.update(null) // miss 1
    tracker.update(q) // re-detected — misses reset
    expect(tracker.update(null)).not.toBeNull()
    expect(tracker.update(null)).not.toBeNull()
    expect(tracker.update(null)).toBeNull()
  })

  it('reports null immediately when there is no track to hold', () => {
    const tracker = createQuadTracker()
    expect(tracker.update(null)).toBeNull()
  })

  it('ships the intended defaults: smooth ≤0.05 apart, snap beyond, hold 3 ticks', () => {
    // useCardScanner calls createQuadTracker() bare, so THESE defaults are the live
    // scanner's behavior — pin them.
    const tracker = createQuadTracker()
    const base = quadAt(0.2, 0.15)
    tracker.update(base)
    // 0.04 mean offset — inside the default snapDistance, so it smooths (default
    // blend 0.5 puts the track at the midpoint), rather than snapping.
    const near = quadAt(0.24, 0.15)
    expect(tracker.update(near)![0]!.x).toBeCloseTo(0.22, 10)
    // 0.2 offset — beyond snapDistance: adopts the new position wholesale.
    const far = quadAt(0.44, 0.15)
    expect(tracker.update(far)).toEqual(far)
    // Held for exactly 3 misses, gone on the 4th.
    expect(tracker.update(null)).toEqual(far)
    expect(tracker.update(null)).toEqual(far)
    expect(tracker.update(null)).toEqual(far)
    expect(tracker.update(null)).toBeNull()
  })

  it('reset() forgets the track and the miss count', () => {
    const tracker = createQuadTracker({ holdTicks: 3 })
    tracker.update(quadAt(0.2, 0.15))
    tracker.reset()
    expect(tracker.update(null)).toBeNull()
    // A fresh detection after reset locks on cleanly (no stale smoothing source).
    const q = quadAt(0.5, 0.3)
    expect(tracker.update(q)).toEqual(q)
  })
})
