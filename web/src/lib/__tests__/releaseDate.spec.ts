import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { formatReleaseLabel } from '../releaseDate'

describe('formatReleaseLabel', () => {
  beforeEach(() => {
    // Pin "now" so future/past is deterministic.
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-07-20T12:00:00Z'))
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it('reads "Releases …" for a future date and "Released …" for a past one', () => {
    const future = formatReleaseLabel('2026-08-01')
    expect(future?.upcoming).toBe(true)
    expect(future?.label.startsWith('Releases ')).toBe(true)

    const past = formatReleaseLabel('2020-01-01')
    expect(past?.upcoming).toBe(false)
    expect(past?.label.startsWith('Released ')).toBe(true)
  })

  it('honours the month style option', () => {
    expect(formatReleaseLabel('2026-08-01', 'long')?.label).toContain('August')
    expect(formatReleaseLabel('2026-08-01', 'short')?.label).toContain('Aug')
  })

  it('returns null for a missing or unparseable date', () => {
    expect(formatReleaseLabel(null)).toBeNull()
    expect(formatReleaseLabel(undefined)).toBeNull()
    expect(formatReleaseLabel('')).toBeNull()
    expect(formatReleaseLabel('not-a-date')).toBeNull()
  })
})
