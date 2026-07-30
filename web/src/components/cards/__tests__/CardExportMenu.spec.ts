import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount } from '@vue/test-utils'
import CardExportMenu from '../CardExportMenu.vue'
import { ApiError } from '@/lib/api'

const exportCards = vi.fn<(...args: unknown[]) => Promise<Blob>>()
const exportSetCards = vi.fn<(...args: unknown[]) => Promise<Blob>>()
const exportCollectionCards = vi.fn<(...args: unknown[]) => Promise<Blob>>()
const exportWishlistCards = vi.fn<(...args: unknown[]) => Promise<Blob>>()
const downloadBlob = vi.fn<(...args: unknown[]) => void>()

vi.mock('@/lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api')>()
  return {
    ...actual,
    exportCards: (...args: unknown[]) => exportCards(...args),
    exportSetCards: (...args: unknown[]) => exportSetCards(...args),
    exportCollectionCards: (...args: unknown[]) => exportCollectionCards(...args),
    exportWishlistCards: (...args: unknown[]) => exportWishlistCards(...args),
  }
})

vi.mock('@/lib/download', () => ({
  downloadBlob: (...args: unknown[]) => downloadBlob(...args),
}))

// The holdings mode goes through the auth store's authFetch (per-user download); hand
// the exporter a token straight away so no Pinia instance is needed.
vi.mock('@/stores/auth', () => ({
  useAuthStore: () => ({
    authFetch: (call: (token: string) => Promise<Blob>) => call('tok'),
  }),
}))

const blob = new Blob(['1 Sol Ring (LTC) 284\n'], { type: 'text/plain' })

function mountMenu(props: Partial<InstanceType<typeof CardExportMenu>['$props']> = {}) {
  return mount(CardExportMenu, {
    props: { game: 'mtg', sort: 'name:asc', defaultSort: 'name:asc', ...props },
    attachTo: document.body,
  })
}

/** Open the dropdown and click the item whose label starts with `label`. */
async function pick(wrapper: ReturnType<typeof mountMenu>, label: string) {
  await wrapper.get('button').trigger('click')
  await new Promise((resolve) => setTimeout(resolve, 0))
  const item = Array.from(document.body.querySelectorAll<HTMLElement>('[role="menuitem"]')).find(
    (node) => node.textContent?.includes(label),
  )
  if (!item) throw new Error(`no menu item matching "${label}"`)
  item.click()
  await new Promise((resolve) => setTimeout(resolve, 0))
  await wrapper.vm.$nextTick()
}

describe('CardExportMenu', () => {
  beforeEach(() => {
    exportCards.mockReset().mockResolvedValue(blob)
    exportSetCards.mockReset().mockResolvedValue(blob)
    exportCollectionCards.mockReset().mockResolvedValue(blob)
    exportWishlistCards.mockReset().mockResolvedValue(blob)
    downloadBlob.mockReset()
  })
  afterEach(() => document.body.replaceChildren())

  it('exports the all-cards search with the view’s query and sort', async () => {
    const wrapper = mountMenu({ query: 'c:r t:goblin', sort: 'price:desc' })
    await pick(wrapper, 'Card list')

    expect(exportSetCards).not.toHaveBeenCalled()
    expect(exportCards).toHaveBeenCalledWith('mtg', {
      q: 'c:r t:goblin',
      sort: 'price',
      dir: 'desc',
      // Vue defaults an absent Boolean prop to false; the path builder drops it either way.
      includeRelated: false,
      format: 'text',
    })
    expect(downloadBlob).toHaveBeenCalledWith(blob, 'tcglense-mtg-cards.txt')
  })

  it('routes to the set endpoint and filename when scoped to a set', async () => {
    const wrapper = mountMenu({ setCode: 'neo', includeRelated: true })
    await pick(wrapper, 'Card names')

    expect(exportCards).not.toHaveBeenCalled()
    expect(exportSetCards).toHaveBeenCalledWith(
      'mtg',
      'neo',
      expect.objectContaining({ includeRelated: true, format: 'names' }),
    )
    expect(downloadBlob).toHaveBeenCalledWith(blob, 'tcglense-mtg-neo-card-names.txt')
  })

  it('exports the collection listing with the token and set scope', async () => {
    const wrapper = mountMenu({ list: 'collection', setCode: 'neo', sort: 'updated:desc' })
    await pick(wrapper, 'Card list')

    // The holdings surface never touches the catalog endpoints...
    expect(exportCards).not.toHaveBeenCalled()
    expect(exportSetCards).not.toHaveBeenCalled()
    // ...it downloads the user's own listing (authed), with the set as a query scope.
    expect(exportCollectionCards).toHaveBeenCalledWith('tok', 'mtg', {
      q: undefined,
      sort: 'updated',
      dir: 'desc',
      includeRelated: false,
      format: 'text',
      set: 'neo',
    })
    // The filename mirrors the server's scope-free holdings name.
    expect(downloadBlob).toHaveBeenCalledWith(blob, 'tcglense-mtg-collection-cards.txt')
  })

  it('describes holdings lines as carrying your counts', async () => {
    const wrapper = mountMenu({ list: 'collection' })
    await wrapper.get('button').trigger('click')
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(document.body.textContent).toContain('with your counts')
  })

  it('exports the wish-list listing through the wish-list twin', async () => {
    const wrapper = mountMenu({ list: 'wishlist' })
    await pick(wrapper, 'Card names')

    expect(exportCollectionCards).not.toHaveBeenCalled()
    expect(exportWishlistCards).toHaveBeenCalledWith(
      'tok',
      'mtg',
      expect.objectContaining({ format: 'names', set: undefined }),
    )
    expect(downloadBlob).toHaveBeenCalledWith(blob, 'tcglense-mtg-wishlist-card-names.txt')
  })

  it('drops an empty search rather than sending a blank q', async () => {
    const wrapper = mountMenu({ query: '' })
    await pick(wrapper, 'Card list')
    expect(exportCards).toHaveBeenCalledWith('mtg', expect.objectContaining({ q: undefined }))
  })

  it('surfaces a failed export inline and never triggers a download', async () => {
    exportCards.mockRejectedValue(new ApiError('unknown filter', 422))
    const wrapper = mountMenu({ query: 'bogus:1' })
    await pick(wrapper, 'Card list')

    expect(downloadBlob).not.toHaveBeenCalled()
    expect(wrapper.text()).toContain('unknown filter')
  })

  it('falls back to generic copy for a non-API failure', async () => {
    exportCards.mockRejectedValue(new Error('offline'))
    const wrapper = mountMenu()
    await pick(wrapper, 'Card list')

    expect(wrapper.text()).toContain('Export failed. Please try again.')
  })

  it('stays quiet about size for an ordinary search', async () => {
    const wrapper = mountMenu({ total: 42 })
    await wrapper.get('button').trigger('click')
    await new Promise((resolve) => setTimeout(resolve, 0))
    // Nothing is capped or withheld, so there is nothing to disclose.
    expect(document.body.textContent).not.toContain('may take a moment')
  })

  it('flags a large export with the real match count', async () => {
    const wrapper = mountMenu({ total: 47_231 })
    await wrapper.get('button').trigger('click')
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(document.body.textContent).toContain(
      'Exporting all 47,231 matches — this may take a moment.',
    )
  })

  it('says nothing when the count is unknown', async () => {
    // The set view's grouped modes count *groups*, so they pass no total rather than a
    // wrong one — better silent than warning off a number we don't have.
    const wrapper = mountMenu({ total: undefined })
    await wrapper.get('button').trigger('click')
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(document.body.textContent).not.toContain('may take a moment')
  })

  it('exports a large result set anyway — the note informs, it does not block', async () => {
    const wrapper = mountMenu({ total: 47_231 })
    await pick(wrapper, 'Card list')
    expect(exportCards).toHaveBeenCalled()
    expect(downloadBlob).toHaveBeenCalled()
  })

  it('disables the trigger when there is nothing to export', () => {
    expect(mountMenu({ disabled: true }).get('button').attributes('disabled')).toBeDefined()
    expect(mountMenu().get('button').attributes('disabled')).toBeUndefined()
  })
})
