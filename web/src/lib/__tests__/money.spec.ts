import { describe, it, expect } from 'vitest'

import { formatUsd } from '../money'

describe('formatUsd', () => {
  // The exact currency symbol/placement is locale-dependent (the test runtime may render
  // USD as "$1,234.50" or "USD 1,234.50"), so assert the amount is present + grouped rather
  // than pinning the symbol.
  it('formats a decimal string as a grouped 2-dp amount', () => {
    expect(formatUsd('1234.5')).toContain('1,234.50')
    expect(formatUsd('9.99')).toContain('9.99')
    expect(formatUsd('0.01')).toContain('0.01')
  })

  it('returns null when the API sends no value', () => {
    expect(formatUsd(null)).toBeNull()
    expect(formatUsd(undefined)).toBeNull()
    expect(formatUsd('')).toBeNull()
  })

  it('falls back to a bare $-prefixed string for a non-numeric value', () => {
    // This branch is our own formatting, so it's locale-independent.
    expect(formatUsd('not-a-number')).toBe('$not-a-number')
  })
})
