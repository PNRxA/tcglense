import { describe, expect, it } from 'vitest'
import { createCardLock } from '../cardLock'
import type { Quad } from '../detect'

function quadAt(x: number, y: number, w = 0.4, h = 0.56): Quad {
  return [
    { x, y },
    { x: x + w, y },
    { x: x + w, y: y + h },
    { x, y: y + h },
  ]
}

describe('createCardLock', () => {
  it('never displays a single isolated detection', () => {
    const lock = createCardLock()
    expect(lock.update(quadAt(0.2, 0.15), 0)).toBeNull()
    // The tentative candidate is still offered as the next tick's search prior.
    expect(lock.prior()).toEqual(quadAt(0.2, 0.15))
  })

  it('locks on the consistent second detection, blended from the tentative quad', () => {
    const lock = createCardLock()
    lock.update(quadAt(0.2, 0.15), 0)
    const confirmed = lock.update(quadAt(0.22, 0.15), 120)
    expect(confirmed).not.toBeNull()
    // Default tracker blend 0.5: the displayed outline starts at the midpoint.
    expect(confirmed![0]!.x).toBeCloseTo(0.21, 10)
    expect(lock.prior()).toEqual(confirmed)
  })

  it('tolerates one intervening miss during confirmation', () => {
    const lock = createCardLock()
    lock.update(quadAt(0.2, 0.15), 0)
    expect(lock.update(null, 120)).toBeNull()
    expect(lock.update(quadAt(0.2, 0.15), 240)).not.toBeNull()
  })

  it('drops the tentative candidate after two consecutive misses', () => {
    const lock = createCardLock()
    lock.update(quadAt(0.2, 0.15), 0)
    lock.update(null, 120)
    lock.update(null, 240)
    expect(lock.prior()).toBeNull()
    // The next detection starts a fresh acquisition, not a confirmation.
    expect(lock.update(quadAt(0.2, 0.15), 360)).toBeNull()
  })

  it('expires a stale tentative candidate instead of confirming against it', () => {
    const lock = createCardLock()
    lock.update(quadAt(0.2, 0.15), 0)
    // Same geometry but far beyond the confirmation window: restart, no lock.
    expect(lock.update(quadAt(0.2, 0.15), 1000)).toBeNull()
    // …and the restarted acquisition confirms normally afterwards.
    expect(lock.update(quadAt(0.2, 0.15), 1120)).not.toBeNull()
  })

  it('restarts acquisition at a new position instead of confirming across a jump', () => {
    const lock = createCardLock()
    lock.update(quadAt(0.1, 0.1), 0)
    // A distant candidate is a different object: it must not confirm the first.
    expect(lock.update(quadAt(0.5, 0.4), 120)).toBeNull()
    // But it became the new tentative candidate, so a consistent follow-up locks.
    expect(lock.update(quadAt(0.5, 0.4), 240)).not.toBeNull()
  })

  it('holds an established lock through three misses, then clears prior and display', () => {
    const lock = createCardLock()
    lock.update(quadAt(0.2, 0.15), 0)
    const displayed = lock.update(quadAt(0.2, 0.15), 120)
    for (let miss = 0; miss < 3; miss++) {
      expect(lock.update(null, 240 + miss * 120)).toEqual(displayed)
      expect(lock.prior()).toEqual(displayed)
    }
    expect(lock.update(null, 720)).toBeNull()
    expect(lock.prior()).toBeNull()
  })

  it('reacquires cleanly after a lost track, even near the old position', () => {
    const lock = createCardLock()
    lock.update(quadAt(0.2, 0.2, 0.4, 0.56), 0)
    lock.update(quadAt(0.2, 0.2, 0.4, 0.56), 120)
    // Lose the track: three held misses, then the clearing fourth.
    for (let miss = 0; miss < 4; miss++) lock.update(null, 240 + miss * 120)
    expect(lock.prior()).toBeNull()
    // The same card reappears under a perspective change: three corners near the old
    // quad, one corner ~0.09 away. The wrapped tracker's retained memory of the LOST
    // card must not veto this genuinely confirmed new acquisition.
    const shifted: Quad = [
      { x: 0.2, y: 0.2 },
      { x: 0.6, y: 0.2 },
      { x: 0.69, y: 0.76 },
      { x: 0.2, y: 0.76 },
    ]
    expect(lock.update(shifted, 840)).toBeNull()
    expect(lock.update(shifted, 960)).toEqual(shifted)
  })

  it('reset() clears tentative state, the track, and the prior', () => {
    const lock = createCardLock()
    lock.update(quadAt(0.2, 0.15), 0)
    lock.update(quadAt(0.2, 0.15), 120)
    lock.reset()
    expect(lock.prior()).toBeNull()
    expect(lock.update(quadAt(0.2, 0.15), 240)).toBeNull()
  })
})
