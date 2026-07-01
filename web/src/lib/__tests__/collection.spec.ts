import { describe, it, expect } from 'vitest'

import { collectionEntryPath, collectionPath } from '../api'

describe('collectionPath', () => {
  it('builds the base collection path with no params', () => {
    expect(collectionPath('mtg')).toBe('/api/collection/mtg')
  })

  it('appends pagination params', () => {
    expect(collectionPath('mtg', { page: 2, pageSize: 60 })).toBe(
      '/api/collection/mtg?page=2&page_size=60',
    )
  })

  it('omits falsy params', () => {
    expect(collectionPath('mtg', { page: 3 })).toBe('/api/collection/mtg?page=3')
  })

  it('encodes the game segment', () => {
    expect(collectionPath('a/b')).toContain('a%2Fb')
  })
})

describe('collectionEntryPath', () => {
  it('builds the per-card path', () => {
    expect(collectionEntryPath('mtg', 'abc')).toBe('/api/collection/mtg/cards/abc')
  })

  it('encodes path segments to avoid breaking the URL', () => {
    expect(collectionEntryPath('mtg', 'a/b')).toContain('a%2Fb')
  })
})
