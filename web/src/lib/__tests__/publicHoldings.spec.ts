import { describe, it, expect, vi, afterEach } from 'vitest'

import {
  getPublicCollection,
  getPublicCollectionDrops,
  getPublicCollectionSubtypes,
  getPublicWishlist,
  getPublicWishlistDrops,
  getPublicWishlistSubtypes,
} from '../api'

// The public (token-less) holdings reads honour the same copy-count + finish filter as their
// authed twins (issue #677) — the four browse views drive one engine, so a shared collection
// must browse with the same controls as its owner's copy. Path-level pins: the params ride as
// `min_copies` / `max_copies` / `finish`, never folded into `q`.

/** Stub fetch and hand back the URL the call requested. */
async function requestedUrl(call: () => Promise<unknown>): Promise<string> {
  const fetchMock = vi.fn<(url: string, init?: unknown) => Promise<Response>>(
    async () =>
      ({
        ok: true,
        status: 200,
        text: async () =>
          JSON.stringify({ data: [], page: 1, page_size: 20, total: 0, has_more: false }),
      }) as Response,
  )
  vi.stubGlobal('fetch', fetchMock)
  await call()
  const [url] = fetchMock.mock.calls[0] as [string]
  return url
}

describe('public holdings reads', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('carries the copy filter on a public collection listing', async () => {
    const url = await requestedUrl(() =>
      getPublicCollection('alice-0001', 'mtg', { minCopies: 5, finish: 'foil' }),
    )
    expect(url).toContain('/api/u/alice-0001/mtg?min_copies=5&finish=foil')
  })

  it('carries it into the public by-drop and by-sub-type listings', async () => {
    expect(
      await requestedUrl(() =>
        getPublicCollectionDrops('alice-0001', 'mtg', 'sld', { minCopies: 4 }),
      ),
    ).toContain('/api/u/alice-0001/mtg/sets/sld/drops?min_copies=4')
    expect(
      await requestedUrl(() =>
        getPublicCollectionSubtypes('alice-0001', 'mtg', 'blb', { maxCopies: 3 }),
      ),
    ).toContain('/api/u/alice-0001/mtg/sets/blb/subtypes?max_copies=3')
  })

  it('carries it on the public wish-list listings too', async () => {
    expect(
      await requestedUrl(() =>
        getPublicWishlist('alice-0001', 'mtg', { minCopies: 2, maxCopies: 3, finish: 'regular' }),
      ),
    ).toContain('/api/u/alice-0001/wishlist/mtg?min_copies=2&max_copies=3&finish=regular')
    expect(
      await requestedUrl(() =>
        getPublicWishlistDrops('alice-0001', 'mtg', 'sld', { minCopies: 1 }),
      ),
    ).toContain('/api/u/alice-0001/wishlist/mtg/sets/sld/drops?min_copies=1')
    expect(
      await requestedUrl(() =>
        getPublicWishlistSubtypes('alice-0001', 'mtg', 'blb', { finish: 'foil' }),
      ),
    ).toContain('/api/u/alice-0001/wishlist/mtg/sets/blb/subtypes?finish=foil')
  })
})
