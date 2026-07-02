import { describe, it, expect } from 'vitest'

import { mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { makeCardSet } from '@/test/fixtures'
import type { SetGroup as SetGroupModel } from '@/lib/setGroups'
import SetGroup from '../SetGroup.vue'

// Ixalan with a related "Jurassic World" sub-set (and its tokens) — the shape where a
// filter can match a sub-set but not the main set (issue #128 / #149).
const ixalan: SetGroupModel = {
  main: makeCardSet('xln', { name: 'Ixalan' }),
  children: [
    makeCardSet('rex', { name: 'Jurassic World Collection', parent_set_code: 'xln' }),
    makeCardSet('trex', { name: 'Jurassic World Tokens', parent_set_code: 'xln' }),
  ],
}

// SetGroup renders SetTile (RouterLinks) + a "View all" RouterLink, so the tree needs a
// router; the fixture's null icon_svg_uri keeps every lazy <img> off (nothing network-facing).
function mountGroup(props: { group?: SetGroupModel; query?: string } = {}) {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:pathMatch(.*)*', component: { template: '<div />' } }],
  })
  return mount(SetGroup, {
    props: { game: 'mtg', group: ixalan, ...props },
    global: { plugins: [router] },
  })
}

// The related-sub-set <ul> only renders while the group is expanded, so its presence is a
// direct read of the expanded state.
const subSetList = (wrapper: ReturnType<typeof mountGroup>) =>
  wrapper.find('ul[aria-label="Sets related to Ixalan"]')
const toggle = (wrapper: ReturnType<typeof mountGroup>) => wrapper.find('button[aria-expanded]')

describe('SetGroup related-sets dropdown', () => {
  it('is collapsed by default when there is no active filter', () => {
    const wrapper = mountGroup()
    expect(subSetList(wrapper).exists()).toBe(false)
    expect(wrapper.text()).toContain('Show 2 related sets')
  })

  it('auto-opens when the filter matches a related sub-set (issue #149)', () => {
    const wrapper = mountGroup({ query: 'jurassic' })
    expect(subSetList(wrapper).exists()).toBe(true)
    expect(wrapper.text()).toContain('Hide 2 related sets')
  })

  it('auto-opens on a related sub-set code match too', () => {
    expect(subSetList(mountGroup({ query: 'trex' })).exists()).toBe(true)
  })

  it('opens when the filter starts matching a sub-set after mount', async () => {
    const wrapper = mountGroup({ query: '' })
    expect(subSetList(wrapper).exists()).toBe(false)
    await wrapper.setProps({ query: 'jurassic' })
    expect(subSetList(wrapper).exists()).toBe(true)
  })

  it('stays collapsed when the filter matches only the main set', () => {
    expect(subSetList(mountGroup({ query: 'ixalan' })).exists()).toBe(false)
    expect(subSetList(mountGroup({ query: 'xln' })).exists()).toBe(false)
  })

  it('lets the user collapse it by hand even while the filter matches (additive only)', async () => {
    const wrapper = mountGroup({ query: 'jurassic' })
    expect(subSetList(wrapper).exists()).toBe(true)
    await toggle(wrapper).trigger('click')
    expect(subSetList(wrapper).exists()).toBe(false)
  })

  it('still expands on a manual toggle with no active filter', async () => {
    const wrapper = mountGroup()
    expect(subSetList(wrapper).exists()).toBe(false)
    await toggle(wrapper).trigger('click')
    expect(subSetList(wrapper).exists()).toBe(true)
  })
})
