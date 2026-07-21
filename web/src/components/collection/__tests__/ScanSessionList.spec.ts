import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import type { Card } from '@/lib/api'
import type { SessionEntry } from '@/composables/useScanSession'
import ScanSessionList from '../ScanSessionList.vue'

function makeCard(id: string): Card {
  return {
    id,
    name: `Card ${id}`,
    set_code: 'tst',
    set_name: 'Test Set',
    collector_number: id,
    rarity: 'rare',
    lang: 'en',
    released_at: '2024-01-01',
    mana_cost: null,
    cmc: 0,
    type_line: 'Artifact',
    oracle_text: null,
    power: null,
    toughness: null,
    loyalty: null,
    color_identity: [],
    colors: [],
    layout: 'normal',
    prices: { usd: null, usd_foil: null, eur: null, tix: null },
    has_image: false,
    drop_name: null,
    drop_slug: null,
    secret_lair_bonus: false,
    secret_lair_spend_incentive: false,
    faces: [],
  }
}

function makeEntry(id: number): SessionEntry {
  return {
    id,
    card: makeCard(String(id)),
    quantity: id,
    foil_quantity: id === 1 ? 1 : 0,
    previous: { quantity: id - 1, foil_quantity: 0 },
  }
}

describe('ScanSessionList', () => {
  it('keeps long sessions compact and preserves each visible entry index for undo', async () => {
    const wrapper = mount(ScanSessionList, {
      props: {
        game: 'mtg',
        entries: [1, 2, 3, 4, 5].map(makeEntry),
        disabled: false,
      },
      global: { stubs: { CardImage: true } },
    })

    expect(wrapper.findAll('li')).toHaveLength(3)
    expect(wrapper.text()).toContain('Now 1 regular')
    expect(wrapper.text()).toContain('View all (5)')
    const disclosure = wrapper.findAll('button').find((button) => button.text() === 'View all (5)')!
    expect(disclosure.attributes('aria-expanded')).toBe('false')
    expect(disclosure.attributes('aria-controls')).toBe(wrapper.get('ul').attributes('id'))

    await disclosure.trigger('click')
    expect(wrapper.findAll('li')).toHaveLength(5)
    expect(disclosure.attributes('aria-expanded')).toBe('true')

    await wrapper.findAll('button[aria-label^="Undo adding"]')[1]!.trigger('click')
    expect(wrapper.emitted('undo')).toEqual([[1]])
  })
})
