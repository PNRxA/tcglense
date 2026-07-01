import { describe, it, expect } from 'vitest'

import { cardImageUrl, cardNamesPath, priceHistoryPath, setIconUrl } from '../api'

describe('cardImageUrl', () => {
  it('builds a proxy URL with the default size', () => {
    expect(cardImageUrl('mtg', 'abc')).toBe('/api/games/mtg/cards/abc/image?size=normal')
  })

  it('includes the size and face when provided', () => {
    expect(cardImageUrl('mtg', 'abc', 'large', 1)).toBe(
      '/api/games/mtg/cards/abc/image?size=large&face=1',
    )
  })

  it('encodes path segments to avoid breaking the URL', () => {
    expect(cardImageUrl('mtg', 'a/b')).toContain('a%2Fb')
  })
})

describe('setIconUrl', () => {
  it('builds the set-icon proxy URL', () => {
    expect(setIconUrl('mtg', 'blb')).toBe('/api/games/mtg/sets/blb/icon')
  })
})

describe('cardNamesPath', () => {
  it('builds the autocomplete path with the query and a default limit', () => {
    expect(cardNamesPath('mtg', 'bolt')).toBe('/api/games/mtg/card-names?q=bolt&limit=10')
  })

  it('honours an explicit limit and URL-encodes the query', () => {
    expect(cardNamesPath('mtg', 'sol ring', 5)).toBe('/api/games/mtg/card-names?q=sol+ring&limit=5')
    // A name full of punctuation binds safely into the query string.
    expect(cardNamesPath('mtg', 'Ach! Hans, Run!')).toContain('q=Ach%21+Hans%2C+Run%21')
  })

  it('encodes the game path segment', () => {
    expect(cardNamesPath('a/b', 'x')).toContain('/api/games/a%2Fb/card-names')
  })
})

describe('priceHistoryPath', () => {
  it('omits the range query when none is given (full daily series)', () => {
    expect(priceHistoryPath('mtg', 'abc')).toBe('/api/games/mtg/cards/abc/prices')
  })

  it('appends the selected range', () => {
    expect(priceHistoryPath('mtg', 'abc', '1y')).toBe('/api/games/mtg/cards/abc/prices?range=1y')
    expect(priceHistoryPath('mtg', 'abc', 'all')).toBe('/api/games/mtg/cards/abc/prices?range=all')
  })

  it('returns a relative path (no API origin) and encodes path segments', () => {
    const path = priceHistoryPath('mtg', 'a/b', '30d')
    expect(path.startsWith('/api/')).toBe(true)
    expect(path).toContain('a%2Fb')
  })
})
