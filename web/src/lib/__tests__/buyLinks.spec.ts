import { describe, it, expect } from 'vitest'

import { buildSearchUrl, buyLinksFor, searchName } from '../buyLinks'

const singleFaced = { name: "Gaea's Cradle", layout: 'normal', faces: [] }
const doubleFaced = {
  name: 'Fable of the Mirror-Breaker // Reflection of Kiki-Jiki',
  layout: 'transform',
  faces: [{ name: 'Fable of the Mirror-Breaker' }, { name: 'Reflection of Kiki-Jiki' }],
}
const splitCard = {
  name: 'Fire // Ice',
  layout: 'split',
  faces: [{ name: 'Fire' }, { name: 'Ice' }],
}

describe('buildSearchUrl', () => {
  it('substitutes the percent-encoded card name into the template', () => {
    expect(buildSearchUrl('https://example.com/search?q={name}', "Gaea's Cradle")).toBe(
      "https://example.com/search?q=Gaea's%20Cradle",
    )
  })

  it('fills every occurrence of the placeholder', () => {
    expect(buildSearchUrl('https://example.com/{name}?q={name}', 'Fog')).toBe(
      'https://example.com/Fog?q=Fog',
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

  it("keeps a split card's combined name — the halves aren't products on their own", () => {
    expect(searchName(splitCard)).toBe('Fire // Ice')
  })

  it('falls back to the combined name cut at the separator when face names are missing', () => {
    expect(searchName({ ...doubleFaced, faces: [{ name: null }, { name: null }] })).toBe(
      'Fable of the Mirror-Breaker',
    )
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

  it('drops literal double quotes from the name for exact-phrase stores', () => {
    // 'Kongming, "Sleeping Dragon"' and friends would nest quotes inside the
    // %22-wrapped phrase search, malforming it; other stores keep the name.
    const quoted = { name: 'Kongming, "Sleeping Dragon"', layout: 'normal', faces: [] }
    const links = buyLinksFor('mtg', quoted).flatMap((s) => s.links)
    const phrase = links.find((l) => l.name === 'MTG Singles Australia')
    expect(phrase?.href).toBe(
      'https://www.mtgsinglesaustralia.com/search?q=%22Kongming%2C%20Sleeping%20Dragon%22',
    )
    const plain = links.find((l) => l.name === 'Card Kingdom')
    expect(plain?.href).toContain(encodeURIComponent('Kongming, "Sleeping Dragon"'))
  })
})
