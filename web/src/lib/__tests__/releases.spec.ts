import { describe, expect, it } from 'vitest'
import type { CardSet, ReleaseCalendar, SecretLairDropRelease, SetRelease } from '@/lib/api'
import {
  RELEASE_HEADS_UP_ANCHOR,
  RELEASE_HEADS_UP_PATH,
  calendarMonths,
  calendarWindow,
  entryKindLabel,
  entryName,
  entryPath,
  isUpcoming,
  isoDate,
  monthKeysBetween,
  releasesPath,
  setTypeLabel,
} from '@/lib/releases'

function set(code: string, released_at: string, extra: Partial<SetRelease> = {}): SetRelease {
  return {
    set: { code, name: code.toUpperCase(), released_at } as CardSet,
    released_at,
    secret_lair: false,
    precons: [],
    products: [],
    ...extra,
  }
}

function drop(slug: string, released_at: string): SecretLairDropRelease {
  return { slug, title: `Drop ${slug}`, set_code: 'sld', released_at, products: [] }
}

function calendar(
  from: string,
  to: string,
  sets: SetRelease[],
  secret_lair_drops: SecretLairDropRelease[] = [],
): ReleaseCalendar {
  return { from, to, sets, secret_lair_drops }
}

describe('calendarWindow', () => {
  it('is month-aligned: the first of last month through the end of five months ahead', () => {
    expect(calendarWindow(new Date(2026, 8, 8))).toEqual({ from: '2026-08-01', to: '2027-02-28' })
    // The same window for every day of the month — that is what keeps the URL stable.
    expect(calendarWindow(new Date(2026, 8, 30))).toEqual(calendarWindow(new Date(2026, 8, 1)))
  })

  it('rolls across a year boundary and lands on real month ends', () => {
    expect(calendarWindow(new Date(2026, 0, 15))).toEqual({ from: '2025-12-01', to: '2026-06-30' })
    // A leap February as the last month.
    expect(calendarWindow(new Date(2027, 8, 1)).to).toBe('2028-02-29')
  })

  it('stays inside the API cap of 366 days', () => {
    for (const month of [0, 3, 6, 11]) {
      const { from, to } = calendarWindow(new Date(2026, month, 1))
      const days = (Date.parse(to) - Date.parse(from)) / 86_400_000 + 1
      expect(days).toBeLessThanOrEqual(366)
    }
  })
})

describe('isoDate', () => {
  it('formats a local calendar day without a timezone shift', () => {
    expect(isoDate(new Date(2026, 0, 5))).toBe('2026-01-05')
    expect(isoDate(new Date(2026, 11, 31, 23, 59))).toBe('2026-12-31')
  })
})

describe('monthKeysBetween', () => {
  it('lists every month of the window inclusive', () => {
    expect(monthKeysBetween('2026-11-15', '2027-02-03')).toEqual([
      '2026-11',
      '2026-12',
      '2027-01',
      '2027-02',
    ])
    expect(monthKeysBetween('2026-05-01', '2026-05-31')).toEqual(['2026-05'])
  })

  it('answers nothing for a reversed or unparseable window', () => {
    expect(monthKeysBetween('2026-06-01', '2026-05-01')).toEqual([])
    expect(monthKeysBetween('soon', '2026-05-01')).toEqual([])
  })
})

describe('calendarMonths', () => {
  const today = new Date(2026, 8, 8)

  it('folds sets and drops into every month of the window, date ascending', () => {
    const months = calendarMonths(
      calendar(
        '2026-08-01',
        '2026-10-31',
        [set('oct', '2026-10-09'), set('aug', '2026-08-01')],
        [drop('sept-drop', '2026-09-25')],
      ),
      today,
    )
    expect(months.map((m) => m.key)).toEqual(['2026-08', '2026-09', '2026-10'])
    expect(months.map((m) => m.entries.map((e) => e.key))).toEqual([
      ['set:aug'],
      ['drop:sept-drop'],
      ['set:oct'],
    ])
    expect(months.map((m) => m.current)).toEqual([false, true, false])
    // The label is the viewer's locale; pin the pieces rather than one locale's spelling.
    expect(months[1]?.label).toMatch(/2026/)
  })

  it('keeps an empty month as a section rather than dropping it', () => {
    const months = calendarMonths(calendar('2026-08-01', '2026-09-30', []), today)
    expect(months.map((m) => [m.key, m.entries.length])).toEqual([
      ['2026-08', 0],
      ['2026-09', 0],
    ])
  })

  it('orders a shared day set first, then drops, then by name', () => {
    const months = calendarMonths(
      calendar(
        '2026-09-01',
        '2026-09-30',
        [set('zzz', '2026-09-25'), set('aaa', '2026-09-25')],
        [drop('b', '2026-09-25'), drop('a', '2026-09-25')],
      ),
      today,
    )
    expect(months[0]?.entries.map((e) => e.key)).toEqual(['set:aaa', 'set:zzz', 'drop:a', 'drop:b'])
  })

  it('gives an entry outside the window months a home instead of losing it', () => {
    const months = calendarMonths(
      calendar('2026-09-01', '2026-09-30', [set('late', '2026-11-01')]),
      today,
    )
    expect(months.map((m) => m.key)).toEqual(['2026-09', '2026-11'])
  })
})

describe('entry helpers', () => {
  const today = new Date(2026, 8, 8)
  const setEntry = calendarMonths(
    calendar('2026-09-01', '2026-09-30', [set('blb', '2026-09-08')]),
    today,
  )[0]!.entries[0]!
  const zeta = calendarMonths(
    calendar('2026-09-01', '2026-09-30', [set('slz', '2026-09-02', { secret_lair: true })]),
    today,
  )[0]!.entries[0]!
  const dropEntry = calendarMonths(
    calendar('2026-09-01', '2026-09-30', [], [drop('wild-in-bloom', '2026-09-25')]),
    today,
  )[0]!.entries[0]!

  it('names and links each kind to the page it already has', () => {
    expect(entryName(setEntry)).toBe('BLB')
    expect(entryPath('mtg', setEntry)).toBe('/cards/mtg/sets/blb')
    expect(entryName(dropEntry)).toBe('Drop wild-in-bloom')
    // The Secret Lair set page, filtered to the drop by its title — the by-drop view's own
    // `?drop=` filter — since a drop has no page of its own.
    expect(entryPath('mtg', dropEntry)).toBe('/cards/mtg/sets/sld?drop=Drop+wild-in-bloom')
  })

  it('labels the kind the way the heads-up opt-ins word it', () => {
    expect(entryKindLabel(setEntry)).toBe('Set')
    expect(entryKindLabel(zeta)).toBe('Secret Lair set')
    expect(entryKindLabel(dropEntry)).toBe('Secret Lair drop')
  })

  it('reads release day itself as released, tomorrow as upcoming', () => {
    expect(isUpcoming(setEntry, today)).toBe(false)
    expect(isUpcoming(dropEntry, today)).toBe(true)
  })
})

describe('setTypeLabel', () => {
  it('names the admitted set types and humanises the rest', () => {
    expect(setTypeLabel('expansion')).toBe('Expansion')
    expect(setTypeLabel('draft_innovation')).toBe('Draft innovation')
    expect(setTypeLabel('box')).toBe('Box set')
    expect(setTypeLabel('starter')).toBe('Starter')
    expect(setTypeLabel('duel_deck')).toBe('Duel deck')
    expect(setTypeLabel(null)).toBeNull()
    expect(setTypeLabel('')).toBeNull()
  })
})

describe('paths', () => {
  it('builds the per-game path and the heads-up deep link from one spelling', () => {
    expect(releasesPath('mtg')).toBe('/releases/mtg')
    expect(releasesPath('a b')).toBe('/releases/a%20b')
    expect(RELEASE_HEADS_UP_PATH).toBe(`/alerts#${RELEASE_HEADS_UP_ANCHOR}`)
  })
})
