import type { Card } from '@/lib/api'

// "Where to buy" links for the card detail page (issue #175). Each store is a
// card-NAME search deep link — we don't ingest per-store prices or product ids,
// so a button lands the user on the store's own search results for the card
// rather than a specific listing (and deliberately shows no price of its own).
//
// A store's `template` holds a literal `{name}` placeholder that
// `buildSearchUrl` replaces with the encodeURIComponent-encoded card name.
// The registry is keyed by game slug so a future TCG gets its own store list
// (an unknown game simply renders no buy section).

export interface BuyStore {
  name: string
  template: string
}

export interface BuySection {
  title: string
  stores: BuyStore[]
}

export interface BuyLink {
  name: string
  href: string
}

export interface BuyLinkSection {
  title: string
  links: BuyLink[]
}

// Store templates were each verified against the live site (or, where a bot
// wall blocked fetching, against how Scryfall/search engines deep-link to it).
// MTG Mate has no free-text search endpoint — /cards/{name} is its exact-name
// landing page listing every printing. MTG Singles Australia wraps the name in
// literal %22 quotes: its Shopify search otherwise OR-matches individual words
// (quotes force the exact phrase; no MTG card name contains a double quote).
const MTG_SECTIONS: BuySection[] = [
  {
    title: 'Global',
    stores: [
      {
        name: 'TCGplayer',
        template: 'https://www.tcgplayer.com/search/magic/product?productLineName=magic&view=grid&q={name}',
      },
      {
        name: 'Card Kingdom',
        template: 'https://www.cardkingdom.com/catalog/search?search=header&filter%5Bname%5D={name}',
      },
      {
        name: 'Cardmarket',
        template: 'https://www.cardmarket.com/en/Magic/Products/Search?searchString={name}',
      },
      {
        name: 'Star City Games',
        template: 'https://starcitygames.com/search/?search_query={name}',
      },
      {
        name: 'CoolStuffInc',
        template: 'https://www.coolstuffinc.com/main_search.php?pa=searchOnName&q={name}',
      },
      {
        name: 'MTGMintCard',
        template: 'https://www.mtgmintcard.com/mtg/singles/search?keywords={name}',
      },
      { name: 'Mana Pool', template: 'https://manapool.com/cards?q={name}' },
      { name: 'Troll and Toad', template: 'https://www.trollandtoad.com/search?q={name}' },
      {
        name: 'Face to Face Games',
        template: 'https://facetofacegames.com/en-us/search?q={name}',
      },
      {
        // Digital-only: Magic Online singles (priced in tix), Scryfall's MTGO partner.
        name: 'Cardhoarder (MTGO)',
        template: 'https://www.cardhoarder.com/cards?data%5Bsearch%5D={name}',
      },
    ],
  },
  {
    title: 'Australia',
    stores: [
      { name: 'MTG Mate', template: 'https://www.mtgmate.com.au/cards/{name}' },
      { name: 'Good Games', template: 'https://tcg.goodgames.com.au/search?q={name}' },
      {
        name: 'MTG Singles Australia',
        template: 'https://www.mtgsinglesaustralia.com/search?q=%22{name}%22',
      },
      { name: 'Guf', template: 'https://guf.com.au/search?q={name}' },
      { name: 'The Games Cube', template: 'https://www.thegamescube.com/products/search?q={name}' },
      { name: 'Games Portal', template: 'https://gamesportal.com.au/search?q={name}' },
      { name: 'Ronin Games', template: 'https://roningames.com.au/search?q={name}' },
    ],
  },
]

const SECTIONS_BY_GAME: Record<string, BuySection[]> = {
  mtg: MTG_SECTIONS,
}

// The slice of `Card` the link builder needs (structural, so tests don't have
// to fabricate full CardFace rows).
export type BuyCard = Pick<Card, 'name'> & { faces: { name: string | null }[] }

export function buildSearchUrl(template: string, cardName: string): string {
  return template.replace('{name}', encodeURIComponent(cardName))
}

// Multi-faced cards search by their FRONT face name: every store indexes the
// front face, while the full "A // B" form (or its `/`s) trips some search
// engines. Single-faced cards have an empty `faces` array and use `name`.
export function searchName(card: BuyCard): string {
  return card.faces[0]?.name ?? card.name
}

export function buyLinksFor(game: string, card: BuyCard): BuyLinkSection[] {
  const sections = SECTIONS_BY_GAME[game]
  if (!sections) return []
  const name = searchName(card)
  return sections.map((section) => ({
    title: section.title,
    links: section.stores.map((store) => ({
      name: store.name,
      href: buildSearchUrl(store.template, name),
    })),
  }))
}
