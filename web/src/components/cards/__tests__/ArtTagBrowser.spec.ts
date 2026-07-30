import { describe, it, expect, afterEach } from 'vitest'

import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import type { ArtTagEntry } from '@/lib/api'
import ArtTagBrowser from '../ArtTagBrowser.vue'

function tag(slug: string, count = 1): ArtTagEntry {
  return { slug, label: slug, count, description: null }
}

let wrapper: VueWrapper | undefined

// The dialog content is teleported to <body>, which persists across tests — unmount so one
// case's markup can't be read as the next one's.
afterEach(() => {
  wrapper?.unmount()
  wrapper = undefined
})

/** Mount the (open) browser with `tags` already in the cache — no network in tests. */
async function mountBrowser(tags: ArtTagEntry[]) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  queryClient.setQueryData(['art-tags-all', 'mtg'], { data: tags })
  wrapper = mount(ArtTagBrowser, {
    props: { game: 'mtg', modelValue: '', open: true },
    global: { plugins: [[VueQueryPlugin, { queryClient }]] },
  })
  await flushPromises()
  return wrapper
}

/** Text of the teleported dialog. */
function dialogText(): string {
  return document.body.textContent ?? ''
}

function dialogButton(label: string): HTMLElement | undefined {
  return [...document.body.querySelectorAll('button')].find((b) => b.textContent?.includes(label))
}

describe('ArtTagBrowser', () => {
  it('lists the vocabulary with each tag artwork count', async () => {
    await mountBrowser([tag('bear', 12), tag('squirrel', 228)])
    expect(dialogText()).toContain('squirrel')
    expect(dialogText()).toContain('228')
    expect(dialogText()).toContain('bear')
  })

  it('reports an unimported vocabulary as such, not as an unmatched filter', async () => {
    // The server returns an empty list until the art-tag dataset has been imported; the
    // filter box is untouched here, so blaming a search term would send the reader hunting
    // for a typo they never made.
    await mountBrowser([])
    expect(dialogText()).toContain('No art tags have been imported yet')
    expect(dialogText()).not.toContain('No tags match')
  })

  it('blames the filter only when a filter actually excluded everything', async () => {
    await mountBrowser([tag('squirrel')])
    const filter = document.body.querySelector<HTMLInputElement>(
      'input[aria-label="Filter art tags"]',
    )
    expect(filter).toBeTruthy()
    filter!.value = 'dragon'
    filter!.dispatchEvent(new Event('input'))
    await flushPromises()

    expect(dialogText()).toContain('No tags match "dragon"')
    expect(dialogText()).not.toContain('No art tags have been imported yet')
  })

  it('toggles a clicked tag into the shared query as an `art:` token', async () => {
    const browser = await mountBrowser([tag('squirrel')])
    dialogButton('squirrel')?.click()
    await flushPromises()
    expect(browser.emitted('update:modelValue')?.slice(-1)[0]).toEqual(['art:squirrel'])
  })
})
