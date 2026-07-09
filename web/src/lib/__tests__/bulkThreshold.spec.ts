import { describe, expect, it } from 'vitest'
import {
  DEFAULT_BULK_THRESHOLD_CENTS,
  MAX_BULK_THRESHOLD_CENTS,
  MIN_BULK_THRESHOLD_CENTS,
  centsToDollars,
  clampBulkThresholdCents,
  dollarsToCents,
} from '../bulkThreshold'

describe('bulk threshold helpers', () => {
  it('clamps to whole cents within range', () => {
    expect(clampBulkThresholdCents(250)).toBe(250)
    expect(clampBulkThresholdCents(250.6)).toBe(251) // rounds to a whole cent
    expect(clampBulkThresholdCents(-5)).toBe(MIN_BULK_THRESHOLD_CENTS)
    expect(clampBulkThresholdCents(9_999_999)).toBe(MAX_BULK_THRESHOLD_CENTS)
  })

  it('falls back to the default for a non-finite value', () => {
    expect(clampBulkThresholdCents(Number.NaN)).toBe(DEFAULT_BULK_THRESHOLD_CENTS)
    expect(clampBulkThresholdCents(Number.POSITIVE_INFINITY)).toBe(DEFAULT_BULK_THRESHOLD_CENTS)
  })

  it('converts between dollars and cents', () => {
    expect(centsToDollars(100)).toBe(1)
    expect(centsToDollars(250)).toBe(2.5)
    expect(dollarsToCents(1)).toBe(100)
    expect(dollarsToCents(2.5)).toBe(250)
    expect(dollarsToCents(0.999)).toBe(100) // rounds sub-cent up to a whole cent ($1.00)
  })

  it('clamps a dollar amount past the range back into cents', () => {
    expect(dollarsToCents(-1)).toBe(MIN_BULK_THRESHOLD_CENTS)
    expect(dollarsToCents(1_000_000)).toBe(MAX_BULK_THRESHOLD_CENTS)
    expect(dollarsToCents(Number.NaN)).toBe(DEFAULT_BULK_THRESHOLD_CENTS)
  })

  it('resolves a cleared field (null) to the default, not $0', () => {
    // A number field emits `null` when cleared; `null * 100` would coerce to 0 and read
    // as "nothing is bulk" without the finite guard.
    expect(dollarsToCents(null as unknown as number)).toBe(DEFAULT_BULK_THRESHOLD_CENTS)
    expect(dollarsToCents(undefined as unknown as number)).toBe(DEFAULT_BULK_THRESHOLD_CENTS)
  })
})
