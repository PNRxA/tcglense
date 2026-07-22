import { describe, expect, it } from 'vitest'
import type { Quad } from '../detect'
import type { QuadCandidate } from '../opencvContours'
import { guideTarget, selectCardQuad, selectScoredCardQuad } from '../quadSelect'
import { CARD_ASPECT, GUIDE_MARGIN } from '../regions'

/** An axis-aligned candidate centred at (cx, cy), `h` tall at the exact card aspect
 * unless a width override skews it. */
function candidateAt(
  cx: number,
  cy: number,
  h: number,
  options: { width?: number; shapeError?: number } = {},
): QuadCandidate {
  const w = options.width ?? h * CARD_ASPECT
  const quad: Quad = [
    { x: cx - w / 2, y: cy - h / 2 },
    { x: cx + w / 2, y: cy - h / 2 },
    { x: cx + w / 2, y: cy + h / 2 },
    { x: cx - w / 2, y: cy + h / 2 },
  ]
  return { quad, areaFraction: w * h, shapeError: options.shapeError ?? 0 }
}

describe('guideTarget', () => {
  it('is the centred guide box in normalised coordinates', () => {
    const target = guideTarget(640, 480)
    expect(target.cx).toBeCloseTo(0.5, 10)
    expect(target.cy).toBeCloseTo(0.5, 10)
    // Height-limited landscape frame: the guide is (1 - 2×margin) of the height.
    expect(target.halfH).toBeCloseTo((1 - 2 * GUIDE_MARGIN) / 2, 10)
  })
})

describe('selectCardQuad', () => {
  const guide = guideTarget(640, 480)

  it('plain mode keeps the classic area-dominant, aspect-weighted choice', () => {
    const bigger = candidateAt(0.3, 0.5, 0.5)
    const smaller = candidateAt(0.7, 0.5, 0.4)
    expect(selectCardQuad([smaller, bigger], { mode: 'plain' })).toEqual(bigger.quad)
    // A larger candidate with a near-tolerance aspect error loses to a true card shape.
    const wrongAspect = candidateAt(0.3, 0.5, 0.5, { shapeError: 0.26 })
    expect(selectCardQuad([wrongAspect, smaller], { mode: 'plain' })).toEqual(smaller.quad)
  })

  it('acquisition prefers the card at the guide over a larger off-centre distractor', () => {
    const atGuide = candidateAt(0.5, 0.5, 0.5)
    const distractor = candidateAt(0.12, 0.5, 0.62)
    expect(selectCardQuad([distractor, atGuide], { mode: 'acquisition', guide })).toEqual(
      atGuide.quad,
    )
  })

  it('acquisition rejects a candidate beyond the guide-distance gate outright', () => {
    // Push the candidate far outside the guide: on a 640×480 frame the guide's half
    // width is ~0.33 of the frame, so a centre at the left edge is well past 1.35.
    const remote = candidateAt(0.03, 0.06, 0.3)
    expect(selectCardQuad([remote], { mode: 'acquisition', guide })).toBeNull()
  })

  it('acquisition returns null when two distinct candidates are near-tied', () => {
    const left = candidateAt(0.42, 0.5, 0.5)
    const right = candidateAt(0.58, 0.5, 0.5)
    expect(selectCardQuad([left, right], { mode: 'acquisition', guide })).toBeNull()
  })

  it('deduplicates near-identical candidates instead of calling them ambiguous', () => {
    const first = candidateAt(0.5, 0.5, 0.5)
    const jittered = candidateAt(0.505, 0.5, 0.5)
    expect(selectCardQuad([first, jittered], { mode: 'acquisition', guide })).toEqual(first.quad)
  })

  it('tracking only accepts candidates associated with the prior', () => {
    const prior = candidateAt(0.5, 0.5, 0.5).quad
    const near = candidateAt(0.52, 0.5, 0.5)
    const far = candidateAt(0.8, 0.3, 0.5)
    expect(selectCardQuad([far], { mode: 'tracking', prior })).toBeNull()
    expect(selectCardQuad([far, near], { mode: 'tracking', prior })).toEqual(near.quad)
  })

  it('tracking rejects a single-corner excursion beyond the corner gate', () => {
    const prior = candidateAt(0.5, 0.5, 0.5).quad
    const near = candidateAt(0.5, 0.5, 0.5)
    const jumped: QuadCandidate = {
      ...near,
      quad: near.quad.map((p, i) => (i === 0 ? { x: p.x - 0.15, y: p.y - 0.15 } : p)) as Quad,
    }
    expect(selectCardQuad([jumped], { mode: 'tracking', prior })).toBeNull()
  })

  it('tracking rejects an area change beyond the same-card ratio', () => {
    const prior = candidateAt(0.5, 0.5, 0.5).quad
    const grown = candidateAt(0.5, 0.5, 0.56)
    expect(selectCardQuad([grown], { mode: 'tracking', prior })).toBeNull()
  })

  it('capture applies the stricter association gates', () => {
    const prior = candidateAt(0.5, 0.5, 0.5).quad
    // Mean drift of 0.06 is fine for tracking but beyond the capture gate.
    const drifted = candidateAt(0.56, 0.5, 0.5)
    expect(selectCardQuad([drifted], { mode: 'tracking', prior })).toEqual(drifted.quad)
    expect(selectCardQuad([drifted], { mode: 'capture', prior })).toBeNull()
  })

  it('returns the mode score alongside the quad for cross-pass arbitration', () => {
    const candidate = candidateAt(0.5, 0.5, 0.5)
    const selected = selectScoredCardQuad([candidate], { mode: 'plain' })
    expect(selected!.quad).toEqual(candidate.quad)
    expect(selected!.score).toBeCloseTo(candidate.areaFraction, 10)
  })
})
