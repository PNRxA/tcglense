import { describe, it, expect } from 'vitest'

import { parseManaText, colorLettersToText, type SymbolToken } from '../mana'

// Pull the symbol classes out of a parse, in order.
function classes(text: string): string[] {
  return parseManaText(text)
    .filter((t): t is SymbolToken => t.type === 'symbol')
    .map((t) => t.className)
}

describe('parseManaText', () => {
  it('returns an empty list for an empty string', () => {
    expect(parseManaText('')).toEqual([])
  })

  it('returns a single text run when there are no symbols', () => {
    expect(parseManaText('Draw a card.')).toEqual([{ type: 'text', value: 'Draw a card.' }])
  })

  it('maps a mana cost to consecutive symbol tokens', () => {
    expect(classes('{2}{W}{U}')).toEqual(['ms-2', 'ms-w', 'ms-u'])
  })

  it('interleaves oracle text and symbols, preserving surrounding whitespace', () => {
    expect(parseManaText('{T}: Add {G}.')).toEqual([
      { type: 'symbol', className: 'ms-tap', label: 'Tap' },
      { type: 'text', value: ': Add ' },
      { type: 'symbol', className: 'ms-g', label: 'Green mana' },
      { type: 'text', value: '.' },
    ])
  })

  it('handles hybrid, twobrid, colourless-hybrid and Phyrexian symbols', () => {
    expect(classes('{W/U}')).toEqual(['ms-wu'])
    expect(classes('{2/W}')).toEqual(['ms-2w'])
    expect(classes('{C/W}')).toEqual(['ms-cw'])
    expect(classes('{W/P}')).toEqual(['ms-wp'])
    expect(classes('{G/U/P}')).toEqual(['ms-gup'])
  })

  it('maps tap/untap/snow/energy and the variable + large numeric symbols', () => {
    expect(classes('{Q}')).toEqual(['ms-untap'])
    expect(classes('{S}')).toEqual(['ms-s'])
    expect(classes('{E}')).toEqual(['ms-e'])
    expect(classes('{X}')).toEqual(['ms-x'])
    expect(classes('{10}{100}{1000000}')).toEqual(['ms-10', 'ms-100', 'ms-1000000'])
  })

  it('aliases the unicode half and infinity symbols', () => {
    expect(classes('{½}')).toEqual(['ms-half'])
    expect(classes('{∞}')).toEqual(['ms-infinity'])
  })

  it('is case-insensitive on symbol bodies', () => {
    expect(classes('{w}{t}')).toEqual(['ms-w', 'ms-tap'])
  })

  it('leaves an unrecognised token as literal text, folded into its text run', () => {
    expect(parseManaText('Add {FOO} now')).toEqual([{ type: 'text', value: 'Add {FOO} now' }])
  })

  it('keeps an unrecognised token literal while still parsing real symbols around it', () => {
    expect(parseManaText('{W}{FOO}{U}')).toEqual([
      { type: 'symbol', className: 'ms-w', label: 'White mana' },
      { type: 'text', value: '{FOO}' },
      { type: 'symbol', className: 'ms-u', label: 'Blue mana' },
    ])
  })

  it('produces readable accessibility labels', () => {
    const labels = parseManaText('{W}{2}{T}{C}')
      .filter((t): t is SymbolToken => t.type === 'symbol')
      .map((t) => t.label)
    expect(labels).toEqual(['White mana', '2 generic mana', 'Tap', 'Colorless mana'])
  })

  it('labels hybrid/Phyrexian and non-mana symbols without echoing braces', () => {
    const labelOf = (text: string): string => {
      const first = parseManaText(text)[0]
      return first && first.type === 'symbol' ? first.label : ''
    }
    // Hybrid / twobrid / Phyrexian name each part.
    expect(labelOf('{W/U}')).toBe('White/Blue hybrid mana')
    expect(labelOf('{2/W}')).toBe('2/White hybrid mana')
    expect(labelOf('{W/P}')).toBe('White/Phyrexian hybrid mana')
    expect(labelOf('{G/U/P}')).toBe('Green/Blue/Phyrexian hybrid mana')
    // Symbols that aren't mana must not be labelled "… mana", nor echo the braces.
    expect(labelOf('{CHAOS}')).toBe('Chaos')
    expect(labelOf('{Q}')).toBe('Untap')
    // Genuine mana that used to echo braces.
    expect(labelOf('{∞}')).toBe('Infinity mana')
  })
})

describe('colorLettersToText', () => {
  it('wraps each colour letter in braces so it renders as a pip', () => {
    expect(colorLettersToText(['W', 'U'])).toBe('{W}{U}')
  })

  it('is empty for a colourless identity', () => {
    expect(colorLettersToText([])).toBe('')
  })
})
