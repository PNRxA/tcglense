import { describe, it, expect } from 'vitest'

import type { BuyList, BuyListCard, BuyListProduct } from '@/lib/api'
import {
  MASS_ENTRY_URL_BUDGET,
  MTG_MATE_DECKLIST_URL,
  bulkBuyOptionsFor,
  buyListSummary,
  decklistText,
  massEntryNote,
  massEntryRow,
  tcgplayerMassEntryLink,
  tcgplayerMassEntryUrl,
  truncationNote,
} from '../bulkBuy'

function card(overrides: Partial<BuyListCard> = {}): BuyListCard {
  return {
    card_id: 'sf-1',
    name: 'Sol Ring',
    set_code: 'cmm',
    collector_number: '410',
    quantity: 1,
    foil_quantity: 0,
    tcgplayer_id: 500123,
    ...overrides,
  }
}

function product(overrides: Partial<BuyListProduct> = {}): BuyListProduct {
  return {
    product_id: '517079',
    name: 'Bloomburrow Play Booster Box',
    quantity: 1,
    foil_quantity: 0,
    ...overrides,
  }
}

function list(cards: BuyListCard[], products: BuyListProduct[] = []): BuyList {
  return {
    cards,
    products,
    total_cards: cards.length,
    total_products: products.length,
    truncated: false,
  }
}

// TCGplayer's Mass Entry share-link format, read off its own page bundle: `?c=` carries
// the rows joined by a literal `||`, each row `{qty}-{productId}` or `{qty} Name [SET]
// number`, and `productline` names the game. These pins are the contract — a change here
// lands the user on an empty Mass Entry page, silently.
describe('massEntryRow', () => {
  it('sends a printing TCGplayer lists by its product id, both finishes merged', () => {
    expect(massEntryRow(card({ quantity: 2, foil_quantity: 1 }))).toBe('3-500123')
  })

  it('falls back to name + set + collector number when there is no product id', () => {
    expect(massEntryRow(card({ tcgplayer_id: null, quantity: 4 }))).toBe('4 Sol Ring [CMM] 410')
  })
})

describe('tcgplayerMassEntryUrl', () => {
  it('joins the encoded rows with a literal || and names the product line', () => {
    const url = tcgplayerMassEntryUrl(
      list([
        card(),
        card({ card_id: 'sf-2', name: 'Fire // Ice', tcgplayer_id: null, quantity: 2 }),
      ]),
      'Magic',
    )
    expect(url).toBe(
      'https://www.tcgplayer.com/massentry?c=1-500123||2%20Fire%20%2F%2F%20Ice%20%5BCMM%5D%20410&productline=Magic',
    )
  })

  it('leaves rows off once the encoded list would pass the URL budget, and counts them', () => {
    // 500 name-form rows (no product id) encode to ~40 characters each — far past the
    // budget the row cap alone would allow through.
    const cards = Array.from({ length: 500 }, (_, i) =>
      card({ card_id: `sf-${i}`, name: `Card Number ${i}`, tcgplayer_id: null }),
    )
    const link = tcgplayerMassEntryLink(list(cards), 'Magic')
    const encoded = link.href.slice(
      link.href.indexOf('?c=') + 3,
      link.href.indexOf('&productline='),
    )
    expect(encoded.length).toBeLessThanOrEqual(MASS_ENTRY_URL_BUDGET)
    expect(link.href.length).toBeLessThan(8_000)
    expect(link.sent + link.dropped).toBe(500)
    expect(link.dropped).toBeGreaterThan(0)
    // The rows that fit are the first ones, in order.
    expect(
      decodeURIComponent(encoded).startsWith(
        '1 Card Number 0 [CMM] 410||1 Card Number 1 [CMM] 410',
      ),
    ).toBe(true)
    // An id-only list of the cap size fits whole.
    const byId = Array.from({ length: 500 }, (_, i) =>
      card({ card_id: `sf-${i}`, tcgplayer_id: 500000 + i }),
    )
    expect(tcgplayerMassEntryLink(list(byId), 'Magic')).toMatchObject({ sent: 500, dropped: 0 })
  })

  it('appends sealed products by their TCGplayer product id and skips empty rows', () => {
    const url = tcgplayerMassEntryUrl(
      list(
        [card({ quantity: 0, foil_quantity: 0 }), card({ card_id: 'sf-3' })],
        [product({ quantity: 2 })],
      ),
      'Magic',
    )
    expect(url).toBe('https://www.tcgplayer.com/massentry?c=1-500123||2-517079&productline=Magic')
  })
})

describe('decklistText', () => {
  it('writes one "{qty} Name" line per card, folding printings of one name', () => {
    const text = decklistText([
      card({ quantity: 2 }),
      card({ card_id: 'sf-2', set_code: 'ltc', quantity: 1, foil_quantity: 1 }),
      card({ card_id: 'sf-3', name: 'Arcane Signet', quantity: 1 }),
      card({ card_id: 'sf-4', name: 'Nothing Wanted', quantity: 0, foil_quantity: 0 }),
    ])
    expect(text).toBe('4 Sol Ring\n1 Arcane Signet')
  })
})

describe('massEntryNote', () => {
  const whole = { href: '', sent: 2, dropped: 0 }

  it('says how many rows go by name when some printings have no product id', () => {
    const note = massEntryNote(list([card(), card({ card_id: 'sf-2', tcgplayer_id: null })]), whole)
    expect(note).toContain('1 of 2 printings by exact product')
    expect(note).toContain('1 by name')
  })

  it('stays quiet about matching when every row has an id, and counts sealed products', () => {
    const note = massEntryNote(list([card()], [product()]), whole)
    expect(note).not.toContain('by name')
    expect(note).toContain('1 sealed product included')
  })

  it('says how many rows the URL budget left off', () => {
    const note = massEntryNote(list([card()]), { href: '', sent: 1, dropped: 3 })
    expect(note).toContain('the last 3 rows left off')
  })
})

describe('buyListSummary', () => {
  it('counts cards, copies and sealed products with the right plurals', () => {
    expect(buyListSummary(list([card()]))).toBe('1 card (1 copy)')
    expect(
      buyListSummary(
        list([card({ quantity: 2, foil_quantity: 1 }), card({ card_id: 'b' })], [product()]),
      ),
    ).toBe('2 cards (4 copies) and 1 sealed product')
  })
})

describe('truncationNote', () => {
  it('is silent when nothing was cut', () => {
    expect(truncationNote(list([card()]))).toBeNull()
  })

  it('words a card cut and a product cut separately, never blaming card rows for a product cut', () => {
    const cardsCut = { ...list([card()]), total_cards: 501, truncated: true }
    expect(truncationNote(cardsCut)).toBe(
      'Only the first 1 card rows are sent (500 more on the list). Narrow the list with a search or a set to buy the rest.',
    )
    const productsCut = { ...list([], [product()]), total_products: 201, truncated: true }
    expect(truncationNote(productsCut)).toBe(
      'Only the first 1 sealed products are sent (200 more on the list). Narrow the list with a search or a set to buy the rest.',
    )
    expect(truncationNote(productsCut)).not.toContain('card rows')
  })
})

describe('bulkBuyOptionsFor', () => {
  const sample = list([card()])

  it('offers nothing for a game with no bulk-entry registry', () => {
    expect(bulkBuyOptionsFor('unknown-game', sample)).toEqual([])
  })

  it('offers TCGplayer as a prefilled link and MTG Mate as a copy-and-paste, global first', () => {
    const options = bulkBuyOptionsFor('mtg', sample, 'USD')
    expect(options.map((o) => [o.name, o.kind])).toEqual([
      ['TCGplayer', 'link'],
      ['MTG Mate', 'paste'],
    ])
    const [tcg, mate] = options
    expect(tcg?.href).toContain('tcgplayer.com/massentry?c=1-500123')
    expect(mate?.href).toBe(MTG_MATE_DECKLIST_URL)
    expect(mate?.kind === 'paste' && mate.list).toBe('1 Sol Ring')
    // Every option is https, and no paste store is ever presented as a prefilled link.
    expect(options.every((option) => option.href.startsWith('https://'))).toBe(true)
    expect(
      options
        .filter((option) => option.kind === 'paste')
        .every((option) => !option.href.includes('?')),
    ).toBe(true)
  })

  it('leads with the Australian store for a viewer paying in AUD or NZD', () => {
    for (const currency of ['AUD', 'NZD'] as const) {
      expect(bulkBuyOptionsFor('mtg', sample, currency).map((o) => o.name)).toEqual([
        'MTG Mate',
        'TCGplayer',
      ])
    }
    expect(bulkBuyOptionsFor('mtg', sample, 'EUR').map((o) => o.name)).toEqual([
      'TCGplayer',
      'MTG Mate',
    ])
  })
})
