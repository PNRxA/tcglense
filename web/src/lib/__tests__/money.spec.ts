import { describe, it, expect } from 'vitest'

import { formatUsd } from '../money'

describe('formatUsd', () => {
  // The `$` prefix is emitted literally (not via Intl currency, which would render "US$"
  // or "USD" in some locales), so it's always present; only the grouping/decimals are
  // locale-shaped, so the amount is asserted with `toContain`.
  it('formats a decimal string as a $-prefixed, grouped 2-dp amount', () => {
    expect(formatUsd('1234.5')).toBe('$1,234.50')
    expect(formatUsd('9.99')).toBe('$9.99')
    expect(formatUsd('0.01')).toBe('$0.01')
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
