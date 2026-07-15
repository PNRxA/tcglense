import { describe, expect, it } from 'vitest'
import { automaticDeckSection, presetDeckSection } from '../deckCategories'

const sections = [
  { id: 1, name: 'Commander', position: 0 },
  { id: 2, name: 'Creatures', position: 1 },
  { id: 3, name: 'Lands', position: 2 },
]

describe('deck category defaults', () => {
  it('uses the most useful front-face type bucket', () => {
    expect(presetDeckSection({ type_line: 'Basic Land — Island' })).toBe('Lands')
    expect(presetDeckSection({ type_line: 'Artifact Creature — Golem' })).toBe('Creatures')
    expect(presetDeckSection({ type_line: 'Planeswalker' })).toBe('Planeswalkers')
    expect(presetDeckSection({ type_line: 'Instant' })).toBe('Instants')
    expect(presetDeckSection({ type_line: 'Sorcery // Land' })).toBe('Sorceries')
    expect(presetDeckSection({ type_line: 'Enchantment' })).toBe('Enchantments')
    expect(presetDeckSection({ type_line: 'Artifact' })).toBe('Artifacts')
    expect(presetDeckSection({ type_line: 'Battle — Siege' })).toBeNull()
  })

  it('requires a safe catch-all or an explicit section when its preset is absent', () => {
    expect(automaticDeckSection({ type_line: 'Creature — Cat' }, sections)?.id).toBe(2)
    expect(automaticDeckSection({ type_line: 'Instant' }, sections)).toBeUndefined()
    expect(automaticDeckSection({ type_line: null }, sections)).toBeUndefined()

    const withMainboard = [...sections, { id: 4, name: 'Mainboard', position: 3 }]
    expect(automaticDeckSection({ type_line: 'Battle — Siege' }, withMainboard)?.id).toBe(4)
  })
})
