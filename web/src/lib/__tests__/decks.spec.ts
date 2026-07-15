import { afterEach, describe, expect, it, vi } from 'vitest'

import {
  ApiError,
  deckExportPath,
  exportDeckFile,
  importDeck,
  type DeckImportRequest,
} from '../api'

describe('deck import/export API', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('POSTs a provider deck upload as the unified import body', async () => {
    type FetchInit = { method: string; headers: Record<string, string>; body: string }
    const fetchMock = vi.fn<(url: string, init: FetchInit) => Promise<Response>>(
      async () =>
        ({
          ok: true,
          status: 200,
          text: async () => JSON.stringify({ provider: 'archidekt', matched_cards: 2 }),
        }) as Response,
    )
    vi.stubGlobal('fetch', fetchMock)
    const body: DeckImportRequest = {
      provider: 'archidekt',
      source: null,
      contents: 'Quantity,Scryfall ID,Categories\n1,abc,Mainboard\n',
      format: 'csv',
      name: 'Imported',
    }

    await importDeck('tok', 'mtg', body)

    const [url, init] = fetchMock.mock.calls[0]!
    expect(url).toContain('/api/decks/mtg/import')
    expect(init.method).toBe('POST')
    expect(init.headers.Authorization).toBe('Bearer tok')
    expect(JSON.parse(init.body)).toEqual(body)
  })

  it('builds an encoded deck export path', () => {
    expect(deckExportPath('a/b', 42, 'moxfield-text')).toBe(
      '/api/decks/a%2Fb/42/export?format=moxfield-text',
    )
  })

  it('downloads a deck export with bearer auth and surfaces failures', async () => {
    const csv = new Blob(['Count,Name\n1,Card\n'], { type: 'text/csv' })
    type FetchInit = { headers: Record<string, string> }
    const fetchMock = vi.fn<(url: string, init: FetchInit) => Promise<Response>>(
      async () => ({ ok: true, status: 200, blob: async () => csv }) as Response,
    )
    vi.stubGlobal('fetch', fetchMock)

    await expect(exportDeckFile('tok', 'mtg', 42, 'moxfield')).resolves.toBe(csv)
    expect(fetchMock.mock.calls[0]![0]).toContain('/api/decks/mtg/42/export?format=moxfield')
    expect(fetchMock.mock.calls[0]![1].headers.Authorization).toBe('Bearer tok')

    fetchMock.mockResolvedValueOnce({ ok: false, status: 404 } as Response)
    await expect(exportDeckFile('tok', 'mtg', 42, 'archidekt')).rejects.toBeInstanceOf(ApiError)
  })
})
