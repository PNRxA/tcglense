import { describe, it, expect } from 'vitest'

import { mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { CardSet } from '@/lib/api'
import { makeCardSet } from '@/test/fixtures'
import SetTile from '../SetTile.vue'

// This spec's assertions ('142/281 owned') depend on these defaults, so keep them local
// rather than exposing the tile tests to shared-fixture default drift.
const makeSet = (over: Partial<CardSet> = {}): CardSet =>
  makeCardSet('blb', { name: 'Bloomburrow', card_count: 281, ...over })

// SetTile renders a RouterLink, so the tree needs a router; no icon_svg_uri keeps the
// lazy <img> off, so nothing network-facing is exercised.
function mountTile(props: {
  set: CardSet
  ownedCount?: number
  ownedCopies?: number
  ownedValue?: string | null
  bulkValue?: string | null
}) {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:pathMatch(.*)*', component: { template: '<div />' } }],
  })
  return mount(SetTile, { props: { game: 'mtg', ...props }, global: { plugins: [router] } })
}

describe('SetTile owned-count line', () => {
  it('shows a set-completion "N/M owned" count when an owned count is passed', () => {
    const wrapper = mountTile({ set: makeSet(), ownedCount: 142 })
    expect(wrapper.text()).toContain('142/281 owned')
    // The completion count replaces the catalog "M cards" line.
    expect(wrapper.text()).not.toContain('281 cards')
  })

  it('clamps the owned count to the set total so it never reads "N+1 of N"', () => {
    const wrapper = mountTile({ set: makeSet({ card_count: 100 }), ownedCount: 130 })
    expect(wrapper.text()).toContain('100/100 owned')
  })

  it('falls back to a plain "N owned" when the set total is unknown (card_count 0)', () => {
    const wrapper = mountTile({ set: makeSet({ card_count: 0 }), ownedCount: 5 })
    expect(wrapper.text()).toContain('5 owned')
    expect(wrapper.text()).not.toContain('5/0')
  })

  it('appends "N copies" when more copies are owned than distinct cards', () => {
    const wrapper = mountTile({ set: makeSet(), ownedCount: 142, ownedCopies: 180 })
    expect(wrapper.text()).toContain('142/281 owned')
    expect(wrapper.text()).toContain('180 copies')
  })

  it('omits the copies count when every owned card is a single copy', () => {
    const wrapper = mountTile({ set: makeSet(), ownedCount: 142, ownedCopies: 142 })
    expect(wrapper.text()).toContain('142/281 owned')
    expect(wrapper.text()).not.toContain('copies')
  })

  it('shows the total owned value labelled "Total" when one is passed', () => {
    const wrapper = mountTile({ set: makeSet(), ownedCount: 142, ownedValue: '$412.00' })
    expect(wrapper.text()).toContain('142/281 owned')
    expect(wrapper.text()).toContain('Total')
    expect(wrapper.text()).toContain('$412.00')
  })

  it('shows the bulk (< $1) value labelled "Bulk" when one is passed', () => {
    const wrapper = mountTile({
      set: makeSet(),
      ownedCount: 142,
      ownedValue: '$412.00',
      bulkValue: '$12.30',
    })
    expect(wrapper.text()).toContain('Bulk')
    expect(wrapper.text()).toContain('$12.30')
  })

  it('omits the value labels in catalog use (no owned/bulk value passed)', () => {
    const wrapper = mountTile({ set: makeSet() })
    expect(wrapper.text()).toContain('281 cards')
    expect(wrapper.text()).not.toContain('owned')
    expect(wrapper.text()).not.toContain('Total')
    expect(wrapper.text()).not.toContain('Bulk')
  })
})
