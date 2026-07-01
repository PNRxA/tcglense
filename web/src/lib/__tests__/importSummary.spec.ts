import { describe, expect, it } from 'vitest'
import { formatImportSummaryLines } from '@/lib/importSummary'
import type { ImportSummary } from '@/lib/api'

function summary(overrides: Partial<ImportSummary> = {}): ImportSummary {
  return {
    provider: 'archidekt',
    mode: 'overwrite',
    total_rows: 0,
    distinct_cards: 0,
    matched_cards: 0,
    unmatched_cards: 0,
    unmatched_sample: [],
    regular_copies: 0,
    foil_copies: 0,
    removed_cards: 0,
    stopped_early: false,
    ...overrides,
  }
}

describe('formatImportSummaryLines', () => {
  it('leads with matched cards + total copies, pluralized', () => {
    const [lead] = formatImportSummaryLines(
      summary({ matched_cards: 3, regular_copies: 4, foil_copies: 1 }),
    )
    expect(lead).toBe('Imported 3 cards (5 copies).')
  })

  it('uses singular card/copy at a count of one', () => {
    const [lead] = formatImportSummaryLines(
      summary({ matched_cards: 1, regular_copies: 1, foil_copies: 0 }),
    )
    expect(lead).toBe('Imported 1 card (1 copy).')
  })

  it('says "Updated" and reports the stop for smart mode', () => {
    const lines = formatImportSummaryLines(
      summary({ mode: 'smart', matched_cards: 2, regular_copies: 2, stopped_early: true }),
    )
    expect(lines[0]).toBe('Updated 2 cards (2 copies).')
    expect(lines[1]).toBe('Smart sync stopped once it reached cards already in sync.')
  })

  it('appends unmatched and removed lines only when non-zero', () => {
    const lines = formatImportSummaryLines(
      summary({ matched_cards: 5, regular_copies: 5, unmatched_cards: 2, removed_cards: 1 }),
    )
    expect(lines).toHaveLength(3)
    expect(lines[1]).toContain('2 cards')
    expect(lines[1]).toContain('skipped')
    expect(lines[2]).toContain('1 card ')
    expect(lines[2]).toContain('mirror the list')
  })

  it('localizes large counts', () => {
    const [lead] = formatImportSummaryLines(
      summary({ matched_cards: 1234, regular_copies: 2000, foil_copies: 500 }),
    )
    expect(lead).toBe('Imported 1,234 cards (2,500 copies).')
  })
})
