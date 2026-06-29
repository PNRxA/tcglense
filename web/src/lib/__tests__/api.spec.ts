import { describe, it, expect } from 'vitest'

import { cardImageUrl } from '../api'

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
