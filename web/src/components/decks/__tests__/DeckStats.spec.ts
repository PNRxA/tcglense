import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import DeckStats from '@/components/decks/DeckStats.vue'
import type { Card, DeckCardEntry, DeckSection } from '@/lib/api'

function entry(sectionId: number, id: string, name: string, quantity: number): DeckCardEntry {
  return {
    section_id: sectionId,
    quantity,
    foil_quantity: 0,
    card: {
      id,
      name,
      color_identity: ['R'],
      cmc: 1,
      type_line: 'Instant',
    } as Card,
  }
}

describe('DeckStats draw sections', () => {
  it('excludes sideboards by default and lets the viewer include them', async () => {
    const sections: DeckSection[] = [
      { id: 1, name: 'Mainboard', position: 0 },
      { id: 2, name: 'Sideboard', position: 1 },
    ]
    const wrapper = mount(DeckStats, {
      props: {
        sections,
        cards: [entry(1, 'bolt', 'Lightning Bolt', 60), entry(2, 'blast', 'Pyroblast', 15)],
      },
    })

    expect(wrapper.text()).toContain('60 cards from 1 selected section')
    const checkboxes = wrapper.findAll<HTMLInputElement>('input[type="checkbox"]')
    expect(checkboxes).toHaveLength(2)
    expect(checkboxes[0]!.element.checked).toBe(true)
    expect(checkboxes[1]!.element.checked).toBe(false)

    await checkboxes[1]!.setValue(true)
    expect(wrapper.text()).toContain('75 cards from 2 selected sections')
  })
})
