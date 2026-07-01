import { describe, it, expect, vi, afterEach } from 'vitest'

import {
  collectionEntryPath,
  collectionImportJobPath,
  collectionImportPath,
  collectionPath,
  collectionSourcePath,
  collectionSyncPath,
  getCollectionOwned,
} from '../api'

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

describe('getCollectionOwned', () => {
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

  it('POSTs the ids and returns the owned-counts map', async () => {
    const fetchMock = stubFetch(() => ({ data: { a: { quantity: 2, foil_quantity: 1 } } }))
    const map = await getCollectionOwned('tok', 'mtg', ['a', 'b'])

    expect(map).toEqual({ a: { quantity: 2, foil_quantity: 1 } })
    expect(fetchMock).toHaveBeenCalledTimes(1)
    const [url, init] = fetchMock.mock.calls[0] as [string, { method: string; body: string }]
    expect(url).toContain('/api/collection/mtg/owned')
    expect(init.method).toBe('POST')
    expect(JSON.parse(init.body)).toEqual({ ids: ['a', 'b'] })
  })

  it('splits a large id list into batches under the server cap and merges them', async () => {
    // Echo one owned entry per requested id so the merged map reflects every batch.
    const fetchMock = stubFetch((ids) => ({
      data: Object.fromEntries(ids.map((id) => [id, { quantity: 1, foil_quantity: 0 }])),
    }))
    const ids = Array.from({ length: 950 }, (_, i) => `c${i}`)
    const map = await getCollectionOwned('tok', 'mtg', ids)

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
    expect(await getCollectionOwned('tok', 'mtg', [])).toEqual({})
    expect(fetchMock).not.toHaveBeenCalled()
  })
})

describe('import / sync paths', () => {
  it('builds the import, source, and sync paths', () => {
    expect(collectionImportPath('mtg')).toBe('/api/collection/mtg/import')
    expect(collectionSourcePath('mtg')).toBe('/api/collection/mtg/source')
    expect(collectionSyncPath('mtg')).toBe('/api/collection/mtg/sync')
  })

  it('builds the import-job status path', () => {
    expect(collectionImportJobPath('mtg', 42)).toBe('/api/collection/mtg/import/jobs/42')
  })

  it('encodes the game segment', () => {
    expect(collectionImportPath('a/b')).toContain('a%2Fb')
    expect(collectionSourcePath('a/b')).toContain('a%2Fb')
    expect(collectionSyncPath('a/b')).toContain('a%2Fb')
    expect(collectionImportJobPath('a/b', 1)).toContain('a%2Fb')
  })
})
