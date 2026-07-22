// Two-hit acquisition wrapper around the outline tracker. The per-tick detector can
// produce a plausible-looking quad from one noisy frame of a busy scene; showing that
// instantly as a green lock invites a wrong capture. So a brand-new detection is held
// TENTATIVE (nothing displayed) until a second, consistent detection confirms it within
// a short window — one extra 120 ms tick on ordinary scenes, a real false-lock filter
// on cluttered ones. Once confirmed, the existing quadTracker owns the lock unchanged
// (EMA smoothing, snap, three-tick hold); this wrapper never re-implements it.
//
// The wrapper is also the owner of the scanner's spatial prior: the quad the live loop
// should search around next tick. That is the tentative candidate while acquiring, the
// displayed/held quad while locked, and null otherwise — never the tracker's internal
// long-lived state, which deliberately survives display expiry and would go stale as a
// search hint. Pure and unit-tested; the caller supplies timestamps.

import { quadArea, type Quad } from './detect'
import { cornerMetrics, createQuadTracker, type QuadTrackerOptions } from './quadTracker'

export interface CardLockOptions {
  /** How long (ms) a tentative candidate may wait for its confirming detection. */
  confirmMaxAgeMs?: number
  /** Consecutive missed ticks a tentative candidate survives. */
  confirmMaxMisses?: number
  /** Confirmation gates: the second detection must plausibly be the same card. */
  confirmMeanDistance?: number
  confirmMaxCornerDistance?: number
  confirmAreaRatio?: readonly [number, number]
  /** Passed through to the wrapped {@link createQuadTracker}. */
  tracker?: QuadTrackerOptions
}

export interface CardLock {
  /** Feed one tick's (mode-selected) detection; returns the quad to display, or null
   * while acquiring / once the track is lost. */
  update: (detected: Quad | null, nowMs: number) => Quad | null
  /** The quad the next detection tick should search around, or null for a cold
   * full-frame acquisition search. */
  prior: () => Quad | null
  /** Forget everything (camera stopped / switched, capture invalidated the lock). */
  reset: () => void
}

export function createCardLock(options: CardLockOptions = {}): CardLock {
  const confirmMaxAgeMs = options.confirmMaxAgeMs ?? 400
  const confirmMaxMisses = options.confirmMaxMisses ?? 1
  const confirmMeanDistance = options.confirmMeanDistance ?? 0.06
  const confirmMaxCornerDistance = options.confirmMaxCornerDistance ?? 0.09
  const confirmAreaRatio = options.confirmAreaRatio ?? [0.88, 1.15]
  const tracker = createQuadTracker(options.tracker)

  let tentative: { quad: Quad; sinceMs: number; misses: number } | null = null
  let displayed: Quad | null = null
  let locked = false

  const confirms = (candidate: Quad, reference: Quad): boolean => {
    const metrics = cornerMetrics(reference, candidate)
    if (metrics.mean > confirmMeanDistance) return false
    if (metrics.max > confirmMaxCornerDistance) return false
    const referenceArea = quadArea(reference)
    if (referenceArea <= 0) return false
    const ratio = quadArea(candidate) / referenceArea
    return ratio >= confirmAreaRatio[0] && ratio <= confirmAreaRatio[1]
  }

  return {
    update(detected, nowMs) {
      if (locked) {
        displayed = tracker.update(detected)
        if (!displayed) {
          // Track lost: cold acquisition next — a stale prior must not linger.
          locked = false
          tentative = null
        }
        return displayed
      }

      if (!detected) {
        if (tentative && ++tentative.misses > confirmMaxMisses) tentative = null
        return null
      }

      if (tentative && nowMs - tentative.sinceMs <= confirmMaxAgeMs) {
        if (confirms(detected, tentative.quad)) {
          locked = true
          // Seed the tracker with the tentative quad first so the confirming
          // detection blends from it — the outline appears already settled.
          tracker.update(tentative.quad)
          tentative = null
          displayed = tracker.update(detected)
          if (!displayed) locked = false
          return displayed
        }
      }
      // No tentative, an expired one, or a non-matching candidate: (re)start
      // acquisition at the new position.
      tentative = { quad: detected, sinceMs: nowMs, misses: 0 }
      return null
    },
    prior() {
      if (locked) return displayed
      return tentative?.quad ?? null
    },
    reset() {
      tracker.reset()
      tentative = null
      displayed = null
      locked = false
    },
  }
}
