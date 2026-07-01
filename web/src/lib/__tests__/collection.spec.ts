import { describe, it, expect, vi, afterEach } from 'vitest'

import {
  collectionEntryPath,
  collectionImportCsvPath,
  collectionImportJobPath,
  collectionImportPath,
  collectionPath,
  collectionSourcePath,
  collectionSyncPath,
  getCollectionOwned,
  getCollectionSets,
  getCollectionSummary,
  importCollectionCsv,
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

  it('appends the search query and sort', () => {
    expect(collectionPath('mtg', { q: 't:goblin', sort: 'price', dir: 'desc' })).toBe(
      '/api/collection/mtg?q=t%3Agoblin&sort=price&dir=desc',
    )
  })

  it('omits an empty search query', () => {
    expect(collectionPath('mtg', { q: '', page: 2 })).toBe('/api/collection/mtg?page=2')
  })

  it('appends and encodes the set scope', () => {
    expect(collectionPath('mtg', { set: 'blb' })).toBe('/api/collection/mtg?set=blb')
    expect(collectionPath('mtg', { q: 't:goblin', set: 'blb' })).toBe(
      '/api/collection/mtg?q=t%3Agoblin&set=blb',
    )
  })

  it('encodes the game segment', () => {
    expect(collectionPath('a/b')).toContain('a%2Fb')
  })
})

describe('getCollectionSets / getCollectionSummary', () => {
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
    await getCollectionSets('tok', 'mtg')
    const [url] = fetchMock.mock.calls[0] as [string]
    expect(url).toContain('/api/collection/mtg/sets')
  })

  it('scopes the summary to a set when given', async () => {
    const fetchMock = stubJson({ unique_cards: 0, total_cards: 0, total_value_usd: null })
    await getCollectionSummary('tok', 'mtg', 'blb')
    const [url] = fetchMock.mock.calls[0] as [string]
    expect(url).toContain('/api/collection/mtg/summary?set=blb')
  })

  it('omits the set param for the whole-collection summary', async () => {
    const fetchMock = stubJson({ unique_cards: 0, total_cards: 0, total_value_usd: null })
    await getCollectionSummary('tok', 'mtg')
    const [url] = fetchMock.mock.calls[0] as [string]
    expect(url).toContain('/api/collection/mtg/summary')
    expect(url).not.toContain('set=')
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

  it('builds the CSV import path with the reconcile mode as a query param', () => {
    expect(collectionImportCsvPath('mtg', 'overwrite')).toBe(
      '/api/collection/mtg/import/csv?mode=overwrite',
    )
    expect(collectionImportCsvPath('a/b', 'replace')).toContain('a%2Fb')
  })
})

describe('importCollectionCsv', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('POSTs the raw file body with a text/csv content type and bearer token', async () => {
    type FetchInit = { method: string; headers: Record<string, string>; body: unknown }
    const fetchMock = vi.fn<(url: string, init: FetchInit) => Promise<Response>>(
      async () =>
        ({
          ok: true,
          status: 200,
          text: async () => JSON.stringify({ provider: 'archidekt', matched_cards: 2 }),
        }) as Response,
    )
    vi.stubGlobal('fetch', fetchMock)

    const file = new File(['Scryfall ID,Finish,Quantity\nabc,Normal,1\n'], 'export.csv', {
      type: 'text/csv',
    })
    const summary = await importCollectionCsv('tok', 'mtg', file, 'merge')

    expect(summary).toEqual({ provider: 'archidekt', matched_cards: 2 })
    const [url, init] = fetchMock.mock.calls[0]!
    expect(url).toContain('/api/collection/mtg/import/csv?mode=merge')
    expect(init.method).toBe('POST')
    expect(init.headers['Content-Type']).toBe('text/csv')
    expect(init.headers.Authorization).toBe('Bearer tok')
    // The File is sent verbatim (not JSON-stringified), so it stays re-readable on retry.
    expect(init.body).toBe(file)
  })
})
