import type { RouteLocationRaw } from 'vue-router'
import type { Card, Deck, KeywordEntry, PreconDeck, Product, SearchResults } from '@/lib/api'
import { KIND_LABELS, glossaryPath, keywordPath } from '@/lib/keywords'
import { preconsPath } from '@/lib/precons'
import { productTypeLabel } from '@/lib/productType'

// The universal search box's pure half: turning one `SearchResults` payload (plus the
// signed-in user's own decks) into the grouped, linkable option list the combobox renders.
//
// Kept out of the composable so the shaping rules — which groups exist and in what order,
// what each hit says under its name, where it links, when a group offers a "see all" row —
// are unit-tested without mounting anything, the same posture as `lib/deckFilter.ts`. The
// server decides *what matches* (see `api/src/handlers/search.rs`); this only decides how a
// match reads and where it goes. The one matching rule that lives here is the deck filter,
// because a user's decks never leave the authed deck list: it **mirrors** the API's every-word
// name rule so "Your decks" answers the same question as every other group.

/** Characters typed before the box asks the API — the quick-add autocomplete's threshold. */
export const SEARCH_MIN_CHARS = 2

/** Matches per group the box asks the API for (and shows of the user's decks). Five groups
 * of four fit a dropdown without scrolling on a laptop; the API clamps to 1–10 anyway. */
export const SEARCH_GROUP_LIMIT = 4

/** What a row in the dropdown is: one of the five result kinds, a group's "see all" row, or
 * the closing "search all cards" row that Enter also triggers. */
export type SearchKind = 'card' | 'deck' | 'product' | 'precon' | 'keyword' | 'more' | 'search'

/** The small image beside a row, drawn with the CardImage / ProductImage proxies. */
export interface SearchThumbnail {
  kind: 'card' | 'product'
  id: string
  name: string
  hasImage: boolean
}

/** One selectable row. `key` is unique across the whole list (it is the listbox option's
 * DOM id suffix and the `v-for` key), `to` is where picking it goes. */
export interface SearchOption {
  key: string
  kind: SearchKind
  label: string
  sublabel?: string
  to: RouteLocationRaw
  thumbnail?: SearchThumbnail
}

/** One heading in the dropdown with its rows — the last row is the group's "see all" link
 * when the API (or the deck cut) left matches behind. */
export interface SearchGroupView {
  id: Exclude<SearchKind, 'more' | 'search'>
  label: string
  options: SearchOption[]
}

const enc = encodeURIComponent

// ----- Where a hit goes -----

/** The full card search for a query — the Scryfall-grammar listing this box hands off to. */
export function cardSearchLocation(game: string, q: string): RouteLocationRaw {
  return { path: `/cards/${enc(game)}/cards`, query: { q } }
}

export function sealedSearchLocation(game: string, q: string): RouteLocationRaw {
  return { path: `/sealed/${enc(game)}/products`, query: { q } }
}

export function preconSearchLocation(game: string, q: string): RouteLocationRaw {
  return { path: `${preconsPath(game)}/all`, query: { q } }
}

export function cardPath(game: string, id: string): string {
  return `/cards/${enc(game)}/cards/${enc(id)}`
}

export function productPath(game: string, id: string): string {
  return `/sealed/${enc(game)}/${enc(id)}`
}

export function preconPath(game: string, slug: string): string {
  return `${preconsPath(game)}/${enc(slug)}`
}

export function deckPath(game: string, id: number): string {
  return `/decks/${enc(game)}/${id}`
}

export function decksPath(game: string): string {
  return `/decks/${enc(game)}`
}

// ----- The deck filter: a client-side mirror of the API's name rule -----

/** Whether `name` contains every whitespace-separated word of `term`, in any order, case-
 * insensitively — the rule the API applies to card, sealed-product, precon and keyword
 * names (`handlers::shared::every_word_matches`), mirrored here for the one group that
 * never reaches the API. A blank term matches nothing, as it answers nothing there. */
export function matchesEveryWord(name: string, term: string): boolean {
  const words = term.toLowerCase().split(/\s+/).filter(Boolean)
  if (words.length === 0) return false
  const haystack = name.toLowerCase()
  return words.every((word) => haystack.includes(word))
}

/** The API's rank, mirrored: a name that starts with the whole term leads one that merely
 * contains it, and ties keep name order. Stable, so equal ranks keep the input order. */
export function rankByPrefix<T>(items: readonly T[], name: (item: T) => string, term: string): T[] {
  const prefix = term.trim().toLowerCase()
  const rank = (item: T) => (name(item).toLowerCase().startsWith(prefix) ? 0 : 1)
  return [...items].sort(
    (a, b) =>
      rank(a) - rank(b) || name(a).localeCompare(name(b), undefined, { sensitivity: 'base' }),
  )
}

/** The user's decks matching `term`, ranked like an API group and cut at the group limit,
 * plus whether the cut left any behind. */
export function filterDecks(
  decks: readonly Deck[],
  term: string,
): { data: Deck[]; hasMore: boolean } {
  const matched = rankByPrefix(
    decks.filter((deck) => matchesEveryWord(deck.name, term)),
    (deck) => deck.name,
    term,
  )
  return {
    data: matched.slice(0, SEARCH_GROUP_LIMIT),
    hasMore: matched.length > SEARCH_GROUP_LIMIT,
  }
}

// ----- Rows -----

function cardOption(game: string, card: Card): SearchOption {
  return {
    key: `card:${card.id}`,
    kind: 'card',
    label: card.name,
    // The type line identifies a card better than the set of the one printing the fold
    // happened to pick; the set is a click away on the card page with every printing.
    sublabel: card.type_line ?? card.set_name,
    to: cardPath(game, card.id),
    thumbnail: { kind: 'card', id: card.id, name: card.name, hasImage: card.has_image },
  }
}

function deckOption(game: string, deck: Deck): SearchOption {
  const commander = deck.commanders[0]
  const parts = [deck.format, `${deck.card_count} card${deck.card_count === 1 ? '' : 's'}`]
  return {
    key: `deck:${deck.id}`,
    kind: 'deck',
    label: deck.name,
    sublabel: parts.filter(Boolean).join(' · '),
    to: deckPath(game, deck.id),
    // The deck list names its commander without saying whether art exists; the image
    // component falls back to a placeholder on a miss, so claim one and let it check.
    thumbnail: commander
      ? { kind: 'card', id: commander.card_id, name: commander.name, hasImage: true }
      : undefined,
  }
}

function productOption(game: string, product: Product): SearchOption {
  return {
    key: `product:${product.id}`,
    kind: 'product',
    label: product.name,
    sublabel: `${product.set_name ?? product.set_code.toUpperCase()} · ${productTypeLabel(product.product_type)}`,
    to: productPath(game, product.id),
    thumbnail: { kind: 'product', id: product.id, name: product.name, hasImage: product.has_image },
  }
}

function preconOption(game: string, precon: PreconDeck): SearchOption {
  return {
    key: `precon:${precon.slug}`,
    kind: 'precon',
    label: precon.name,
    sublabel: `${precon.deck_type} · ${precon.set_name ?? precon.set_code.toUpperCase()}`,
    to: preconPath(game, precon.slug),
    thumbnail: precon.face_card
      ? {
          kind: 'card',
          id: precon.face_card.card_id,
          name: precon.face_card.name,
          hasImage: precon.face_card.has_image,
        }
      : undefined,
  }
}

function keywordOption(game: string, keyword: KeywordEntry): SearchOption {
  return {
    key: `keyword:${keyword.slug}`,
    kind: 'keyword',
    label: keyword.name,
    sublabel: KIND_LABELS[keyword.kind],
    to: keywordPath(game, keyword.slug),
  }
}

function moreOption(id: SearchGroupView['id'], label: string, to: RouteLocationRaw): SearchOption {
  return { key: `more:${id}`, kind: 'more', label, to }
}

/** The closing row every non-blank search ends with (and what Enter does with nothing
 * highlighted): the full card search, where the whole Scryfall grammar applies. */
export function searchAllOption(game: string, term: string): SearchOption {
  return {
    key: 'search:cards',
    kind: 'search',
    label: `Search all cards for “${term}”`,
    sublabel: 'Full Scryfall-style syntax — colours, types, oracle text, prices…',
    to: cardSearchLocation(game, term),
  }
}

export interface BuildGroupsInput {
  game: string
  /** The trimmed text the results answer. */
  term: string
  /** The API's answer, absent while the first request is in flight. */
  results?: SearchResults
  /** The signed-in user's decks; absent when signed out (the group is then not offered). */
  decks?: readonly Deck[]
}

/**
 * The dropdown's groups, in display order, each with its rows and — where the API cut the
 * group, or the deck cut did — a trailing "see all" row. Empty groups are dropped, so the
 * list only ever names kinds that matched.
 *
 * Cards get no "see all" row of their own: the closing {@link searchAllOption} is that link,
 * and it is offered on every search so a visitor can always fall through to the full grammar.
 */
export function buildSearchGroups({
  game,
  term,
  results,
  decks,
}: BuildGroupsInput): SearchGroupView[] {
  const groups: SearchGroupView[] = []
  const quoted = `“${term}”`

  if (results) {
    groups.push({
      id: 'card',
      label: 'Cards',
      options: results.cards.data.map((card) => cardOption(game, card)),
    })
  }

  if (decks) {
    const mine = filterDecks(decks, term)
    const options = mine.data.map((deck) => deckOption(game, deck))
    if (mine.hasMore) options.push(moreOption('deck', 'All your decks', decksPath(game)))
    groups.push({ id: 'deck', label: 'Your decks', options })
  }

  if (results) {
    const products = results.products.data.map((product) => productOption(game, product))
    if (results.products.has_more) {
      products.push(
        moreOption(
          'product',
          `All sealed products matching ${quoted}`,
          sealedSearchLocation(game, term),
        ),
      )
    }
    groups.push({ id: 'product', label: 'Sealed products', options: products })

    const precons = results.precons.data.map((precon) => preconOption(game, precon))
    if (results.precons.has_more) {
      precons.push(
        moreOption(
          'precon',
          `All preconstructed decks matching ${quoted}`,
          preconSearchLocation(game, term),
        ),
      )
    }
    groups.push({ id: 'precon', label: 'Preconstructed decks', options: precons })

    const keywords = results.keywords.data.map((keyword) => keywordOption(game, keyword))
    // The glossary index filters locally, not by URL, so "more" opens the index itself.
    if (results.keywords.has_more) {
      keywords.push(moreOption('keyword', 'Browse the keyword glossary', glossaryPath(game)))
    }
    groups.push({ id: 'keyword', label: 'Keywords', options: keywords })
  }

  return groups.filter((group) => group.options.length > 0)
}
