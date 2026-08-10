import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import type { DeckLegality } from '@/lib/api'
import DeckLegalityBanner from '../DeckLegalityBanner.vue'

/** A clean verdict, so each case spells out only what it asserts on. */
function makeLegality(over: Partial<DeckLegality> = {}): DeckLegality {
  const legality: DeckLegality = {
    format_key: 'modern',
    format_label: 'Modern',
    issues: [],
    violations: [],
    card_statuses: {},
    unknown_count: 0,
    legal: true,
    ...over,
  }
  // `legal` is derived server-side; keep the fixture honest unless a case
  // overrides it explicitly.
  return 'legal' in over
    ? legality
    : {
        ...legality,
        legal:
          legality.issues.length === 0 &&
          !legality.violations.some((violation) => violation.severity === 'error'),
      }
}

describe('DeckLegalityBanner', () => {
  it('renders a quiet success line when the deck has no issues', () => {
    const wrapper = mount(DeckLegalityBanner, { props: { legality: makeLegality() } })

    expect(wrapper.text()).toBe('No Modern legality issues')
    expect(wrapper.element.tagName).toBe('P')
    expect(wrapper.classes()).toContain('text-muted-foreground')
    expect(wrapper.classes()).not.toContain('rounded-lg')
    expect(wrapper.find('svg').classes()).toContain('text-success')
  })

  it('summarizes and lists mixed legality issues', () => {
    const legality = makeLegality({
      format_key: 'vintage',
      format_label: 'Vintage',
      issues: [
        { card_id: 'black-lotus', name: 'Black Lotus', status: 'banned', quantity: 1 },
        { card_id: 'chaos-orb', name: 'Chaos Orb', status: 'banned', quantity: 1 },
        {
          card_id: 'expressive-iteration',
          name: 'Expressive Iteration',
          status: 'not_legal',
          quantity: 1,
        },
        {
          card_id: 'ancestral-recall',
          name: 'Ancestral Recall',
          status: 'restricted',
          quantity: 3,
        },
      ],
      card_statuses: {
        'black-lotus': 'banned',
        'chaos-orb': 'banned',
        'expressive-iteration': 'not_legal',
        'ancestral-recall': 'restricted',
      },
    })

    const wrapper = mount(DeckLegalityBanner, { props: { legality } })
    const text = wrapper.text()

    expect(wrapper.classes()).toContain('border-destructive/40')
    expect(text).toContain('Not legal in Vintage')
    expect(text).toContain('2 banned, 1 not legal, 1 restricted over the 1-copy limit')
    expect(text).toContain('Black Lotus')
    expect(text).toContain('Chaos Orb')
    expect(text).toContain('Expressive Iteration')
    expect(text).toContain('Ancestral Recall')
    expect(text).toContain('Restricted · 3 copies')

    const chips = wrapper.findAll('li span:last-child')
    expect(chips.find((chip) => chip.text() === 'Banned')?.classes()).toContain('bg-destructive/15')
    expect(chips.find((chip) => chip.text() === 'Not Legal')?.classes()).toContain('bg-muted')
    expect(chips.find((chip) => chip.text().startsWith('Restricted'))?.classes()).toContain(
      'bg-warning/15',
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
    const legality = makeLegality({
      format_key: 'standard',
      format_label: 'Standard',
      issues: names.map((name, index) => ({
        card_id: `card-${index}`,
        name,
        status: 'not_legal' as const,
        quantity: 1,
      })),
    })

    const wrapper = mount(DeckLegalityBanner, { props: { legality } })
    const rows = wrapper.findAll('li')

    expect(rows).toHaveLength(8)
    expect(rows.map((row) => row.text().replace('Not Legal', '').trim())).toEqual(names.slice(0, 8))
    expect(wrapper.text()).toContain('…and 4 more')
    for (const hiddenName of names.slice(8)) expect(wrapper.text()).not.toContain(hiddenName)
  })

  it('uses correct singular wording for one issue', () => {
    const legality = makeLegality({
      format_key: 'legacy',
      format_label: 'Legacy',
      issues: [{ card_id: 'contract', name: 'Contract from Below', status: 'banned', quantity: 1 }],
      card_statuses: { contract: 'banned' },
    })

    const wrapper = mount(DeckLegalityBanner, { props: { legality } })

    expect(wrapper.findAll('p')[1]!.text()).toBe('1 banned')
    expect(wrapper.text()).not.toContain('1 banneds')
  })

  it('stays on the warning tint and calls an under-built deck in progress, not illegal', () => {
    const legality = makeLegality({
      format_key: 'commander',
      format_label: 'Commander',
      violations: [
        { rule: 'deck-size', severity: 'warning', message: '63 of 100 cards — 37 to go.' },
        {
          rule: 'command-zone',
          severity: 'warning',
          message: 'No commander — put one in a section named "Commander".',
        },
      ],
    })

    const wrapper = mount(DeckLegalityBanner, { props: { legality } })
    const text = wrapper.text()

    expect(wrapper.classes()).toContain('border-warning/40')
    expect(wrapper.classes()).not.toContain('border-destructive/40')
    expect(text).toContain('Commander deck in progress')
    expect(text).not.toContain('Not legal in')
    expect(text).toContain('63 of 100 cards — 37 to go.')
    expect(text).toContain('No commander')
  })

  it('lists construction breaches above the offending cards, errors first', () => {
    const legality = makeLegality({
      format_key: 'commander',
      format_label: 'Commander',
      issues: [{ card_id: 'bolt', name: 'Lightning Bolt', status: 'off_colour', quantity: 1 }],
      violations: [
        { rule: 'deck-size', severity: 'warning', message: '99 of 100 cards — 1 to go.' },
        {
          rule: 'colour-identity',
          severity: 'error',
          message: "1 card falls outside Atraxa's colour identity ({W}{U}{B}{G}).",
        },
      ],
      card_statuses: { bolt: 'off_colour' },
    })

    const wrapper = mount(DeckLegalityBanner, { props: { legality } })

    expect(wrapper.classes()).toContain('border-destructive/40')
    expect(wrapper.text()).toContain('Not legal in Commander')
    expect(wrapper.text()).toContain("1 outside the commander's colour identity")
    const messages = wrapper.findAll('li').map((row) => row.text())
    expect(messages[0]).toContain("outside Atraxa's colour identity")
    expect(messages[1]).toContain('99 of 100 cards')
    expect(messages[2]).toContain('Lightning Bolt')
    expect(messages[2]).toContain('Off Colour')
  })
})
