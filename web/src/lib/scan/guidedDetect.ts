// Pure spatial policy for guided card detection: where in the frame to search, how a
// cropped search window maps back to full-frame coordinates, and which crop edges are
// physical camera boundaries (with clipped-card semantics) versus artificial crop cuts
// (where geometry is merely unknown). No OpenCV, no canvas, no timers — everything here
// is plain geometry so it unit-tests without the WASM runtime.
//
// The live loop and the capture path both search a padded region of interest around a
// prior (the tracked green lock) when one exists: background clutter outside the ROI
// vanishes from the search entirely, which is what makes lock *retention* on a busy
// background robust, and the smaller crop is cheaper than a full-frame pass. Detection
// results stay in full-frame normalised coordinates throughout — only the pixel search
// area changes — so every downstream gate (frame clearance, area fractions, tracker
// association) keeps its meaning.

import type { Point, Quad } from './detect'

/** An integer pixel rectangle of a detection frame. */
export interface PixelRect {
  x: number
  y: number
  width: number
  height: number
}

/** A cropped search area's location within its full detection frame (all in the same
 * pixel space). The crop's own dimensions ride along so gates can be evaluated in
 * full-frame terms and crop pixels mapped back to full-frame coordinates. */
export interface SearchWindow extends PixelRect {
  fullWidth: number
  fullHeight: number
}

/** Which sides of a search window coincide with the full camera frame. A physical side
 * keeps the frame-edge posture (clipped-card blockers, inset clearance); an artificial
 * side only means the scene continues beyond the crop, so a contour touching it has
 * unknown geometry and is skipped — it must be neither a candidate nor a blocker. */
export interface PhysicalSides {
  left: boolean
  top: boolean
  right: boolean
  bottom: boolean
}

/** The whole frame as its own (trivially all-physical) search window. */
export function fullFrameWindow(width: number, height: number): SearchWindow {
  return { x: 0, y: 0, width, height, fullWidth: width, fullHeight: height }
}

export function physicalSides(window: SearchWindow): PhysicalSides {
  return {
    left: window.x === 0,
    top: window.y === 0,
    right: window.x + window.width === window.fullWidth,
    bottom: window.y + window.height === window.fullHeight,
  }
}

/** Fractional padding around a prior's bounds — generous enough that ordinary hand-held
 * drift between two 120 ms ticks stays inside the ROI. */
const ROI_PRIOR_PAD = 0.18
/** Padding floor as a fraction of the full frame (covers very small priors). */
const ROI_FRAME_PAD = 0.08
/** Absolute padding floor in pixels. */
const ROI_MIN_PAD = 12

/** The padded, clamped, integer search window around a prior quad (normalised 0..1
 * full-frame coordinates), or the full frame when the prior nearly fills it anyway. */
export function priorSearchWindow(
  prior: Quad,
  fullWidth: number,
  fullHeight: number,
): SearchWindow {
  let minX = Infinity
  let minY = Infinity
  let maxX = -Infinity
  let maxY = -Infinity
  for (const p of prior) {
    minX = Math.min(minX, p.x * fullWidth)
    maxX = Math.max(maxX, p.x * fullWidth)
    minY = Math.min(minY, p.y * fullHeight)
    maxY = Math.max(maxY, p.y * fullHeight)
  }
  const padX = Math.max(ROI_MIN_PAD, ROI_FRAME_PAD * fullWidth, ROI_PRIOR_PAD * (maxX - minX))
  const padY = Math.max(ROI_MIN_PAD, ROI_FRAME_PAD * fullHeight, ROI_PRIOR_PAD * (maxY - minY))
  const x = Math.max(0, Math.floor(minX - padX))
  const y = Math.max(0, Math.floor(minY - padY))
  const right = Math.min(fullWidth, Math.ceil(maxX + padX))
  const bottom = Math.min(fullHeight, Math.ceil(maxY + padY))
  return {
    x,
    y,
    width: Math.max(1, right - x),
    height: Math.max(1, bottom - y),
    fullWidth,
    fullHeight,
  }
}

/** Map a point in crop-pixel coordinates to full-frame normalised (0..1) coordinates. */
export function cropPointToFull(point: Point, window: SearchWindow): Point {
  return {
    x: (window.x + point.x) / window.fullWidth,
    y: (window.y + point.y) / window.fullHeight,
  }
}

/** Map a quad detected in crop pixels to full-frame normalised coordinates. */
export function cropQuadToFull(quad: Quad, window: SearchWindow): Quad {
  return quad.map((p) => cropPointToFull(p, window)) as Quad
}

/** Copy a window's pixels out of a full-frame RGBA image into a standalone crop whose
 * dimensions match the window. Plain typed-array copy — usable in workers/tests where
 * no canvas exists; the live loop instead reads only the window via `getImageData`. */
export function cropImageData(image: ImageData, window: SearchWindow): ImageData {
  const { x, y, width, height } = window
  const out = new Uint8ClampedArray(width * height * 4)
  for (let row = 0; row < height; row++) {
    const srcStart = ((y + row) * image.width + x) * 4
    out.set(image.data.subarray(srcStart, srcStart + width * 4), row * width * 4)
  }
  return { data: out, width, height, colorSpace: 'srgb' } as ImageData
}
