import { describe, expect, it } from 'vitest'

import {
  holdingDelta,
  holdingDeltaIsFoil,
  holdingDeltaLabel,
  holdingDeltaSummary,
} from '../holdingDelta'

const counts = (quantity: number, foil_quantity: number) => ({ quantity, foil_quantity })

describe('holdingDelta', () => {
  it('subtracts the two absolute holdings field by field', () => {
    expect(holdingDelta(counts(1, 0), counts(1, 1))).toEqual({ quantity: 0, foil_quantity: 1 })
    expect(holdingDelta(counts(4, 2), counts(2, 2))).toEqual({ quantity: -2, foil_quantity: 0 })
    expect(holdingDelta(counts(0, 0), counts(0, 0))).toEqual({ quantity: 0, foil_quantity: 0 })
  })
})

describe('holdingDeltaIsFoil', () => {
  it('is true only when the foil count moved on its own', () => {
    expect(holdingDeltaIsFoil({ quantity: 0, foil_quantity: 1 })).toBe(true)
    expect(holdingDeltaIsFoil({ quantity: 0, foil_quantity: -2 })).toBe(true)
  })

  it('is false when the regular count moved too, or when nothing moved', () => {
    // A mixed change is not "a foil add", so a surface pricing it must not reach for the
    // foil price — the regular copies in the same change would be priced as foils.
    expect(holdingDeltaIsFoil({ quantity: 1, foil_quantity: 1 })).toBe(false)
    expect(holdingDeltaIsFoil({ quantity: 1, foil_quantity: 0 })).toBe(false)
    expect(holdingDeltaIsFoil({ quantity: 0, foil_quantity: 0 })).toBe(false)
  })
})

describe('holdingDeltaLabel', () => {
  it('names the finish that moved, signed', () => {
    expect(holdingDeltaLabel({ quantity: 0, foil_quantity: 1 })).toBe('+1 foil')
    expect(holdingDeltaLabel({ quantity: 2, foil_quantity: 0 })).toBe('+2 regular')
    expect(holdingDeltaLabel({ quantity: -1, foil_quantity: 0 })).toBe('-1 regular')
  })

  it('lists both finishes when both moved, regular first', () => {
    expect(holdingDeltaLabel({ quantity: 1, foil_quantity: 1 })).toBe('+1 regular, +1 foil')
    expect(holdingDeltaLabel({ quantity: -1, foil_quantity: 1 })).toBe('-1 regular, +1 foil')
  })

  it('has no label for a change that changed nothing', () => {
    expect(holdingDeltaLabel({ quantity: 0, foil_quantity: 0 })).toBeNull()
  })
})

describe('holdingDeltaSummary', () => {
  it('reads as a sentence, pluralised per finish', () => {
    expect(holdingDeltaSummary({ quantity: 0, foil_quantity: 1 })).toBe('Adding 1 foil copy')
    expect(holdingDeltaSummary({ quantity: 1, foil_quantity: 0 })).toBe('Adding 1 regular copy')
    expect(holdingDeltaSummary({ quantity: 3, foil_quantity: 0 })).toBe('Adding 3 regular copies')
    expect(holdingDeltaSummary({ quantity: 1, foil_quantity: 2 })).toBe(
      'Adding 1 regular copy and 2 foil copies',
    )
  })

  it('says removing when a count goes down', () => {
    expect(holdingDeltaSummary({ quantity: -2, foil_quantity: 0 })).toBe(
      'Removing 2 regular copies',
    )
    expect(holdingDeltaSummary({ quantity: 0, foil_quantity: -1 })).toBe('Removing 1 foil copy')
  })

  it('spells out a finish correction as both clauses', () => {
    // Fixing a misdetected foil star moves the scanned copy across, and the panel has to say
    // so — "Adding 1 foil copy" alone would hide that the regular count is going back down.
    expect(holdingDeltaSummary({ quantity: -1, foil_quantity: 1 })).toBe(
      'Adding 1 foil copy, removing 1 regular copy',
    )
  })

  it('has no sentence for a change that changed nothing', () => {
    expect(holdingDeltaSummary({ quantity: 0, foil_quantity: 0 })).toBeNull()
  })
})
