import { describe, it, expect } from 'vitest'
import {
  cleanCardName,
  isSameHeldCard,
  nameQueryCandidates,
  normalizeCollectorNumber,
  parseSetHint,
  sameCardText,
} from '../ocr'

describe('cleanCardName', () => {
  it('collapses whitespace and newlines', () => {
    expect(cleanCardName('  Lightning   \n Bolt  ')).toBe('Lightning Bolt')
  })

  it('normalises curly apostrophes to straight ones', () => {
    expect(cleanCardName('Gaea’s Cradle')).toBe("Gaea's Cradle")
    expect(cleanCardName('Gaea`s Cradle')).toBe("Gaea's Cradle")
  })

  it('keeps accented letters and in-name punctuation', () => {
    expect(cleanCardName('Lim-Dûl the Necromancer')).toBe('Lim-Dûl the Necromancer')
    expect(cleanCardName('Ach! Hans, Run!')).toBe('Ach! Hans, Run!')
    expect(cleanCardName('Borrowing 100,000 Arrows')).toBe('Borrowing 100,000 Arrows')
  })

  it('drops mana pips and frame glyphs the OCR hallucinates into the title', () => {
    expect(cleanCardName('Serra Angel ⓦ★|')).toBe('Serra Angel')
    expect(cleanCardName('~ Counterspell ~')).toBe('Counterspell')
  })

  it('trims leading and trailing punctuation noise', () => {
    expect(cleanCardName('. : Island')).toBe('Island')
    expect(cleanCardName('Island -')).toBe('Island')
  })

  it('returns empty for a strip with no letters', () => {
    expect(cleanCardName('★ ✦ 12 //')).toBe('12')
    expect(cleanCardName('|||')).toBe('')
  })
})

describe('nameQueryCandidates', () => {
  it('returns the full string first', () => {
    expect(nameQueryCandidates('Lightning Bolt')).toEqual(['Lightning Bolt'])
  })

  it('adds shorter leading prefixes when the name runs long (type line bled in)', () => {
    expect(nameQueryCandidates('Serra Angel Creature Angel')).toEqual([
      'Serra Angel Creature Angel',
      'Serra Angel Creature',
      'Serra Angel',
    ])
  })

  it('drops candidates below the minimum length', () => {
    expect(nameQueryCandidates('ab')).toEqual([])
    expect(nameQueryCandidates('a')).toEqual([])
  })

  it('does not add redundant prefixes for a one- or two-word name', () => {
    expect(nameQueryCandidates('Island')).toEqual(['Island'])
    expect(nameQueryCandidates('Serra Angel')).toEqual(['Serra Angel'])
  })
})

describe('normalizeCollectorNumber', () => {
  it('strips zero padding but keeps a bare zero and letter suffixes', () => {
    expect(normalizeCollectorNumber('0123')).toBe('123')
    expect(normalizeCollectorNumber('007a')).toBe('7a')
    expect(normalizeCollectorNumber('0')).toBe('0')
    expect(normalizeCollectorNumber(' 42 ')).toBe('42')
  })
})

describe('parseSetHint', () => {
  it('reads the collector number from the slashed form and the set code', () => {
    expect(parseSetHint('0123/0264 U\nNEO • EN')).toEqual({
      collectorNumber: '123',
      setCode: 'NEO',
    })
  })

  it('excludes the language code when choosing the set code', () => {
    expect(parseSetHint('264 R  MH2 · EN  John Avon').setCode).toBe('MH2')
  })

  it('keeps numeric-containing set codes like 40K and 2X2', () => {
    expect(parseSetHint('012/180 C  40K  EN').setCode).toBe('40K')
    expect(parseSetHint('55/332  2X2  EN').setCode).toBe('2X2')
  })

  it('returns an empty hint when there is no code-shaped token or slashed number', () => {
    expect(parseSetHint('12 34')).toEqual({})
    expect(parseSetHint('by an artiste')).toEqual({})
  })

  it('does not treat a bare number as a collector number', () => {
    expect(parseSetHint('artist 264').collectorNumber).toBeUndefined()
  })

  it('flags foil when the collector line carries a star, keeping the plain number and code', () => {
    // Modern foils print a `★` on the collector line; the star drives the finish while the
    // number/code stay plain so printing resolution lands on the ordinary card.
    const hint = parseSetHint('0123/0264 R ★\nNEO • EN')
    expect(hint.foil).toBe(true)
    expect(hint.collectorNumber).toBe('123')
    expect(hint.setCode).toBe('NEO')
  })

  it('still reads the number when the star sits against it (123★/264)', () => {
    // The star is blanked before the slashed-number match, so an adjacent star can't hide it.
    const hint = parseSetHint('0123★/0264 R\nNEO • EN')
    expect(hint.foil).toBe(true)
    expect(hint.collectorNumber).toBe('123')
    expect(hint.setCode).toBe('NEO')
  })

  it('leaves foil unset when no star is present', () => {
    expect(parseSetHint('0123/0264 U\nNEO • EN').foil).toBeUndefined()
  })
})

describe('sameCardText', () => {
  it('is true for identical text ignoring case and padding', () => {
    expect(sameCardText('Lightning Bolt', '  lightning bolt ')).toBe(true)
  })

  it('is true when one is a prefix of the other (OCR trims the last word)', () => {
    expect(sameCardText('Lightning', 'Lightning Bolt')).toBe(true)
  })

  it('is false for different names and for empty input', () => {
    expect(sameCardText('Lightning Bolt', 'Counterspell')).toBe(false)
    expect(sameCardText('', 'Island')).toBe(false)
  })
})

describe('isSameHeldCard', () => {
  it('treats identical text and a truncated last word as the same held card', () => {
    expect(isSameHeldCard('Lightning Bolt', 'lightning bolt')).toBe(true)
    // OCR jitter clipping the final word's tail — still the same card.
    expect(isSameHeldCard('Lightning Bol', 'Lightning Bolt')).toBe(true)
  })

  it('does NOT suppress a distinct card whose name extends the current one by a word', () => {
    // "Island" and "Island Sanctuary" are different real cards — must fall through.
    expect(isSameHeldCard('Island', 'Island Sanctuary')).toBe(false)
    // A whole extra word (cleanCardName turns "Fire // Ice" into "Fire Ice").
    expect(isSameHeldCard('Fire', 'Fire Ice')).toBe(false)
    // Accepted limitation: a within-word extension stays "same" to tolerate OCR truncation
    // jitter, so Fire/Fireball can't be told apart by this gate (catalog resolution can).
    expect(isSameHeldCard('Fire', 'Fireball')).toBe(true)
  })

  it('is false for unrelated names', () => {
    expect(isSameHeldCard('Mountain', 'Forest')).toBe(false)
  })
})
