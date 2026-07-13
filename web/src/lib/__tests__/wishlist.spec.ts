import { describe, it, expect, vi, afterEach } from 'vitest'

import {
  getWishlistCounts,
  getWishlistProductEntry,
  getWishlistProducts,
  getWishlistSetDrops,
  getWishlistSets,
  getWishlistSummary,
  setWishlistProductEntry,
  wishlistEntryPath,
  wishlistPath,
  wishlistProductEntryPath,
  wishlistProductsPath,
  wishlistSetDropsPath,
} from '../api'

describe('wishlistPath', () => {
  it('builds the base wish-list path with no params', () => {
    expect(wishlistPath('mtg')).toBe('/api/wishlist/mtg')
  })

  it('appends pagination params', () => {
    expect(wishlistPath('mtg', { page: 2, pageSize: 60 })).toBe(
      '/api/wishlist/mtg?page=2&page_size=60',
    )
  })

  it('omits falsy params', () => {
    expect(wishlistPath('mtg', { page: 3 })).toBe('/api/wishlist/mtg?page=3')
  })

  it('appends the search query and sort', () => {
    expect(wishlistPath('mtg', { q: 't:goblin', sort: 'price', dir: 'desc' })).toBe(
      '/api/wishlist/mtg?q=t%3Agoblin&sort=price&dir=desc',
    )
  })

  it('omits an empty search query', () => {
    expect(wishlistPath('mtg', { q: '', page: 2 })).toBe('/api/wishlist/mtg?page=2')
  })

  it('appends and encodes the set scope', () => {
    expect(wishlistPath('mtg', { set: 'blb' })).toBe('/api/wishlist/mtg?set=blb')
    expect(wishlistPath('mtg', { q: 't:goblin', set: 'blb' })).toBe(
      '/api/wishlist/mtg?q=t%3Agoblin&set=blb',
    )
  })

  it('spans the set group with include_related', () => {
    expect(wishlistPath('mtg', { set: 'blb', includeRelated: true })).toBe(
      '/api/wishlist/mtg?set=blb&include_related=true',
    )
    // Omitted when falsy, so a plain set scope stays a single set.
    expect(wishlistPath('mtg', { set: 'blb', includeRelated: false })).toBe(
      '/api/wishlist/mtg?set=blb',
    )
  })

  it('encodes the game segment', () => {
    expect(wishlistPath('a/b')).toContain('a%2Fb')
  })
})

describe('wishlistSetDropsPath', () => {
  it('builds the by-drop path with no params', () => {
    expect(wishlistSetDropsPath('mtg', 'sld')).toBe('/api/wishlist/mtg/sets/sld/drops')
  })

  it('appends pagination + search params', () => {
    expect(wishlistSetDropsPath('mtg', 'sld', { page: 2, pageSize: 20, q: 't:goblin' })).toBe(
      '/api/wishlist/mtg/sets/sld/drops?page=2&page_size=20&q=t%3Agoblin',
    )
  })

  it('encodes the game + set segments', () => {
    expect(wishlistSetDropsPath('a/b', 'c/d')).toContain('a%2Fb')
    expect(wishlistSetDropsPath('a/b', 'c/d')).toContain('c%2Fd')
  })
})

describe('getWishlistSetDrops', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('requests the by-drop endpoint with the passed params', async () => {
    const fetchMock = vi.fn<(url: string, init?: unknown) => Promise<Response>>(async () => {
      return {
        ok: true,
        status: 200,
        text: async () =>
          JSON.stringify({ data: [], page: 1, page_size: 20, total: 0, has_more: false }),
      } as Response
    })
    vi.stubGlobal('fetch', fetchMock)
    await getWishlistSetDrops('tok', 'mtg', 'sld', { page: 3 })
    const [url] = fetchMock.mock.calls[0] as [string]
    expect(url).toContain('/api/wishlist/mtg/sets/sld/drops?page=3')
  })
})

describe('getWishlistSets / getWishlistSummary', () => {
  afterEach(() => vi.unstubAllGlobals())

  function stubJson(payload: unknown) {
    const fetchMock = vi.fn<(url: string, init?: unknown) => Promise<Response>>(async () => {
      return { ok: true, status: 200, text: async () => JSON.stringify(payload) } as Response
    })
    vi.stubGlobal('fetch', fetchMock)
    return fetchMock
  }

  it('requests the per-set landing endpoint', async () => {
    const fetchMock = stubJson({ data: [] })
    await getWishlistSets('tok', 'mtg')
    const [url] = fetchMock.mock.calls[0] as [string]
    expect(url).toContain('/api/wishlist/mtg/sets')
  })

  it('scopes the summary to a set when given', async () => {
    const fetchMock = stubJson({ unique_cards: 0, total_cards: 0, total_value_usd: null })
    await getWishlistSummary('tok', 'mtg', 'blb')
    const [url] = fetchMock.mock.calls[0] as [string]
    expect(url).toContain('/api/wishlist/mtg/summary?set=blb')
  })

  it('omits the set param for the whole-wish-list summary', async () => {
    const fetchMock = stubJson({ unique_cards: 0, total_cards: 0, total_value_usd: null })
    await getWishlistSummary('tok', 'mtg')
    const [url] = fetchMock.mock.calls[0] as [string]
    expect(url).toContain('/api/wishlist/mtg/summary')
    expect(url).not.toContain('set=')
  })

  it('spans the set group with include_related', async () => {
    const fetchMock = stubJson({ unique_cards: 0, total_cards: 0, total_value_usd: null })
    await getWishlistSummary('tok', 'mtg', 'blb', true)
    const [url] = fetchMock.mock.calls[0] as [string]
    expect(url).toContain('set=blb')
    expect(url).toContain('include_related=true')
  })

  it('ignores include_related without a set scope', async () => {
    const fetchMock = stubJson({ unique_cards: 0, total_cards: 0, total_value_usd: null })
    await getWishlistSummary('tok', 'mtg', undefined, true)
    const [url] = fetchMock.mock.calls[0] as [string]
    expect(url).not.toContain('include_related')
    expect(url).not.toContain('set=')
  })
})

describe('wishlistEntryPath', () => {
  it('builds the per-card path', () => {
    expect(wishlistEntryPath('mtg', 'abc')).toBe('/api/wishlist/mtg/cards/abc')
  })

  it('encodes path segments to avoid breaking the URL', () => {
    expect(wishlistEntryPath('mtg', 'a/b')).toContain('a%2Fb')
  })
})

describe('getWishlistCounts', () => {
  afterEach(() => vi.unstubAllGlobals())

  // Stub the global fetch the request client uses; the callback shapes each response
  // body from the POSTed ids so a test can observe batching and merging.
  function stubFetch(payloadFor: (ids: string[]) => unknown) {
    const fetchMock = vi.fn<(url: string, init: { body?: string }) => Promise<Response>>(
      async (_url, init) => {
        const body = JSON.parse(init.body ?? '{}') as { ids: string[] }
        return {
          ok: true,
          status: 200,
          text: async () => JSON.stringify(payloadFor(body.ids)),
        } as Response
      },
    )
    vi.stubGlobal('fetch', fetchMock)
    return fetchMock
  }

  it('POSTs the ids to /counts and returns the wanted-counts map', async () => {
    const fetchMock = stubFetch(() => ({ data: { a: { quantity: 2, foil_quantity: 1 } } }))
    const map = await getWishlistCounts('tok', 'mtg', ['a', 'b'])

    expect(map).toEqual({ a: { quantity: 2, foil_quantity: 1 } })
    expect(fetchMock).toHaveBeenCalledTimes(1)
    const [url, init] = fetchMock.mock.calls[0] as [string, { method: string; body: string }]
    expect(url).toContain('/api/wishlist/mtg/counts')
    expect(init.method).toBe('POST')
    expect(JSON.parse(init.body)).toEqual({ ids: ['a', 'b'] })
  })

  it('splits a large id list into batches under the server cap and merges them', async () => {
    // Echo one wanted entry per requested id so the merged map reflects every batch.
    const fetchMock = stubFetch((ids) => ({
      data: Object.fromEntries(ids.map((id) => [id, { quantity: 1, foil_quantity: 0 }])),
    }))
    const ids = Array.from({ length: 950 }, (_, i) => `c${i}`)
    const map = await getWishlistCounts('tok', 'mtg', ids)

    // 950 / 400 per batch = 3 requests, each ≤ the 500-id server cap.
    expect(fetchMock).toHaveBeenCalledTimes(3)
    for (const [, init] of fetchMock.mock.calls as [string, { body: string }][]) {
      expect(JSON.parse(init.body).ids.length).toBeLessThanOrEqual(500)
    }
    expect(Object.keys(map)).toHaveLength(950)
    expect(map.c0).toEqual({ quantity: 1, foil_quantity: 0 })
    expect(map.c949).toEqual({ quantity: 1, foil_quantity: 0 })
  })

  it('makes no request for an empty id list', async () => {
    const fetchMock = stubFetch(() => ({ data: {} }))
    expect(await getWishlistCounts('tok', 'mtg', [])).toEqual({})
    expect(fetchMock).not.toHaveBeenCalled()
  })
})

describe('wishlistProductsPath', () => {
  it('builds the products path with pagination params', () => {
    expect(wishlistProductsPath('mtg', { page: 2, pageSize: 60 })).toBe(
      '/api/wishlist/mtg/products?page=2&page_size=60',
    )
  })

  it('builds the bare products path with no params', () => {
    expect(wishlistProductsPath('mtg')).toBe('/api/wishlist/mtg/products')
  })

  it('omits falsy params', () => {
    expect(wishlistProductsPath('mtg', { page: 3 })).toBe('/api/wishlist/mtg/products?page=3')
  })

  it('encodes the game segment', () => {
    expect(wishlistProductsPath('a/b')).toContain('a%2Fb')
  })
})

describe('wishlistProductEntryPath', () => {
  it('builds the per-product path', () => {
    expect(wishlistProductEntryPath('mtg', '100')).toBe('/api/wishlist/mtg/products/100')
  })

  it('encodes path segments to avoid breaking the URL', () => {
    expect(wishlistProductEntryPath('mtg', 'a/b')).toContain('a%2Fb')
    expect(wishlistProductEntryPath('a/b', '100')).toContain('a%2Fb')
  })
})

describe('getWishlistProducts / getWishlistProductEntry / setWishlistProductEntry', () => {
  afterEach(() => vi.unstubAllGlobals())

  type FetchInit = { method?: string; headers?: Record<string, string>; body?: string }

  function stubJson(payload: unknown) {
    const fetchMock = vi.fn<(url: string, init?: FetchInit) => Promise<Response>>(
      async () =>
        ({ ok: true, status: 200, text: async () => JSON.stringify(payload) }) as Response,
    )
    vi.stubGlobal('fetch', fetchMock)
    return fetchMock
  }

  it('GETs the paged products list with the bearer token', async () => {
    const fetchMock = stubJson({ data: [], page: 1, page_size: 60, total: 0, has_more: false })
    await getWishlistProducts('tok', 'mtg', { page: 2, pageSize: 60 })
    const [url, init] = fetchMock.mock.calls[0] as [string, FetchInit]
    expect(url).toContain('/api/wishlist/mtg/products?page=2&page_size=60')
    expect(init.method ?? 'GET').toBe('GET')
    expect(init.headers?.Authorization).toBe('Bearer tok')
  })

  it('GETs one product entry at its path', async () => {
    const fetchMock = stubJson({ quantity: 0, foil_quantity: 0 })
    await getWishlistProductEntry('tok', 'mtg', '100')
    const [url, init] = fetchMock.mock.calls[0] as [string, FetchInit]
    expect(url).toContain('/api/wishlist/mtg/products/100')
    expect(init.method ?? 'GET').toBe('GET')
  })

  it('PUTs absolute counts to one product entry with the bearer token', async () => {
    const fetchMock = stubJson({ quantity: 3, foil_quantity: 0 })
    const res = await setWishlistProductEntry('tok', 'mtg', '100', {
      quantity: 3,
      foil_quantity: 0,
    })
    const [url, init] = fetchMock.mock.calls[0] as [string, FetchInit]
    expect(url).toContain('/api/wishlist/mtg/products/100')
    expect(init.method).toBe('PUT')
    expect(init.headers?.Authorization).toBe('Bearer tok')
    expect(JSON.parse(init.body ?? '{}')).toEqual({ quantity: 3, foil_quantity: 0 })
    expect(res).toEqual({ quantity: 3, foil_quantity: 0 })
  })
})
