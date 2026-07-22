// @vitest-environment node
//
// Integration spec for the OpenCV card detector — runs the REAL OpenCV.js WASM runtime
// (Node build, no DOM needed) against synthetic camera frames with known ground-truth
// corners, so the pipeline's accuracy claims (adaptive thresholds, hull recovery, the
// Otsu fallback, outer-edge preference) are actually exercised, not just type-checked.
// The runtime is a ~13 MB payload: loaded once for the whole file (~1-2 s).

import { createRequire } from 'node:module'
import { beforeAll, describe, expect, it } from 'vitest'
import { detectCardQuadCv, loadOpenCv, medianLuma } from '../opencvDetect'
import { Frame, cardDims, maxCornerError } from './syntheticFrame'

type Cv = Awaited<ReturnType<typeof loadOpenCv>>

let cv: Cv

beforeAll(async () => {
  // opencv.js's CJS export is a thenable that resolves to the ready runtime, which
  // vitest's ESM interop mis-assimilates on a dynamic import — so this spec require()s
  // it directly. `loadOpenCv`'s browser load path is exercised by the app build.
  const mod = createRequire(import.meta.url)('@techstark/opencv-js') as PromiseLike<unknown>
  cv = (await mod) as Cv
}, 60_000)

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

  it('rejects a card contour tethered to the frame instead of snapping one corner', () => {
    const frame = new Frame(320, 240, 50)
    const { w, h } = cardDims(190)
    const truth = frame.drawCard(160, 120, w, h, 0, 205, 1)
    // A foreground streak joins only the card's top-left corner to the camera boundary.
    // The old hull completed that contour as a plausible card with one corner at y=0;
    // selecting an uncontaminated inner contour is also a safe outcome.
    frame.drawLine(92, 25, 59, 0, 205, 3)
    const detected = detectCardQuadCv(cv, frame.imageData(), { segmentationFallback: false })
    expect(detected === null || maxCornerError(detected, truth) <= 0.03).toBe(true)
  })

  it.each([
    ['top', 160, 92],
    ['right', 254, 120],
    ['bottom', 160, 148],
    ['left', 66, 120],
  ])('rejects a card clipped at the %s frame edge', (_edge, cx, cy) => {
    const frame = new Frame(320, 240, 50)
    const { w, h } = cardDims(190)
    frame.drawCard(cx, cy, w, h, 0, 205, 1)
    expect(detectCardQuadCv(cv, frame.imageData())).toBeNull()
  })

  it.each([0.6, 0.8])('rejects an inner frame at scale %s of a clipped outer card', (scale) => {
    const frame = new Frame(320, 240, 50)
    const { w, h } = cardDims(190)
    frame.drawCard(66, 120, w, h, 0, 205, 1)
    // Printed frame/art borders remain fully visible when the physical card is clipped.
    // They must not replace the rejected outer contour and become a too-tight crop.
    frame.drawCard(66, 120, Math.round(w * scale), Math.round(h * scale), 0, 70, 1)
    expect(detectCardQuadCv(cv, frame.imageData(), { segmentationFallback: false })).toBeNull()
    expect(detectCardQuadCv(cv, frame.imageData())).toBeNull()
  })

  it('still accepts a complete rotated card just beyond the safety inset', () => {
    const frame = new Frame(320, 240, 50)
    const { w, h } = cardDims(190)
    const truth = frame.drawCard(160, 117, w, h, 15, 205, 1)
    expect(maxCornerError(detectCardQuadCv(cv, frame.imageData()), truth)).toBeLessThanOrEqual(0.02)
  })

  it('ignores a frame-attached distractor when a complete card is present', () => {
    const frame = new Frame(400, 300, 50)
    const { w, h } = cardDims(190)
    const truth = frame.drawCard(275, 150, w, h, 0, 205, 1)
    frame.fillRect(0, 55, 115, 245, 205)
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
