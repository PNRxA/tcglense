import { describe, it, expect } from 'vitest'

import { mount } from '@vue/test-utils'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import type { Ruling } from '@/lib/api'
import CardRulings from '../CardRulings.vue'

async function mountRulings(id: string, rulings: Ruling[]) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  // Seed the cache so the rulings are available synchronously (no network in tests).
  queryClient.setQueryData(['card-rulings', 'mtg', id], { data: rulings })
  return mount(CardRulings, {
    props: { game: 'mtg', id },
    global: { plugins: [[VueQueryPlugin, { queryClient }]] },
  })
}

describe('CardRulings', () => {
  it('shows the rulings expanded by default, with the count on the header', async () => {
    const wrapper = await mountRulings('dummy-dmb-0080', [
      { source: 'wotc', published_at: '2019-08-23', comment: 'The older ruling.' },
      { source: 'scryfall', published_at: '2020-01-01', comment: 'A Scryfall note.' },
    ])
    // Open on arrival: the rulings ARE the card's rules text, so they're readable
    // without a click. Each comment, a friendly source label, and the date are shown.
    expect(wrapper.text()).toContain('Notes and Rules Information (2)')
    expect(wrapper.get('button[aria-expanded]').attributes('aria-expanded')).toBe('true')
    expect(wrapper.text()).toContain('The older ruling.')
    expect(wrapper.text()).toContain('A Scryfall note.')
    expect(wrapper.text()).toContain('Wizards of the Coast')
    expect(wrapper.text()).toContain('2019-08-23')

    // …and the header still collapses them away.
    await wrapper.get('button[aria-expanded]').trigger('click')
    expect(wrapper.text()).not.toContain('The older ruling.')
  })

  it('renders nothing when the card has no rulings', async () => {
    const wrapper = await mountRulings('dummy-dmb-0001', [])
    expect(wrapper.find('section').exists()).toBe(false)
    expect(wrapper.text()).not.toContain('Notes and Rules Information')
  })
})
