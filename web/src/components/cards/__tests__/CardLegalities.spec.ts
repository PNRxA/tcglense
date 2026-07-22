import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import type { Card } from '@/lib/api'
import { MTG_FORMATS } from '@/lib/legality'
import CardLegalities from '../CardLegalities.vue'

function cardWithLegalities(legalities: Card['legalities']): Card {
  return {
    id: 'card-1',
    name: 'Test Card',
    legalities,
  } as unknown as Card
}

function formatRow(wrapper: ReturnType<typeof mount>, key: string) {
  return wrapper.get(`[data-format="${key}"]`)
}

describe('CardLegalities', () => {
  it('renders nothing when the card has no legality data', () => {
    const wrapper = mount(CardLegalities, {
      props: { card: cardWithLegalities(null) },
    })

    expect(wrapper.find('div').exists()).toBe(false)
    expect(wrapper.text()).toBe('')
  })

  it('shows each known status beside its format with the matching tint', () => {
    const wrapper = mount(CardLegalities, {
      props: {
        card: cardWithLegalities({
          modern: 'legal',
          legacy: 'banned',
          vintage: 'restricted',
          standard: 'not_legal',
        }),
      },
    })

    const cases = [
      ['modern', 'Legal', 'Modern', 'bg-emerald-500/15', 'text-emerald-700'],
      ['legacy', 'Banned', 'Legacy', 'bg-red-500/15', 'text-red-700'],
      ['vintage', 'Restricted', 'Vintage', 'bg-amber-500/15', 'text-amber-700'],
      ['standard', 'Not Legal', 'Standard', 'bg-muted', 'text-muted-foreground'],
    ] as const

    for (const [key, status, label, backgroundClass, textClass] of cases) {
      const row = formatRow(wrapper, key)
      const spans = row.findAll('span')
      expect(spans[0]?.text()).toBe(status)
      expect(spans[0]?.classes()).toContain(backgroundClass)
      expect(spans[0]?.classes()).toContain(textClass)
      expect(spans[1]?.text()).toBe(label)
    }
  })

  it('shows a muted em dash for a format missing from the legality object', () => {
    const wrapper = mount(CardLegalities, {
      props: { card: cardWithLegalities({ modern: 'legal' }) },
    })

    const chip = formatRow(wrapper, 'pioneer').get('span')
    expect(chip.text()).toBe('—')
    expect(chip.classes()).toContain('bg-muted')
    expect(chip.classes()).toContain('text-muted-foreground')
  })

  it('renders every tracked format label', () => {
    const wrapper = mount(CardLegalities, {
      props: { card: cardWithLegalities({}) },
    })

    for (const format of MTG_FORMATS) {
      const labels = formatRow(wrapper, format.key).findAll('span')
      expect(labels[1]?.text()).toBe(format.label)
    }
  })
})
