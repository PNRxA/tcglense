import { describe, it, expect } from 'vitest'

import { mount, RouterLinkStub } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import type { ArtTagEntry } from '@/lib/api'
import CardArtTags from '../CardArtTags.vue'

/** A tag entry with only the fields a test cares about spelled out. */
function tag(slug: string, count: number): ArtTagEntry {
  return { slug, label: slug, count, description: null }
}

async function mountArtTags(id: string, tags: ArtTagEntry[]) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  // Seed the cache so the tags are available synchronously (no network in tests).
  queryClient.setQueryData(['card-art-tags', 'mtg', id], { data: tags })
  return mount(CardArtTags, {
    props: { game: 'mtg', id },
    global: {
      plugins: [[VueQueryPlugin, { queryClient }]],
      stubs: { RouterLink: RouterLinkStub },
    },
  })
}

describe('CardArtTags', () => {
  it('shows the first tags and reveals the rest behind "show more"', async () => {
    // 12 tags: the first 10 show, the last 2 hide behind the toggle.
    const tags = Array.from({ length: 12 }, (_, i) => tag(`tag-${i}`, i + 1))
    const wrapper = await mountArtTags('dummy-dmb-0080', tags)

    expect(wrapper.text()).toContain('Artwork tags')
    expect(wrapper.findAllComponents(RouterLinkStub)).toHaveLength(10)
    expect(wrapper.text()).toContain('Show all tags (2 more)')
    expect(wrapper.text()).not.toContain('tag-11')

    await wrapper.get('button[aria-expanded]').trigger('click')
    expect(wrapper.findAllComponents(RouterLinkStub)).toHaveLength(12)
    expect(wrapper.text()).toContain('tag-11')
    expect(wrapper.text()).toContain('Show fewer tags')
  })

  it('offers no toggle when every tag already fits', async () => {
    const wrapper = await mountArtTags('dummy-dmb-0081', [tag('squirrel', 23), tag('rodent', 190)])
    expect(wrapper.findAllComponents(RouterLinkStub)).toHaveLength(2)
    expect(wrapper.find('button[aria-expanded]').exists()).toBe(false)
  })

  it('links each tag to its `art:` card search', async () => {
    const wrapper = await mountArtTags('dummy-dmb-0082', [tag('mountain-range', 400)])
    const link = wrapper.getComponent(RouterLinkStub)
    expect(link.props('to')).toEqual({
      path: '/cards/mtg/cards',
      query: { q: 'art:mountain-range' },
    })
  })

  it('renders nothing when the artwork has no tags', async () => {
    const wrapper = await mountArtTags('dummy-dmb-0001', [])
    expect(wrapper.text()).not.toContain('Artwork tags')
    expect(wrapper.find('div').exists()).toBe(false)
  })
})
