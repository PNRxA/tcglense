import { describe, it, expect, vi, afterEach } from 'vitest'

import {
  ApiError,
  collectionEntryPath,
  collectionExportPath,
  collectionImportCsvPath,
  collectionImportJobPath,
  collectionImportPath,
  collectionPath,
  collectionProductCountsPath,
  collectionProductEntryPath,
  collectionProductsPath,
  collectionProductSummaryPath,
  collectionSetDropsPath,
  collectionSourcePath,
  collectionSyncPath,
  collectionValueHistoryPath,
  exportCollectionCsv,
  getCollectionOwned,
  getCollectionProductCounts,
  getCollectionProductEntry,
  getCollectionProducts,
  getCollectionProductSummary,
  getCollectionSetDrops,
  getCollectionSets,
  getCollectionSummary,
  getCollectionValueHistory,
  importCollectionCsv,
  importCollectionText,
  setCollectionProductEntry,
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

  it('spans the set group with include_related', () => {
    expect(collectionPath('mtg', { set: 'blb', includeRelated: true })).toBe(
      '/api/collection/mtg?set=blb&include_related=true',
    )
    // Omitted when falsy, so a plain set scope stays a single set.
    expect(collectionPath('mtg', { set: 'blb', includeRelated: false })).toBe(
      '/api/collection/mtg?set=blb',
    )
  })

  it('encodes the game segment', () => {
    expect(collectionPath('a/b')).toContain('a%2Fb')
  })
})

describe('collectionSetDropsPath', () => {
  it('builds the by-drop path with no params', () => {
    expect(collectionSetDropsPath('mtg', 'sld')).toBe('/api/collection/mtg/sets/sld/drops')
  })

  it('appends pagination + search params', () => {
    expect(collectionSetDropsPath('mtg', 'sld', { page: 2, pageSize: 20, q: 't:goblin' })).toBe(
      '/api/collection/mtg/sets/sld/drops?page=2&page_size=20&q=t%3Agoblin',
    )
  })

  it('encodes the game + set segments', () => {
    expect(collectionSetDropsPath('a/b', 'c/d')).toContain('a%2Fb')
    expect(collectionSetDropsPath('a/b', 'c/d')).toContain('c%2Fd')
  })
})

describe('getCollectionSetDrops', () => {
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
    await getCollectionSetDrops('tok', 'mtg', 'sld', { page: 3 })
    const [url] = fetchMock.mock.calls[0] as [string]
    expect(url).toContain('/api/collection/mtg/sets/sld/drops?page=3')
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

  it('spans the set group with include_related', async () => {
    const fetchMock = stubJson({ unique_cards: 0, total_cards: 0, total_value_usd: null })
    await getCollectionSummary('tok', 'mtg', 'blb', true)
    const [url] = fetchMock.mock.calls[0] as [string]
    expect(url).toContain('set=blb')
    expect(url).toContain('include_related=true')
  })

  it('ignores include_related without a set scope', async () => {
    const fetchMock = stubJson({ unique_cards: 0, total_cards: 0, total_value_usd: null })
    await getCollectionSummary('tok', 'mtg', undefined, true)
    const [url] = fetchMock.mock.calls[0] as [string]
    expect(url).not.toContain('include_related')
    expect(url).not.toContain('set=')
  })

  it('sends the bulk threshold on the summary when given', async () => {
    const fetchMock = stubJson({ unique_cards: 0, total_cards: 0, total_value_usd: null })
    await getCollectionSummary('tok', 'mtg', undefined, undefined, 250)
    const [url] = fetchMock.mock.calls[0] as [string]
    expect(url).toContain('bulk_max_cents=250')
  })

  it('sends a $0 bulk threshold (a meaningful "nothing is bulk" value), not just truthy ones', async () => {
    const fetchMock = stubJson({ unique_cards: 0, total_cards: 0, total_value_usd: null })
    await getCollectionSummary('tok', 'mtg', undefined, undefined, 0)
    const [url] = fetchMock.mock.calls[0] as [string]
    expect(url).toContain('bulk_max_cents=0')
  })

  it('omits the bulk threshold when unspecified', async () => {
    const fetchMock = stubJson({ unique_cards: 0, total_cards: 0, total_value_usd: null })
    await getCollectionSummary('tok', 'mtg')
    const [url] = fetchMock.mock.calls[0] as [string]
    expect(url).not.toContain('bulk_max_cents')
  })

  it('sends the bulk threshold on the per-set landing', async () => {
    const fetchMock = stubJson({ data: [] })
    await getCollectionSets('tok', 'mtg', 500)
    const [url] = fetchMock.mock.calls[0] as [string]
    expect(url).toContain('/api/collection/mtg/sets?bulk_max_cents=500')
  })
})

describe('collectionValueHistoryPath', () => {
  it('builds the value-history path with no range', () => {
    expect(collectionValueHistoryPath('mtg')).toBe('/api/collection/mtg/value-history')
  })

  it('appends and encodes the range', () => {
    expect(collectionValueHistoryPath('mtg', '30d')).toBe(
      '/api/collection/mtg/value-history?range=30d',
    )
  })
})

describe('getCollectionValueHistory', () => {
  afterEach(() => vi.unstubAllGlobals())

  function stubJson(payload: unknown) {
    const fetchMock = vi.fn<(url: string, init?: unknown) => Promise<Response>>(async () => {
      return { ok: true, status: 200, text: async () => JSON.stringify(payload) } as Response
    })
    vi.stubGlobal('fetch', fetchMock)
    return fetchMock
  }

  it('requests the value-history endpoint with the range', async () => {
    const fetchMock = stubJson({ data: [] })
    await getCollectionValueHistory('tok', 'mtg', '1y')
    const [url] = fetchMock.mock.calls[0] as [string]
    expect(url).toContain('/api/collection/mtg/value-history?range=1y')
  })

  it('maps card and sealed wire values onto the chart-shaped primary and secondary fields', async () => {
    stubJson({
      data: [
        { date: '2024-01-01', value_usd: null, sealed_value_usd: '50.00' },
        { date: '2024-01-02', value_usd: '123.45', sealed_value_usd: '75.00' },
      ],
    })
    const result = await getCollectionValueHistory('tok', 'mtg')
    expect(result.data).toEqual([
      { date: '2024-01-01', usd: null, usd_foil: '50.00' },
      { date: '2024-01-02', usd: '123.45', usd_foil: '75.00' },
    ])
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

describe('collection sealed products', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('builds list, entry, summary, and owned-count paths', () => {
    expect(collectionProductsPath('mtg', { page: 2, pageSize: 60 })).toBe(
      '/api/collection/mtg/products?page=2&page_size=60',
    )
    expect(collectionProductEntryPath('mtg', 'a/b')).toBe('/api/collection/mtg/products/a%2Fb')
    expect(collectionProductSummaryPath('mtg')).toBe('/api/collection/mtg/products/summary')
    expect(collectionProductCountsPath('mtg')).toBe('/api/collection/mtg/products/owned')
  })

  it('reads and writes through the collection product routes', async () => {
    const fetchMock = vi.fn<(url: string, init?: RequestInit) => Promise<Response>>(
      async (url) =>
        ({
          ok: true,
          status: 200,
          text: async () =>
            JSON.stringify(
              url.endsWith('/summary')
                ? { unique_products: 0, total_products: 0, total_value_usd: null }
                : url.includes('?page=')
                  ? { data: [], page: 1, page_size: 60, total: 0, has_more: false }
                  : { quantity: 2, foil_quantity: 0 },
            ),
        }) as Response,
    )
    vi.stubGlobal('fetch', fetchMock)

    await getCollectionProducts('tok', 'mtg', { page: 1 })
    await getCollectionProductEntry('tok', 'mtg', '100')
    await getCollectionProductSummary('tok', 'mtg')
    await setCollectionProductEntry('tok', 'mtg', '100', {
      quantity: 2,
      foil_quantity: 0,
    })

    expect(fetchMock.mock.calls.map(([url]) => url)).toEqual([
      expect.stringContaining('/api/collection/mtg/products?page=1'),
      expect.stringContaining('/api/collection/mtg/products/100'),
      expect.stringContaining('/api/collection/mtg/products/summary'),
      expect.stringContaining('/api/collection/mtg/products/100'),
    ])
    expect(fetchMock.mock.calls[3]![1]?.method).toBe('PUT')
  })

  it('batches collection product owned counts', async () => {
    const fetchMock = vi.fn<(url: string, init?: RequestInit) => Promise<Response>>(
      async () =>
        ({
          ok: true,
          status: 200,
          text: async () => JSON.stringify({ data: { '100': { quantity: 2, foil_quantity: 0 } } }),
        }) as Response,
    )
    vi.stubGlobal('fetch', fetchMock)

    expect(await getCollectionProductCounts('tok', 'mtg', ['100'])).toEqual({
      '100': { quantity: 2, foil_quantity: 0 },
    })
    expect(fetchMock.mock.calls[0]![0]).toContain('/api/collection/mtg/products/owned')
    expect(fetchMock.mock.calls[0]![1]?.method).toBe('POST')
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

describe('collectionExportPath', () => {
  it('builds the export path with the format as a query param', () => {
    expect(collectionExportPath('mtg', 'archidekt')).toBe(
      '/api/collection/mtg/export?format=archidekt',
    )
    expect(collectionExportPath('mtg', 'moxfield')).toBe(
      '/api/collection/mtg/export?format=moxfield',
    )
  })

  it('encodes the game segment', () => {
    expect(collectionExportPath('a/b', 'archidekt')).toContain('a%2Fb')
  })
})

describe('exportCollectionCsv', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('GETs the export endpoint with the bearer token and returns the blob', async () => {
    const csv = new Blob(['Quantity,Name\n1,Card\n'], { type: 'text/csv' })
    type FetchInit = { headers: Record<string, string> }
    const fetchMock = vi.fn<(url: string, init: FetchInit) => Promise<Response>>(
      async () => ({ ok: true, status: 200, blob: async () => csv }) as Response,
    )
    vi.stubGlobal('fetch', fetchMock)

    const blob = await exportCollectionCsv('tok', 'mtg', 'moxfield')

    expect(blob).toBe(csv)
    const [url, init] = fetchMock.mock.calls[0]!
    expect(url).toContain('/api/collection/mtg/export?format=moxfield')
    expect(init.headers.Authorization).toBe('Bearer tok')
  })

  it('throws an ApiError carrying the status on a non-2xx response', async () => {
    const fetchMock = vi.fn<() => Promise<Response>>(
      async () => ({ ok: false, status: 500 }) as Response,
    )
    vi.stubGlobal('fetch', fetchMock)

    await expect(exportCollectionCsv('tok', 'mtg', 'archidekt')).rejects.toMatchObject({
      status: 500,
    })
    await expect(exportCollectionCsv('tok', 'mtg', 'archidekt')).rejects.toBeInstanceOf(ApiError)
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

describe('importCollectionText', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('POSTs the pasted text verbatim with a text/plain content type and bearer token', async () => {
    type FetchInit = { method: string; headers: Record<string, string>; body: unknown }
    const fetchMock = vi.fn<(url: string, init: FetchInit) => Promise<Response>>(
      async () =>
        ({
          ok: true,
          status: 200,
          text: async () => JSON.stringify({ provider: 'mythictools', matched_cards: 2 }),
        }) as Response,
    )
    vi.stubGlobal('fetch', fetchMock)

    const list = '2 Sol Ring (C21) 263\n1 Counterspell\n'
    const summary = await importCollectionText('tok', 'mtg', list, 'overwrite')

    expect(summary).toEqual({ provider: 'mythictools', matched_cards: 2 })
    const [url, init] = fetchMock.mock.calls[0]!
    expect(url).toContain('/api/collection/mtg/import/text?mode=overwrite')
    expect(init.method).toBe('POST')
    expect(init.headers['Content-Type']).toBe('text/plain')
    expect(init.headers.Authorization).toBe('Bearer tok')
    // Sent as-is: the server sniffs the format, so the client must not reshape the paste.
    expect(init.body).toBe(list)
  })
})
