import type { Card, Product } from '@/lib/api'

// "Where to buy" links for the card + sealed-product detail pages (issue #175).
// Each store is a NAME search deep link — we don't ingest per-store prices or
// product ids, so a button lands the user on the store's own search results for
// the card/product rather than a specific listing (and deliberately shows no
// price of its own).
//
// A store's `template` holds a literal `{name}` placeholder that
// `buildSearchUrl` replaces with the encodeURIComponent-encoded name. The
// registries are keyed by game slug so a future TCG gets its own store list (an
// unknown game simply renders no buy section).

interface BuyStore {
  name: string
  template: string
  // Shown while the "Where to buy" card is collapsed (its default): the couple of
  // headline stores per region. Everything else stays behind the "Show all stores"
  // toggle (see BuyLinks.vue).
  featured?: boolean
  // Set on stores whose template wraps {name} in %22 quotes (an exact-phrase
  // search): the few card names that contain literal double quotes (Portal
  // Three Kingdoms "nickname" cards, some Un-cards) would nest quotes and
  // malform the phrase query, so those characters are dropped first.
  stripQuotes?: boolean
  // Products only: when the sealed product carries its own provider page URL
  // (`product.url` — the exact TCGplayer product page), link straight to that
  // precise page instead of a name search. Ignored for card links.
  preferProductUrl?: boolean
}

interface BuySection {
  title: string
  stores: BuyStore[]
}

export interface BuyLink {
  name: string
  href: string
  // Mirrors BuyStore.featured: visible while the buy card is collapsed.
  featured: boolean
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
// (quotes force the exact phrase; see BuyStore.stripQuotes).
const MTG_SECTIONS: BuySection[] = [
  {
    title: 'Global',
    stores: [
      {
        name: 'TCGplayer',
        template:
          'https://www.tcgplayer.com/search/magic/product?productLineName=magic&view=grid&q={name}',
        featured: true,
      },
      {
        name: 'Card Kingdom',
        template:
          'https://www.cardkingdom.com/catalog/search?search=header&filter%5Bname%5D={name}',
        featured: true,
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
      {
        name: 'MTG Mate',
        template: 'https://www.mtgmate.com.au/cards/search?q={name}',
        featured: true,
      },
      { name: 'Good Games', template: 'https://tcg.goodgames.com.au/search?q={name}' },
      {
        name: 'MTG Singles Australia',
        template: 'https://www.mtgsinglesaustralia.com/search?q=%22{name}%22',
        stripQuotes: true,
      },
      { name: 'Guf', template: 'https://guf.com.au/search?q={name}' },
      { name: 'The Games Cube', template: 'https://www.thegamescube.com/products/search?q={name}' },
      { name: 'Games Portal', template: 'https://gamesportal.com.au/search?q={name}' },
      { name: 'Ronin Games', template: 'https://roningames.com.au/search?q={name}' },
      { name: 'Gameology', template: 'https://www.gameology.com.au/search?q={name}' },
      {
        name: 'Adventurers Guild',
        template: 'https://www.theadventurersguild.com.au/search?q={name}',
      },
      // Decked Out Gaming runs on Neto/Maropost: keyword search is the store root
      // with rf=kw + kw=<phrase>, not a /search path (same shape as Dice Arcade in
      // the product list below).
      { name: 'Decked Out Gaming', template: 'https://www.deckedoutgaming.com/?rf=kw&kw={name}' },
      { name: 'Ozzie Collectables', template: 'https://www.ozziecollectables.com/search?q={name}' },
      {
        name: 'Cherry Collectables',
        template: 'https://www.cherrycollectables.com.au/search?q={name}',
      },
      { name: 'Plenty of Games', template: 'https://www.plentyofgames.com.au/search?q={name}' },
      // Untapped Realms is a singles-only shop on Spiffy Stores (an AU-built
      // platform that mirrors Shopify's /search?q= endpoint).
      { name: 'Untapped Realms', template: 'https://www.untappedrealms.com/search?q={name}' },
      { name: 'That Game Store', template: 'https://thatgamestore.com.au/search?q={name}' },
      // MagicCards.com.au is Drupal Commerce, not Shopify: its singles listing
      // filters on a `title` query param at /single-cards, and the apex host is
      // canonical (the www variant 503s).
      {
        name: 'MagicCards.com.au',
        template: 'https://magiccards.com.au/single-cards?title={name}',
      },
    ],
  },
]

const SECTIONS_BY_GAME: Record<string, BuySection[]> = {
  mtg: MTG_SECTIONS,
}

// Sealed-product "where to buy" stores, split US / Australia. Singles-only
// stores from the card registry above (MTG Mate, MTG Singles Australia, Untapped
// Realms, That Game Store, MagicCards.com.au) are dropped here; conversely a few
// sealed-only stores (The Gamesmen, Games Empire) appear here but not in the card
// list. Each template's name-search endpoint was verified to surface sealed
// products (a booster box / bundle / deck) for a full product name like
// "Bloomburrow Collector Booster Box" — except The Gamesmen, whose Neto search
// shape is inferred from the platform (a Cloudflare bot wall blocked live
// verification). TCGplayer carries `preferProductUrl` so its entry deep-links to
// the exact product page we already hold (`product.url`) rather than a fuzzy name
// search, falling back to search only when that URL is absent.
const MTG_PRODUCT_SECTIONS: BuySection[] = [
  {
    title: 'US',
    stores: [
      {
        name: 'TCGplayer',
        template:
          'https://www.tcgplayer.com/search/magic/product?productLineName=magic&view=grid&q={name}',
        preferProductUrl: true,
        featured: true,
      },
      {
        name: 'Card Kingdom',
        template:
          'https://www.cardkingdom.com/catalog/search?search=header&filter%5Bname%5D={name}',
        featured: true,
      },
      {
        name: 'Star City Games',
        template: 'https://starcitygames.com/search/?search_query={name}',
      },
      {
        name: 'CoolStuffInc',
        template: 'https://www.coolstuffinc.com/main_search.php?pa=searchOnName&q={name}',
      },
      { name: 'Troll and Toad', template: 'https://www.trollandtoad.com/search?q={name}' },
      { name: 'Amazon', template: 'https://www.amazon.com/s?k={name}' },
    ],
  },
  {
    title: 'Australia',
    stores: [
      // Sealed product lives on Good Games' main storefront (www), not the
      // tcg.goodgames.com.au singles marketplace the card links point at.
      { name: 'Good Games', template: 'https://www.goodgames.com.au/search?q={name}' },
      { name: 'Guf', template: 'https://guf.com.au/search?q={name}' },
      { name: 'The Games Cube', template: 'https://www.thegamescube.com/products/search?q={name}' },
      { name: 'Games Portal', template: 'https://gamesportal.com.au/search?q={name}' },
      { name: 'Ronin Games', template: 'https://roningames.com.au/search?q={name}' },
      // Dice Arcade runs on Neto/Maropost: its site search is the store root with
      // rf=kw (keyword-refinement mode) + kw=<phrase>, not a /search path.
      { name: 'Dice Arcade', template: 'https://www.dicearcade.com.au/?rf=kw&kw={name}' },
      { name: 'RNR Hobbies', template: 'https://www.rnrhobbies.com.au/search?q={name}' },
      // Mind Games is WooCommerce: WordPress search with post_type=product to
      // scope results to products (not blog posts / pages).
      { name: 'Mind Games', template: 'https://www.m-g.com.au/?s={name}&post_type=product' },
      {
        name: 'Collectible Madness',
        template: 'https://collectiblemadness.com.au/search?q={name}',
      },
      { name: 'Gameology', template: 'https://www.gameology.com.au/search?q={name}' },
      {
        name: 'Adventurers Guild',
        template: 'https://www.theadventurersguild.com.au/search?q={name}',
      },
      // The Gamesmen and Decked Out Gaming are Neto/Maropost like Dice Arcade
      // above (?rf=kw&kw=). The Gamesmen stocks sealed but not singles, so it's
      // only in this list.
      { name: 'The Gamesmen', template: 'https://www.gamesmen.com.au/?rf=kw&kw={name}' },
      { name: 'Decked Out Gaming', template: 'https://www.deckedoutgaming.com/?rf=kw&kw={name}' },
      { name: 'Ozzie Collectables', template: 'https://www.ozziecollectables.com/search?q={name}' },
      {
        name: 'Cherry Collectables',
        template: 'https://www.cherrycollectables.com.au/search?q={name}',
      },
      // Games Empire stocks sealed but not singles (its MTG collection is
      // boxes/bundles); the apex host is canonical (no www redirect).
      { name: 'Games Empire', template: 'https://gamesempire.com.au/search?q={name}' },
      { name: 'Plenty of Games', template: 'https://www.plentyofgames.com.au/search?q={name}' },
    ],
  },
]

const PRODUCT_SECTIONS_BY_GAME: Record<string, BuySection[]> = {
  mtg: MTG_PRODUCT_SECTIONS,
}

// The slice of `Card` the link builder needs (structural, so tests don't have
// to fabricate full CardFace rows).
export type BuyCard = Pick<Card, 'name' | 'layout'> & { faces: { name: string | null }[] }

// The slice of `Product` the sealed-product link builder needs: the product name
// to search, and its own provider page URL for the `preferProductUrl` stores.
export type BuyProduct = Pick<Product, 'name' | 'url'>

// Layouts catalogued under the combined "A // B" name: a split card's halves
// aren't standalone product names ('Fire' alone isn't a product, 'Fire // Ice'
// is). Aftermath and Room cards are also `split` in Scryfall's data. Every
// other multi-faced layout (transform / modal_dfc / adventure / flip / …) is
// catalogued by its front face.
const COMBINED_NAME_LAYOUTS = new Set(['split'])

export function buildSearchUrl(template: string, cardName: string): string {
  return template.replace(/\{name\}/g, encodeURIComponent(cardName))
}

// The name to search a store for. Split cards keep the full "A // B" name (the
// halves aren't products on their own); other multi-faced cards search by
// their FRONT face name — stores index the front face, while the combined
// form's `//` trips exact-name lookups and phrase searches. Single-faced cards
// have an empty `faces` array and use `name`; a multi-faced card missing its
// face names falls back to the combined name cut at the face separator.
export function searchName(card: BuyCard): string {
  if (COMBINED_NAME_LAYOUTS.has(card.layout ?? '')) return card.name
  return card.faces[0]?.name ?? card.name.split(' // ')[0] ?? card.name
}

// Shared shaping: turn a store registry into resolved link sections, deferring
// each store's href to `hrefFor` (a name search for cards, or a name search /
// direct product link for sealed products).
function toLinkSections(
  sections: BuySection[],
  hrefFor: (store: BuyStore) => string,
): BuyLinkSection[] {
  return sections.map((section) => ({
    title: section.title,
    links: section.stores.map((store) => ({
      name: store.name,
      href: hrefFor(store),
      featured: store.featured === true,
    })),
  }))
}

export function buyLinksFor(game: string, card: BuyCard): BuyLinkSection[] {
  const sections = SECTIONS_BY_GAME[game]
  if (!sections) return []
  const name = searchName(card)
  return toLinkSections(sections, (store) =>
    buildSearchUrl(store.template, store.stripQuotes ? name.replace(/"/g, '') : name),
  )
}

// "Where to buy" links for a sealed product: a name search per store, except the
// `preferProductUrl` stores (TCGplayer), which deep-link to the exact product
// page (`product.url`) when we have it and fall back to a name search otherwise.
export function productBuyLinksFor(game: string, product: BuyProduct): BuyLinkSection[] {
  const sections = PRODUCT_SECTIONS_BY_GAME[game]
  if (!sections) return []
  return toLinkSections(sections, (store) =>
    store.preferProductUrl && product.url
      ? product.url
      : buildSearchUrl(store.template, product.name),
  )
}
