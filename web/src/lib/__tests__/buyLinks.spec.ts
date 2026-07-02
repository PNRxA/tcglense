import { describe, it, expect } from 'vitest'

import { buildSearchUrl, buyLinksFor, searchName } from '../buyLinks'

const singleFaced = { name: "Gaea's Cradle", faces: [] }
const doubleFaced = {
  name: 'Fable of the Mirror-Breaker // Reflection of Kiki-Jiki',
  faces: [{ name: 'Fable of the Mirror-Breaker' }, { name: 'Reflection of Kiki-Jiki' }],
}

describe('buildSearchUrl', () => {
  it('substitutes the percent-encoded card name into the template', () => {
    expect(buildSearchUrl('https://example.com/search?q={name}', "Gaea's Cradle")).toBe(
      "https://example.com/search?q=Gaea's%20Cradle",
    )
  })
})

describe('searchName', () => {
  it('uses the card name for a single-faced card', () => {
    expect(searchName(singleFaced)).toBe("Gaea's Cradle")
  })

  it('uses the front face name for a multi-faced card', () => {
    expect(searchName(doubleFaced)).toBe('Fable of the Mirror-Breaker')
  })
})

describe('buyLinksFor', () => {
  it('returns nothing for a game with no store registry', () => {
    expect(buyLinksFor('unknown-game', singleFaced)).toEqual([])
  })

  it('returns the Global and Australia sections for mtg', () => {
    const sections = buyLinksFor('mtg', singleFaced)
    expect(sections.map((s) => s.title)).toEqual(['Global', 'Australia'])
    expect(sections.every((s) => s.links.length > 0)).toBe(true)
  })

  it('resolves every link to an https URL carrying the encoded card name', () => {
    for (const section of buyLinksFor('mtg', singleFaced)) {
      for (const link of section.links) {
        expect(link.href).toMatch(/^https:\/\//)
        // The name must have been substituted (no leftover placeholder) and
        // land in the URL percent-encoded.
        expect(link.href).not.toContain('{name}')
        expect(link.href).toContain('Gaea')
      }
    }
  })
})
