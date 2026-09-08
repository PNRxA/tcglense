import { describe, it, expect } from 'vitest'

import {
  buildSearchUrl,
  buyLinksFor,
  cardReferenceLinksFor,
  edhrecSlug,
  productBuyLinksFor,
  searchName,
} from '../buyLinks'

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

  it('deep-links TCGplayer and Cardmarket to the exact printing when the detail carries their ids', () => {
    const detail = { ...singleFaced, tcgplayer_id: 500123, cardmarket_id: 363117 }
    const links = buyLinksFor('mtg', detail).flatMap((s) => s.links)
    expect(links.find((l) => l.name === 'TCGplayer')?.href).toBe(
      'https://www.tcgplayer.com/product/500123',
    )
    expect(links.find((l) => l.name === 'Cardmarket')?.href).toBe(
      'https://www.cardmarket.com/en/Magic/Products?idProduct=363117',
    )
    // Every other store still searches by name.
    expect(links.find((l) => l.name === 'Card Kingdom')?.href).toContain('Gaea')
  })

  it('falls back to the name search when a store id is null or absent', () => {
    // A `CardDetail` whose printing TCGplayer doesn't list, and a plain listing `Card`.
    for (const card of [{ ...singleFaced, tcgplayer_id: null, cardmarket_id: null }, singleFaced]) {
      const links = buyLinksFor('mtg', card).flatMap((s) => s.links)
      expect(links.find((l) => l.name === 'TCGplayer')?.href).toContain('tcgplayer.com/search')
      expect(links.find((l) => l.name === 'Cardmarket')?.href).toContain('searchString=')
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

describe('productBuyLinksFor', () => {
  const product = { name: 'Bloomburrow Collector Booster Box', url: null }

  it('returns nothing for a game with no store registry', () => {
    expect(productBuyLinksFor('unknown-game', product)).toEqual([])
  })

  it('returns the US and Australia sections for mtg', () => {
    const sections = productBuyLinksFor('mtg', product)
    expect(sections.map((s) => s.title)).toEqual(['US', 'Australia'])
    expect(sections.every((s) => s.links.length > 0)).toBe(true)
  })

  it('resolves every link to an https URL carrying the encoded product name', () => {
    for (const section of productBuyLinksFor('mtg', product)) {
      for (const link of section.links) {
        expect(link.href).toMatch(/^https:\/\//)
        expect(link.href).not.toContain('{name}')
        // TCGplayer with a null url falls back to a name search too, so every
        // link carries the encoded product name here.
        expect(link.href).toContain(encodeURIComponent('Bloomburrow'))
      }
    }
  })

  it('deep-links the TCGplayer entry to the exact product page when a url is present', () => {
    const withUrl = {
      name: 'Bloomburrow Play Booster Box',
      url: 'https://www.tcgplayer.com/product/517079',
    }
    const links = productBuyLinksFor('mtg', withUrl).flatMap((s) => s.links)
    const tcg = links.find((l) => l.name === 'TCGplayer')
    expect(tcg?.href).toBe('https://www.tcgplayer.com/product/517079')
    // Other stores still search by name.
    const amazon = links.find((l) => l.name === 'Amazon')
    expect(amazon?.href).toBe(`https://www.amazon.com/s?k=${encodeURIComponent(withUrl.name)}`)
  })

  it('falls back to a TCGplayer name search when the product has no url', () => {
    const links = productBuyLinksFor('mtg', product).flatMap((s) => s.links)
    const tcg = links.find((l) => l.name === 'TCGplayer')
    expect(tcg?.href).toContain('tcgplayer.com/search')
    expect(tcg?.href).toContain(encodeURIComponent(product.name))
  })
})

describe('edhrecSlug', () => {
  it('lower-cases, drops punctuation and dashes the rest, keeping both faces', () => {
    expect(edhrecSlug('Sol Ring')).toBe('sol-ring')
    expect(edhrecSlug('Jace, the Mind Sculptor')).toBe('jace-the-mind-sculptor')
    expect(edhrecSlug("Lim-Dûl's Vault")).toBe('lim-duls-vault')
    expect(edhrecSlug('Æther Vial')).toBe('aether-vial')
    expect(edhrecSlug('Delver of Secrets // Insectile Aberration')).toBe(
      'delver-of-secrets-insectile-aberration',
    )
    expect(edhrecSlug('Kongming, "Sleeping Dragon"')).toBe('kongming-sleeping-dragon')
  })
})

describe('cardReferenceLinksFor', () => {
  it('offers Gatherer only when the printing has a multiverse id, and EDHREC always', () => {
    const listed = cardReferenceLinksFor('mtg', {
      ...singleFaced,
      multiverse_ids: [450221, 450222],
    })
    expect(listed).toEqual([
      {
        name: 'Gatherer',
        href: 'https://gatherer.wizards.com/Pages/Card/Details.aspx?multiverseid=450221',
      },
      { name: 'EDHREC', href: 'https://edhrec.com/cards/gaeas-cradle' },
    ])
    // No id (a listing `Card`, or a printing Gatherer never had): no dead Gatherer button.
    for (const card of [singleFaced, { ...singleFaced, multiverse_ids: [] }]) {
      expect(cardReferenceLinksFor('mtg', card).map((l) => l.name)).toEqual(['EDHREC'])
    }
  })

  it('offers nothing for a game with no references', () => {
    expect(cardReferenceLinksFor('unknown-game', singleFaced)).toEqual([])
  })
})
