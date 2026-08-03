import { describe, it, expect } from 'vitest'
import type { Card } from '@/lib/api'
import { matchPrinting } from '../match'

function print(
  id: string,
  overrides: Partial<Card> & Pick<Card, 'set_code' | 'collector_number'>,
): Card {
  return {
    id,
    name: 'Lightning Bolt',
    set_name: 'Set',
    rarity: 'common',
    lang: 'en',
    released_at: '2024-01-01',
    mana_cost: null,
    cmc: null,
    type_line: null,
    oracle_text: null,
    power: null,
    toughness: null,
    loyalty: null,
    color_identity: [],
    colors: [],
    layout: 'normal',
    prices: { usd: null, usd_foil: null, eur: null, tix: null },
    has_image: false,
    drop_name: null,
    drop_slug: null,
    secret_lair_bonus: false,
    secret_lair_spend_incentive: false,
    faces: [],
    legalities: null,
    ...overrides,
  }
}

// Newest-first, mirroring getCardPrintingsByName's ordering.
const prints: Card[] = [
  print('a', { set_code: 'clu', collector_number: '141' }),
  print('b', { set_code: 'neo', collector_number: '133' }),
  print('c', { set_code: 'mh2', collector_number: '0123' }),
  print('d', { set_code: 'neo', collector_number: '412' }),
]

describe('matchPrinting', () => {
  it('returns null for an empty printing list or an empty hint', () => {
    expect(matchPrinting([], { setCode: 'neo' })).toBeNull()
    expect(matchPrinting(prints, {})).toBeNull()
  })

  it('matches set code + collector number exactly, case-insensitively', () => {
    expect(matchPrinting(prints, { setCode: 'NEO', collectorNumber: '133' })?.id).toBe('b')
  })

  it('ignores zero-padding differences in the collector number', () => {
    expect(matchPrinting(prints, { setCode: 'mh2', collectorNumber: '123' })?.id).toBe('c')
  })

  it('falls back to the newest printing in a set when only the set code is known', () => {
    // 'b' precedes 'd' in the newest-first list, so it wins for set neo.
    expect(matchPrinting(prints, { setCode: 'neo' })?.id).toBe('b')
  })

  it('returns null for a collector number with no set (too ambiguous)', () => {
    expect(matchPrinting(prints, { collectorNumber: '133' })).toBeNull()
  })

  it('falls back to the newest printing in the set when the exact number is not found', () => {
    // The set code read cleanly but the number didn't — still better than ignoring the set.
    expect(matchPrinting(prints, { setCode: 'neo', collectorNumber: '999' })?.id).toBe('b')
  })

  it('returns null when the set code matches nothing (fall back to the caller default)', () => {
    expect(matchPrinting(prints, { setCode: 'zzz', collectorNumber: '133' })).toBeNull()
  })

  it('rescues a set code that is one glyph off, keyed to the collector number', () => {
    // OCR read NE0 (zero) for NEO — one confusable glyph, so it still finds neo #133.
    expect(matchPrinting(prints, { setCode: 'NE0', collectorNumber: '133' })?.id).toBe('b')
  })

  it('rescues a one-glyph set code to the newest printing when the number is unreadable', () => {
    expect(matchPrinting(prints, { setCode: 'ne0' })?.id).toBe('b')
  })

  it('does not rescue a set code more than one glyph off', () => {
    expect(matchPrinting(prints, { setCode: 'nxx', collectorNumber: '133' })).toBeNull()
  })

  it('refuses to guess when two of the card’s set codes are equally close', () => {
    const ambiguous: Card[] = [
      print('x', { set_code: 'aaa', collector_number: '1' }),
      print('y', { set_code: 'aab', collector_number: '1' }),
    ]
    // 'aac' is one glyph from both aaa and aab — too ambiguous to auto-pick.
    expect(matchPrinting(ambiguous, { setCode: 'aac' })).toBeNull()
  })

  it('prefers an exact set code over a one-glyph neighbour', () => {
    const both: Card[] = [
      print('near', { set_code: 'net', collector_number: '5' }),
      print('exact', { set_code: 'neo', collector_number: '5' }),
    ]
    expect(matchPrinting(both, { setCode: 'neo' })?.id).toBe('exact')
  })
})

// One set, three treatments of the same card — the shape that made the scanner open on a
// wildly different artwork. They share a release date and a name, so the printings listing
// tiebreaks on the row id: its order carries no information about which one was scanned.
const treatments: Card[] = [
  print('fullart', { set_code: 'tla', collector_number: '312' }),
  print('borderless', { set_code: 'tla', collector_number: '288' }),
  print('normal', { set_code: 'tla', collector_number: '41' }),
]

describe('matchPrinting with a visual ranking', () => {
  it('picks the visually closest treatment over the listing order for a set-only hint', () => {
    // The set code is right, but it cannot say *which* artwork — the fingerprint can.
    const picked = matchPrinting(treatments, { setCode: 'TLA' }, [
      { id: 'normal', distance: 14 },
      { id: 'borderless', distance: 78 },
      { id: 'fullart', distance: 91 },
    ])
    expect(picked?.id).toBe('normal')
  })

  it('picks the visually closest printing when the set line was unreadable', () => {
    // Previously null (the caller then took prints[0] — here the full-art card).
    const picked = matchPrinting(treatments, {}, [
      { id: 'normal', distance: 12 },
      { id: 'fullart', distance: 88 },
    ])
    expect(picked?.id).toBe('normal')
  })

  it('lets the set code choose among printings the fingerprint cannot separate', () => {
    // Same artwork reprinted into another set: near-identical distances, so the OCR'd set
    // line — the only signal that can tell them apart — decides.
    const reprints: Card[] = [
      print('reprint', { set_code: 'clu', collector_number: '141' }),
      print('original', { set_code: 'neo', collector_number: '133' }),
    ]
    const picked = matchPrinting(reprints, { setCode: 'NEO' }, [
      { id: 'reprint', distance: 17 },
      { id: 'original', distance: 21 },
    ])
    expect(picked?.id).toBe('original')
  })

  it('does not let a set code promote a printing the fingerprint clearly ranked worse', () => {
    // A misread set code (or a set holding an unrelated artwork) must not beat the visual
    // verdict — that is the bug this ordering exists to prevent.
    const picked = matchPrinting(
      [...treatments, print('other-set', { set_code: 'clu', collector_number: '9' })],
      { setCode: 'CLU' },
      [
        { id: 'normal', distance: 10 },
        { id: 'other-set', distance: 84 },
      ],
    )
    expect(picked?.id).toBe('normal')
  })

  it('keeps a set code whose printings the scan never ranked', () => {
    // No fingerprint for that printing (an index built before the reprint shipped) is no
    // evidence against it, so the direct read of the card still stands.
    const picked = matchPrinting(prints, { setCode: 'CLU' }, [{ id: 'b', distance: 11 }])
    expect(picked?.id).toBe('a')
  })

  it('still honours an exact set + collector number over the ranking', () => {
    // The collector number is the one OCR signal that separates a set's own treatments, so
    // a clean read of it outranks a fingerprint that merely ranked a sibling first.
    const picked = matchPrinting(treatments, { setCode: 'TLA', collectorNumber: '288' }, [
      { id: 'normal', distance: 18 },
      { id: 'borderless', distance: 24 },
    ])
    expect(picked?.id).toBe('borderless')
  })

  it('keeps the set code when it rejects the collector number', () => {
    // The number and the set code are separate tokens on the strip, and the number is the
    // flakier read. Misread digits that happen to land on a real printing must not cost the
    // set code too — otherwise a same-art reprint from another set wins on a 2-bit lead.
    const reprint = print('clu-reprint', { set_code: 'clu', collector_number: '141' })
    const picked = matchPrinting(
      [...treatments, reprint],
      { setCode: 'TLA', collectorNumber: '312' },
      [
        { id: 'clu-reprint', distance: 10 },
        { id: 'normal', distance: 12 },
        { id: 'fullart', distance: 87 },
      ],
    )
    // #312 is real but visually hopeless, so it's dropped — and the pick stays in TLA.
    expect(picked?.id).toBe('normal')
  })

  it('overrides an exact collector number the fingerprint ranked far worse', () => {
    // A misread digit keys a real-but-wrong treatment. The scanned card cannot look far
    // less like its own reference than like a sibling's, so the ranking wins.
    const picked = matchPrinting(treatments, { setCode: 'TLA', collectorNumber: '312' }, [
      { id: 'normal', distance: 13 },
      { id: 'fullart', distance: 87 },
    ])
    expect(picked?.id).toBe('normal')
  })

  it('settles an exact distance tie by the scan order, not the listing order', () => {
    // Two printings of one artwork tie exactly all the time (they differ only in the set
    // symbol and info line). Breaking that tie by listing position would hand the pick back
    // to the arbitrary row order this module exists to escape, so the scan's own ranking —
    // which the server tiebreaks deterministically — has to win.
    const picked = matchPrinting(treatments, {}, [
      { id: 'normal', distance: 62 },
      { id: 'borderless', distance: 62 },
    ])
    expect(picked?.id).toBe('normal')
  })

  it('ignores ranked cards that are not printings in the list', () => {
    // The scan ranks whole cards, so a different card's printing can outrank every printing
    // of the resolved name; it must not leak into this card's pick.
    const picked = matchPrinting(treatments, {}, [
      { id: 'some-other-card', distance: 3 },
      { id: 'borderless', distance: 30 },
    ])
    expect(picked?.id).toBe('borderless')
  })
})
