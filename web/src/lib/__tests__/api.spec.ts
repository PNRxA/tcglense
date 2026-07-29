import { afterEach, describe, it, expect, vi } from 'vitest'

import {
  cardExportPath,
  cardImageUrl,
  cardNamesPath,
  exportCards,
  getCardPrintingsByName,
  priceHistoryPath,
  setCardExportPath,
  setIconUrl,
} from '../api'

afterEach(() => vi.unstubAllGlobals())

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

describe('getCardPrintingsByName', () => {
  it('requests the selected page at the maximum printing page size', async () => {
    const fetchMock = vi.fn<
      (
        url: string,
        init?: RequestInit,
      ) => Promise<{ ok: boolean; status: number; text: () => Promise<string> }>
    >(async () => ({
      ok: true,
      status: 200,
      text: async () => '{"data":[],"page":3,"page_size":200,"total":816,"has_more":true}',
    }))
    vi.stubGlobal('fetch', fetchMock)

    await getCardPrintingsByName('mtg', 'Island', 3)

    expect(fetchMock).toHaveBeenCalledOnce()
    expect(fetchMock.mock.calls[0]?.[0]).toContain(
      '/api/games/mtg/cards?page=3&page_size=200&sort=released&dir=desc&name=Island',
    )
  })
})

describe('cardExportPath', () => {
  it('is a bare path when the search carries no filters', () => {
    expect(cardExportPath('mtg')).toBe('/api/games/mtg/cards/export')
  })

  it('carries the search and sort so the file matches the grid', () => {
    expect(cardExportPath('mtg', { q: 'c:r t:goblin', sort: 'price', dir: 'desc' })).toBe(
      '/api/games/mtg/cards/export?q=c%3Ar+t%3Agoblin&sort=price&dir=desc',
    )
  })

  it('omits the default text format and states any other', () => {
    expect(cardExportPath('mtg', { format: 'text' })).toBe('/api/games/mtg/cards/export')
    expect(cardExportPath('mtg', { format: 'names' })).toBe(
      '/api/games/mtg/cards/export?format=names',
    )
  })

  it('never carries paging — an export is the whole result set', () => {
    expect(cardExportPath('mtg', { q: 'goblin' })).not.toContain('page')
  })

  it('returns a relative path and encodes the game segment', () => {
    const path = cardExportPath('a/b')
    expect(path.startsWith('/api/')).toBe(true)
    expect(path).toContain('a%2Fb')
  })
})

describe('setCardExportPath', () => {
  it('scopes to the set and encodes both segments', () => {
    expect(setCardExportPath('mtg', 'neo')).toBe('/api/games/mtg/sets/neo/cards/export')
    expect(setCardExportPath('mtg', 'a/b')).toContain('a%2Fb')
  })

  it('passes the related-group scope through', () => {
    expect(setCardExportPath('mtg', 'neo', { includeRelated: true })).toBe(
      '/api/games/mtg/sets/neo/cards/export?include_related=true',
    )
    // False is the default, so it drops out rather than pinning the flag in the URL.
    expect(setCardExportPath('mtg', 'neo', { includeRelated: false })).toBe(
      '/api/games/mtg/sets/neo/cards/export',
    )
  })
})

describe('exportCards', () => {
  it('reads the response as a blob and surfaces a failure as an ApiError', async () => {
    const blob = new Blob(['1 Sol Ring (LTC) 284\n'], { type: 'text/plain' })
    const fetchMock = vi.fn<
      (url: string) => Promise<{ ok: boolean; status: number; blob: () => Promise<Blob> }>
    >(async () => ({ ok: true, status: 200, blob: async () => blob }))
    vi.stubGlobal('fetch', fetchMock)
    await expect(exportCards('mtg', { q: 'goblin' })).resolves.toBe(blob)
    expect(fetchMock.mock.calls[0]?.[0]).toContain('/api/games/mtg/cards/export?q=goblin')

    vi.stubGlobal(
      'fetch',
      vi.fn<
        (url: string) => Promise<{ ok: boolean; status: number; json: () => Promise<unknown> }>
      >(async () => ({
        ok: false,
        status: 422,
        json: async () => ({ error: 'unknown filter' }),
      })),
    )
    await expect(exportCards('mtg', { q: 'bogus:1' })).rejects.toMatchObject({
      status: 422,
      message: 'unknown filter',
    })
  })
})
