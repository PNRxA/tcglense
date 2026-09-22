import { describe, expect, it } from 'vitest'
import { formatUsd } from '../money'
import {
  CHANGE_WINDOW_OPTIONS,
  DEFAULT_CHANGE_WINDOW,
  changeDirection,
  changeWindowLabel,
  changeWindowSentence,
  describeValueChange,
  formatAsOfDate,
  formatSignedMoney,
  formatSignedPct,
  isChangeWindow,
} from '../valueChange'
import type { ValueChange } from '@/lib/api'

// Mirrors lib/money's formatUsd so the expected strings track the host locale (a hard-coded
// '$1.00' only passes where toLocaleString resolves to a US-style locale).
function usd(amount: number) {
  return `$${amount.toLocaleString(undefined, {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  })}`
}

function change(overrides: Partial<ValueChange> = {}): ValueChange {
  return {
    as_of: '2026-09-22',
    value_usd: '128.50',
    previous_usd: '125.00',
    change_usd: '3.50',
    change_pct: 2.8,
    ...overrides,
  }
}

describe('changeDirection', () => {
  it('reads the sign of a signed decimal string or number', () => {
    expect(changeDirection('3.50')).toBe('up')
    expect(changeDirection('-0.01')).toBe('down')
    expect(changeDirection(-2)).toBe('down')
    expect(changeDirection(12)).toBe('up')
  })

  it('treats zero and garbage as flat', () => {
    expect(changeDirection('0.00')).toBe('flat')
    expect(changeDirection(0)).toBe('flat')
    expect(changeDirection('n/a')).toBe('flat')
  })
})

describe('formatSignedMoney', () => {
  it('formats the magnitude and re-applies the sign as + or a real minus', () => {
    expect(formatSignedMoney('3.50', formatUsd)).toBe(`+${usd(3.5)}`)
    // U+2212, not a hyphen, so the glyph width matches the plus.
    expect(formatSignedMoney('-1234.5', formatUsd)).toBe(`−${usd(1234.5)}`)
    expect(formatSignedMoney(-0.5, formatUsd)).toBe(`−${usd(0.5)}`)
  })

  it('leaves a zero unsigned', () => {
    expect(formatSignedMoney('0.00', formatUsd)).toBe(usd(0))
  })

  it('never hands the formatter a negative string', () => {
    // formatUsd would otherwise render "$-3.50"; the seam exists to prevent exactly that.
    const seen: (string | null | undefined)[] = []
    formatSignedMoney('-3.50', (raw) => {
      seen.push(raw)
      return formatUsd(raw)
    })
    expect(seen).toEqual(['3.50'])
  })

  it('falls back to the raw string when it is not a number', () => {
    expect(formatSignedMoney('oops', formatUsd)).toBe('oops')
    expect(formatSignedMoney(Number.NaN, formatUsd)).toBeNull()
  })
})

describe('formatSignedPct', () => {
  it('rounds to one decimal with a sign', () => {
    expect(formatSignedPct(2.8)).toBe('+2.8%')
    expect(formatSignedPct(-0.55)).toBe('−0.6%')
    expect(formatSignedPct(12.04)).toBe('+12.0%')
  })

  it('shows a rounded-to-zero movement unsigned', () => {
    expect(formatSignedPct(0)).toBe('0.0%')
    expect(formatSignedPct(-0.02)).toBe('0.0%')
  })

  it('is null when the API had no percentage', () => {
    expect(formatSignedPct(null)).toBeNull()
    expect(formatSignedPct(undefined)).toBeNull()
    expect(formatSignedPct(Number.POSITIVE_INFINITY)).toBeNull()
  })
})

describe('describeValueChange', () => {
  it('shapes a gain with its percentage and reference date', () => {
    expect(describeValueChange(change(), formatUsd, 'week')).toEqual({
      text: `+${usd(3.5)}`,
      pctText: '+2.8%',
      direction: 'up',
      window: 'week',
      asOf: '2026-09-22',
    })
  })

  it('shapes a loss and a flat day', () => {
    expect(
      describeValueChange(change({ change_usd: '-10.25', change_pct: -7.6 }), formatUsd, 'day'),
    ).toEqual({
      text: `−${usd(10.25)}`,
      pctText: '−7.6%',
      direction: 'down',
      window: 'day',
      asOf: '2026-09-22',
    })
    expect(
      describeValueChange(change({ change_usd: '0.00', change_pct: 0 }), formatUsd, 'all_time'),
    ).toEqual({
      text: usd(0),
      pctText: '0.0%',
      direction: 'flat',
      window: 'all_time',
      asOf: '2026-09-22',
    })
  })

  it('omits the percentage chip when the baseline was zero', () => {
    const described = describeValueChange(change({ change_pct: null }), formatUsd, 'week')
    expect(described?.pctText).toBeNull()
    expect(described?.text).toBe(`+${usd(3.5)}`)
  })

  it('is null when there is nothing to show', () => {
    // No response yet / nothing held.
    expect(describeValueChange(undefined, formatUsd, 'week')).toBeNull()
    expect(describeValueChange(null, formatUsd, 'week')).toBeNull()
    // A value but no baseline capture yet (a single captured day): the stat shows its value
    // alone rather than a fabricated zero.
    expect(
      describeValueChange(
        change({ change_usd: null, previous_usd: null, change_pct: null }),
        formatUsd,
        'week',
      ),
    ).toBeNull()
    // Nothing priced at all.
    expect(
      describeValueChange(
        {
          as_of: null,
          value_usd: null,
          previous_usd: null,
          change_usd: null,
          change_pct: null,
        },
        formatUsd,
        'week',
      ),
    ).toBeNull()
  })
})

describe('change windows', () => {
  it('offers the seven windows in picker order with the movers panel labels', () => {
    expect(CHANGE_WINDOW_OPTIONS.map((opt) => opt.value)).toEqual([
      'day',
      'week',
      'month',
      'year',
      'two_year',
      'three_year',
      'all_time',
    ])
    expect(CHANGE_WINDOW_OPTIONS.map((opt) => opt.label)).toEqual([
      '1D',
      '7D',
      '30D',
      '1Y',
      '2Y',
      '3Y',
      'All',
    ])
    expect(DEFAULT_CHANGE_WINDOW).toBe('week')
  })

  it('validates stored tokens and labels each window', () => {
    expect(isChangeWindow('week')).toBe(true)
    expect(isChangeWindow('all_time')).toBe(true)
    expect(isChangeWindow('7d')).toBe(false)
    expect(isChangeWindow(null)).toBe(false)
    expect(changeWindowLabel('week')).toBe('7D')
    expect(changeWindowLabel('all_time')).toBe('All')
  })

  it('reads each window as a sentence clause', () => {
    expect(changeWindowSentence('day')).toBe("since the previous day's captured prices")
    expect(changeWindowSentence('week')).toBe('over the last 7 days')
    expect(changeWindowSentence('month')).toBe('over the last 30 days')
    expect(changeWindowSentence('year')).toBe('over the last year')
    expect(changeWindowSentence('two_year')).toBe('over the last 2 years')
    expect(changeWindowSentence('three_year')).toBe('over the last 3 years')
    expect(changeWindowSentence('all_time')).toBe('since the earliest captured prices')
  })
})

describe('formatAsOfDate', () => {
  it('renders a short month + day, or the raw string when unparseable', () => {
    const rendered = formatAsOfDate('2026-09-22')
    expect(rendered).toContain('22')
    expect(rendered).not.toBe('2026-09-22')
    expect(formatAsOfDate('not-a-date')).toBe('not-a-date')
  })
})
