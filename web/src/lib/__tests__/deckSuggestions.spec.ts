import { describe, expect, it } from 'vitest'
import type { DeckSuggestions } from '@/lib/api'
import {
  colourFilterLabel,
  formatFilterLabel,
  rankLabel,
  suggestionsSummary,
} from '@/lib/deckSuggestions'

function base(over: Partial<DeckSuggestions> = {}): DeckSuggestions {
  return {
    format_key: 'commander',
    format_label: 'Commander',
    color_identity: ['W', 'U'],
    commanders: [{ card_id: 'a', name: 'Aminatou' }],
    candidate_count: 14,
    scanned_count: 14,
    top: [],
    roles: [],
    unclassified_count: 0,
    caveats: ['Ranked by global popularity.'],
    ...over,
  }
}

describe('deckSuggestions wording', () => {
  it('names whose colours the filter is, or the deck when it is a union', () => {
    expect(colourFilterLabel(base())).toBe("in Aminatou's colours (WU)")
    expect(
      colourFilterLabel(
        base({
          commanders: [
            { card_id: 'a', name: 'Rograkh' },
            { card_id: 'b', name: 'Silas Renn' },
          ],
        }),
      ),
    ).toBe("in Rograkh + Silas Renn's colours (WU)")
    expect(colourFilterLabel(base({ commanders: [] }))).toBe("in the deck's colours (WU)")
  })

  it('says when no colour filter applied, and that colourless is a filter', () => {
    expect(colourFilterLabel(base({ color_identity: null, commanders: [] }))).toContain(
      'any colour',
    )
    expect(colourFilterLabel(base({ color_identity: [], commanders: [] }))).toBe(
      "in the deck's colours (colourless)",
    )
  })

  it('names the format, or says none was applied', () => {
    expect(formatFilterLabel(base())).toBe('legal in Commander')
    expect(formatFilterLabel(base({ format_key: null, format_label: null }))).toContain(
      'any format',
    )
  })

  it('summarises the count and both filters in one line', () => {
    expect(suggestionsSummary(base())).toBe(
      "14 cards you own fit · in Aminatou's colours (WU) · legal in Commander",
    )
    expect(suggestionsSummary(base({ candidate_count: 1 }))).toMatch(/^1 card you own fits/)
    expect(suggestionsSummary(base({ candidate_count: 0 }))).toMatch(/^0 cards you own fit/)
  })

  it('prints a rank as an ordinal-style hash', () => {
    expect(rankLabel({ edhrec_rank: 42 })).toBe('#42')
  })
})
