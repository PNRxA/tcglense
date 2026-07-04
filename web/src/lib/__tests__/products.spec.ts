import { describe, it, expect } from 'vitest'

import { productImageUrl, productPriceHistoryPath } from '../api'

describe('productImageUrl', () => {
  it('builds a proxy URL with the default size', () => {
    expect(productImageUrl('mtg', 'abc')).toBe('/api/games/mtg/products/abc/image?size=normal')
  })

  it('includes the requested size', () => {
    expect(productImageUrl('mtg', 'abc', 'small')).toBe(
      '/api/games/mtg/products/abc/image?size=small',
    )
  })

  it('encodes path segments to avoid breaking the URL', () => {
    expect(productImageUrl('mtg', 'a/b')).toContain('a%2Fb')
  })
})

describe('productPriceHistoryPath', () => {
  it('omits the range query when none is given (full daily series)', () => {
    expect(productPriceHistoryPath('mtg', 'abc')).toBe('/api/games/mtg/products/abc/prices')
  })

  it('appends the selected range', () => {
    expect(productPriceHistoryPath('mtg', 'abc', '1y')).toBe(
      '/api/games/mtg/products/abc/prices?range=1y',
    )
    expect(productPriceHistoryPath('mtg', 'abc', 'all')).toBe(
      '/api/games/mtg/products/abc/prices?range=all',
    )
  })

  it('returns a relative path (no API origin) and encodes path segments', () => {
    const path = productPriceHistoryPath('mtg', 'a/b', '30d')
    expect(path.startsWith('/api/')).toBe(true)
    expect(path).toContain('a%2Fb')
  })
})
