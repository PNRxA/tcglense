// Temporal stabiliser for the live card outline. The per-tick detector is memoryless:
// raw per-frame corner noise makes the outline wobble, and a single missed frame (motion
// blur, a glare flash) blanks it — so it never *looks* snapped to the card even when
// detection is good. This tracker smooths consecutive detections of the same card with
// an exponential moving average, snaps instantly to a genuinely new position, and holds
// the last quad across a few missed ticks before letting go. Pure and unit-tested; the
// camera loop in `useCardScanner` feeds it one update per detection tick.

import type { Quad } from './detect'

export interface QuadTrackerOptions {
  /** Mean per-corner distance (normalised 0..1 frame units) below which a detection is
   * treated as the same card and smoothed into the track; above it the track snaps. */
  snapDistance?: number
  /** Weight of the new detection when smoothing (0..1) — higher tracks faster, lower
   * is steadier. */
  blend?: number
  /** Consecutive missed ticks the last quad is held for before the track clears —
   * bridges transient detection dropouts without ghosting a card that's gone. */
  holdTicks?: number
}

export interface QuadTracker {
  /** Feed one detection tick (the detected quad, or null on a miss); returns the
   * stabilised quad to display, or null once the track is lost. */
  update: (detected: Quad | null) => Quad | null
  /** Forget the current track (camera stopped / switched). */
  reset: () => void
}

/** Mean Euclidean distance between corresponding corners of two ordered quads. */
export function meanCornerDistance(a: Quad, b: Quad): number {
  let sum = 0
  for (let i = 0; i < 4; i++) {
    sum += Math.hypot(a[i]!.x - b[i]!.x, a[i]!.y - b[i]!.y)
  }
  return sum / 4
}

/** Per-corner linear blend of two ordered quads: `prev * (1 - t) + next * t`. */
export function blendQuads(prev: Quad, next: Quad, t: number): Quad {
  return prev.map((p, i) => ({
    x: p.x + (next[i]!.x - p.x) * t,
    y: p.y + (next[i]!.y - p.y) * t,
  })) as Quad
}

export function createQuadTracker(options: QuadTrackerOptions = {}): QuadTracker {
  const snapDistance = options.snapDistance ?? 0.05
  const blend = options.blend ?? 0.5
  const holdTicks = options.holdTicks ?? 3
  let current: Quad | null = null
  let misses = 0

  return {
    update(detected) {
      if (!detected) {
        if (current && misses < holdTicks) {
          misses++
          return current
        }
        current = null
        return null
      }
      const prev = current
      misses = 0
      current =
        prev && meanCornerDistance(prev, detected) <= snapDistance
          ? blendQuads(prev, detected, blend)
          : detected
      return current
    },
    reset() {
      current = null
      misses = 0
    },
  }
}
