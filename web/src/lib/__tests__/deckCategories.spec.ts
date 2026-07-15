import { describe, expect, it } from 'vitest'
import { automaticDeckSection, presetDeckSection } from '../deckCategories'

const sections = [
  { id: 1, name: 'Commander', position: 0 },
  { id: 2, name: 'Creatures', position: 1 },
  { id: 3, name: 'Lands', position: 2 },
]

describe('deck category defaults', () => {
  it('uses the most useful front-face type bucket', () => {
    expect(presetDeckSection({ type_line: 'Artifact Creature — Golem' })).toBe('Creatures')
    expect(presetDeckSection({ type_line: 'Sorcery // Land' })).toBe('Sorceries')
    expect(presetDeckSection({ type_line: 'Basic Land — Island' })).toBe('Lands')
  })

  it('falls back to the deck first section when its preset is absent', () => {
    expect(automaticDeckSection({ type_line: 'Creature — Cat' }, sections)?.id).toBe(2)
    expect(automaticDeckSection({ type_line: 'Instant' }, sections)?.id).toBe(1)
    expect(automaticDeckSection({ type_line: null }, sections)?.id).toBe(1)
  })
})
