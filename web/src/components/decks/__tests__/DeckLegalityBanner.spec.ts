import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import type { DeckLegality } from '@/lib/legality'
import DeckLegalityBanner from '../DeckLegalityBanner.vue'

describe('DeckLegalityBanner', () => {
  it('renders a quiet success line when the deck has no issues', () => {
    const legality: DeckLegality = {
      formatKey: 'modern',
      formatLabel: 'Modern',
      issues: [],
      statusByCardId: new Map(),
      unknownCount: 0,
    }

    const wrapper = mount(DeckLegalityBanner, { props: { legality } })

    expect(wrapper.text()).toBe('No Modern legality issues')
    expect(wrapper.element.tagName).toBe('P')
    expect(wrapper.classes()).toContain('text-muted-foreground')
    expect(wrapper.classes()).not.toContain('rounded-lg')
    expect(wrapper.find('svg').classes()).toContain('text-emerald-600')
  })

  it('summarizes and lists mixed legality issues', () => {
    const legality: DeckLegality = {
      formatKey: 'vintage',
      formatLabel: 'Vintage',
      issues: [
        { cardId: 'black-lotus', name: 'Black Lotus', status: 'banned', quantity: 1 },
        { cardId: 'chaos-orb', name: 'Chaos Orb', status: 'banned', quantity: 1 },
        {
          cardId: 'expressive-iteration',
          name: 'Expressive Iteration',
          status: 'not_legal',
          quantity: 1,
        },
        { cardId: 'ancestral-recall', name: 'Ancestral Recall', status: 'restricted', quantity: 3 },
      ],
      statusByCardId: new Map([
        ['black-lotus', 'banned'],
        ['chaos-orb', 'banned'],
        ['expressive-iteration', 'not_legal'],
        ['ancestral-recall', 'restricted'],
      ]),
      unknownCount: 0,
    }

    const wrapper = mount(DeckLegalityBanner, { props: { legality } })
    const text = wrapper.text()

    expect(wrapper.classes()).toContain('border-red-500/40')
    expect(text).toContain('Not legal in Vintage')
    expect(text).toContain('2 banned, 1 not legal, 1 restricted over the 1-copy limit')
    expect(text).toContain('Black Lotus')
    expect(text).toContain('Chaos Orb')
    expect(text).toContain('Expressive Iteration')
    expect(text).toContain('Ancestral Recall')
    expect(text).toContain('Restricted · 3 copies')

    const chips = wrapper.findAll('li span:last-child')
    expect(chips.find((chip) => chip.text() === 'Banned')?.classes()).toContain('bg-red-500/15')
    expect(chips.find((chip) => chip.text() === 'Not Legal')?.classes()).toContain('bg-muted')
    expect(chips.find((chip) => chip.text().startsWith('Restricted'))?.classes()).toContain(
      'bg-amber-500/15',
    )
  })

  it('caps the visible issue list at eight cards', () => {
    const names = [
      'Alpha',
      'Bravo',
      'Charlie',
      'Delta',
      'Echo',
      'Foxtrot',
      'Golf',
      'Hotel',
      'India',
      'Juliet',
      'Kilo',
      'Lima',
    ]
    const legality: DeckLegality = {
      formatKey: 'standard',
      formatLabel: 'Standard',
      issues: names.map((name, index) => ({
        cardId: `card-${index}`,
        name,
        status: 'not_legal' as const,
        quantity: 1,
      })),
      statusByCardId: new Map(),
      unknownCount: 0,
    }

    const wrapper = mount(DeckLegalityBanner, { props: { legality } })
    const rows = wrapper.findAll('li')

    expect(rows).toHaveLength(8)
    expect(rows.map((row) => row.text().replace('Not Legal', '').trim())).toEqual(names.slice(0, 8))
    expect(wrapper.text()).toContain('…and 4 more')
    for (const hiddenName of names.slice(8)) expect(wrapper.text()).not.toContain(hiddenName)
  })

  it('uses correct singular wording for one issue', () => {
    const legality: DeckLegality = {
      formatKey: 'legacy',
      formatLabel: 'Legacy',
      issues: [{ cardId: 'contract', name: 'Contract from Below', status: 'banned', quantity: 1 }],
      statusByCardId: new Map([['contract', 'banned']]),
      unknownCount: 0,
    }

    const wrapper = mount(DeckLegalityBanner, { props: { legality } })

    expect(wrapper.findAll('p')[1]!.text()).toBe('1 banned')
    expect(wrapper.text()).not.toContain('1 banneds')
  })
})
