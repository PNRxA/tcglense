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
import { cropImageData, priorSearchWindow } from '../guidedDetect'
import { detectCardQuadCv, loadOpenCv, medianLuma } from '../opencvDetect'
import { guideTarget } from '../quadSelect'
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
    const detected = detectCardQuadCv(cv, frame.imageData(), { fallbackPasses: false })
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
    expect(detectCardQuadCv(cv, frame.imageData(), { fallbackPasses: false })).toBeNull()
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

  // ——— Busy-background cases: realistic optics on cluttered scenes. Each pins one
  // escalation-ladder or arbitration mechanism; the failure modes were reproduced
  // against the pre-ladder detector before the mechanisms existed. ———

  it('finds a weak-edged dark card on dark busy wood under a bright region (low band)', () => {
    // The bright band drags the LUMA median far above the soft card edge, so the
    // median-scaled Canny thresholds miss ~80% of the boundary while the wood grain
    // still fires — only the fixed low band sees the whole outline.
    const frame = new Frame(320, 240, 50)
    frame.fillRect(0, 0, 320, 80, 190)
    for (let y = 80; y < 240; y += 5) frame.fillRect(0, y, 320, y + 2, 70)
    const { w, h } = cardDims(150)
    const truth = frame.drawCard(160, 155, w, h, 0, 35)
    frame.blur(1, 1)
    const acquired = detectCardQuadCv(cv, frame.imageData(), {
      select: { mode: 'acquisition', guide: guideTarget(320, 240) },
    })
    expect(maxCornerError(acquired, truth)).toBeLessThanOrEqual(0.03)
  })

  it('finds a defocused card whose luma sits inside the texture range (low band + interior ring)', () => {
    // Defocus drops the card edge below the luma-derived median thresholds, so only
    // the fixed low band sees the full boundary — the card surfaces as the hole its
    // border moat leaves in the closed texture blob. The tight tolerance additionally pins the interior-ring
    // arbitration: the always-on median band produces a plausible quad that leaked
    // half a tile outward through hysteresis gaps (error ~0.076), and only the
    // interior-clearance ranking lets the low band's true quad outrank it.
    const frame = new Frame(320, 240, 60)
    frame.checkerboard(18, 90, 170)
    const { w, h } = cardDims(190)
    const truth = frame.drawCard(160, 120, w, h, 0, 130)
    frame.blur(1, 1)
    frame.addNoise(3)
    const select = { mode: 'acquisition', guide: guideTarget(320, 240) } as const
    expect(
      maxCornerError(detectCardQuadCv(cv, frame.imageData(), { select }), truth),
    ).toBeLessThanOrEqual(0.03)
    // The recovery lives in the fallback passes — the cadence gate really gates it.
    expect(detectCardQuadCv(cv, frame.imageData(), { select, fallbackPasses: false })).toBeNull()
  })

  it('finds a motion-smeared card on a busy background', () => {
    const frame = new Frame(320, 240, 60)
    frame.checkerboard(14, 50, 130)
    const { w, h } = cardDims(190)
    const truth = frame.drawCard(160, 120, w, h, 2, 200)
    frame.motionBlurX(7)
    frame.addNoise(3)
    const acquired = detectCardQuadCv(cv, frame.imageData(), {
      select: { mode: 'acquisition', guide: guideTarget(320, 240) },
    })
    expect(maxCornerError(acquired, truth)).toBeLessThanOrEqual(0.03)
  })

  it('finds a soft-focus card on a bright busy background', () => {
    const frame = new Frame(320, 240, 60)
    frame.checkerboard(16, 150, 190)
    const { w, h } = cardDims(190)
    const truth = frame.drawCard(160, 120, w, h, 0, 120)
    frame.blur(2, 2)
    frame.addNoise(3)
    const acquired = detectCardQuadCv(cv, frame.imageData(), {
      select: { mode: 'acquisition', guide: guideTarget(320, 240) },
    })
    expect(maxCornerError(acquired, truth)).toBeLessThanOrEqual(0.05)
  })

  it('keeps a perspective trapezoid tight on a textured background (support arbitration)', () => {
    // Texture glued to the outline pulls hull corners outward; the perimeter-support
    // ranking prefers the pass whose quad actually traces the boundary.
    const frame = new Frame(320, 240, 60)
    frame.checkerboard(16, 80, 140)
    const cx = 160
    const cy = 120
    const truth: Quad = [
      { x: (cx - 60) / 320, y: (cy - 88) / 240 },
      { x: (cx + 60) / 320, y: (cy - 88) / 240 },
      { x: (cx + 70) / 320, y: (cy + 88) / 240 },
      { x: (cx - 70) / 320, y: (cy + 88) / 240 },
    ]
    for (let y = cy - 88; y <= cy + 88; y++) {
      const t = (y - (cy - 88)) / 176
      const hw = 60 + 10 * t
      frame.fillRect(Math.round(cx - hw), y, Math.round(cx + hw), y + 1, 205)
    }
    frame.blur(1, 1)
    const acquired = detectCardQuadCv(cv, frame.imageData(), {
      select: { mode: 'acquisition', guide: guideTarget(320, 240) },
    })
    expect(maxCornerError(acquired, truth)).toBeLessThanOrEqual(0.035)
  })

  it.each([
    ['checkerboard', (frame: Frame) => frame.checkerboard(18, 90, 170)],
    [
      'stripes with noise',
      (frame: Frame) => {
        for (let y = 0; y < 240; y += 6) frame.fillRect(0, y, 320, y + 3, 110)
        frame.addNoise(6)
      },
    ],
  ])('stays null on a cardless %s texture even with every fallback pass', (_name, texture) => {
    const frame = new Frame(320, 240, 60)
    texture(frame)
    frame.blur(1, 1)
    expect(
      detectCardQuadCv(cv, frame.imageData(), {
        select: { mode: 'acquisition', guide: guideTarget(320, 240) },
      }),
    ).toBeNull()
  })

  // ——— Guided modes: spatial priors change selection, never the geometric gates. ———

  it('acquisition locks the card at the guide, not a larger distractor at the frame edge', () => {
    const frame = new Frame(400, 240, 50)
    const { w, h } = cardDims(150)
    const truth = frame.drawCard(200, 120, w, h, 0, 205, 1)
    // A larger, equally card-shaped rectangle near the right edge (a book, a phone).
    const distractor = frame.drawCard(330, 120, Math.round(170 * (61 / 85)), 170, 0, 160, 1)
    const guide = guideTarget(400, 240)

    // Classic selection is area-dominant: the distractor demonstrably wins it.
    const plain = detectCardQuadCv(cv, frame.imageData())
    expect(maxCornerError(plain, distractor)).toBeLessThanOrEqual(0.03)
    // Guide-weighted acquisition picks the card the user is aiming.
    const acquired = detectCardQuadCv(cv, frame.imageData(), {
      select: { mode: 'acquisition', guide },
    })
    expect(maxCornerError(acquired, truth)).toBeLessThanOrEqual(0.03)
  })

  it('acquisition refuses a lone candidate far outside the guide', () => {
    const frame = new Frame(400, 240, 50)
    frame.drawCard(330, 120, Math.round(170 * (61 / 85)), 170, 0, 160, 1)
    const acquired = detectCardQuadCv(cv, frame.imageData(), {
      select: { mode: 'acquisition', guide: guideTarget(400, 240) },
    })
    expect(acquired).toBeNull()
  })

  it('tracking keeps the prior card identity against a larger newcomer', () => {
    const frame = new Frame(400, 240, 50)
    const { w, h } = cardDims(140)
    const truth = frame.drawCard(120, 120, w, h, 0, 205, 1)
    const larger = frame.drawCard(300, 120, Math.round(170 * (61 / 85)), 170, 0, 205, 1)
    // Classic selection prefers the larger card; a prior on the smaller one must not.
    expect(maxCornerError(detectCardQuadCv(cv, frame.imageData()), larger)).toBeLessThanOrEqual(
      0.03,
    )
    const prior = truth.map((p) => ({ x: p.x + 0.005, y: p.y })) as Quad
    const tracked = detectCardQuadCv(cv, frame.imageData(), {
      select: { mode: 'tracking', prior },
    })
    expect(maxCornerError(tracked, truth)).toBeLessThanOrEqual(0.03)
  })

  it('detects inside a prior ROI crop and maps corners back to full-frame coordinates', () => {
    const frame = new Frame(320, 240, 60)
    frame.checkerboard(16, 80, 140)
    const { w, h } = cardDims(170)
    const truth = frame.drawCard(160, 120, w, h, 3, 205)
    frame.blur(1, 1)
    const window = priorSearchWindow(truth, 320, 240)
    const crop = cropImageData(frame.imageData(), window)
    const detected = detectCardQuadCv(cv, crop, {
      window,
      select: { mode: 'tracking', prior: truth },
    })
    expect(maxCornerError(detected, truth)).toBeLessThanOrEqual(0.03)
  })

  it('never invents a corner at an artificial crop edge', () => {
    // The crop cuts through a complete, detectable card. Its contour touches the
    // artificial right edge of the window, where geometry is unknown — the detector
    // must skip it rather than hull-complete a fake boundary corner.
    const frame = new Frame(400, 240, 50)
    const { w, h } = cardDims(190)
    const truth = frame.drawCard(200, 120, w, h, 0, 205)
    expect(maxCornerError(detectCardQuadCv(cv, frame.imageData()), truth)).toBeLessThanOrEqual(0.03)
    const window = { x: 0, y: 0, width: 200, height: 240, fullWidth: 400, fullHeight: 240 }
    const crop = cropImageData(frame.imageData(), window)
    expect(detectCardQuadCv(cv, crop, { window })).toBeNull()
  })

  it('reproduces a busy-background live lock at capture resolution via the prior ROI', () => {
    // The capture path must rediscover whatever the live loop locked, at a different
    // resolution — otherwise a busy-background lock still fails at commit time.
    const scene = (width: number): { frame: Frame; truth: Quad } => {
      const scale = width / 320
      const height = Math.round(240 * scale)
      const frame = new Frame(width, height, 60)
      frame.checkerboard(Math.round(20 * scale), 60, 140)
      const { w, h } = cardDims(Math.round(190 * scale))
      const truth = frame.drawCard(width / 2, height / 2, w, h, 0, 205)
      return { frame, truth }
    }
    const live = scene(320)
    const liveQuad = detectCardQuadCv(cv, live.frame.imageData(), {
      select: { mode: 'acquisition', guide: guideTarget(320, 240) },
    })
    expect(maxCornerError(liveQuad, live.truth)).toBeLessThanOrEqual(0.03)

    const capture = scene(1000)
    const window = priorSearchWindow(liveQuad!, 1000, 750)
    const captured = detectCardQuadCv(cv, cropImageData(capture.frame.imageData(), window), {
      window,
      select: { mode: 'capture', prior: liveQuad! },
    })
    expect(maxCornerError(captured, capture.truth)).toBeLessThanOrEqual(0.02)
  })
})
