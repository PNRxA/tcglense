import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount } from '@vue/test-utils'
import CardExportMenu from '../CardExportMenu.vue'
import { ApiError } from '@/lib/api'

const exportCards = vi.fn<(...args: unknown[]) => Promise<Blob>>()
const exportSetCards = vi.fn<(...args: unknown[]) => Promise<Blob>>()
const downloadBlob = vi.fn<(...args: unknown[]) => void>()

vi.mock('@/lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api')>()
  return {
    ...actual,
    exportCards: (...args: unknown[]) => exportCards(...args),
    exportSetCards: (...args: unknown[]) => exportSetCards(...args),
  }
})

vi.mock('@/lib/download', () => ({
  downloadBlob: (...args: unknown[]) => downloadBlob(...args),
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

  it('disables the trigger when there is nothing to export', () => {
    expect(mountMenu({ disabled: true }).get('button').attributes('disabled')).toBeDefined()
    expect(mountMenu().get('button').attributes('disabled')).toBeUndefined()
  })
})
