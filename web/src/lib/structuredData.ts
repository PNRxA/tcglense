// Builders for the SEO enrichment on the card + sealed-product detail pages (issue #302):
// keyword-rich meta descriptions, schema.org `Product` JSON-LD carrying "what's in the box"
// and the card's stats, and a `BreadcrumbList` — all emitted through `usePageMeta` (seo.ts).
//
// Two deliberate constraints, mirroring the pre-existing views:
//   1. NO `offers`/`availability`/`price`/`AggregateOffer` in the structured data. TCGLense is
//      a price-TRACKING site, not a storefront; marking a card/product as purchasable here
//      would be a false structured-data claim and risks Google suppressing the rich result.
//      Prices appear only in the human-readable prose (a description string, not a machine
//      offer). The `structuredData.spec.ts` guard test fails the build if any leaks back in.
//   2. Contents are linked with `isRelatedTo` (domain `Product`), NOT `hasPart` (domain
//      `CreativeWork`, out-of-domain on a `Product` → invalid markup).
//
// Pure functions with no Vue/query-client dependency, so they're unit-tested directly.

import type { Card, Product, ProductComponent } from '@/lib/api'
import { formatUsd } from '@/lib/money'
import { absoluteUrl } from '@/lib/seo'

/** Fixed call-to-action closing every meta description; always survives the length budget. */
const TRACKING_TAIL = 'Track its price history on TCGLense.'

/** Meta-description length target — Google typically truncates the snippet near here. */
const MAX_DESCRIPTION = 160

/** JSON-LD `description` cap — long enough for a card's oracle text / a product's full
 * contents, short enough to stay a summary (this text is not the SERP snippet). */
const MAX_JSON_LD_DESCRIPTION = 500

/** `"rare"` → `"Rare"`; passes `null`/empty through unchanged. */
export function capitalize(s: string | null | undefined): string {
  return s ? s.charAt(0).toUpperCase() + s.slice(1) : (s ?? '')
}

/** `"{2}{W}{U}"` → `"2WU"`, `"{W/U}"` → `"W/U"`; `null`/empty → `null`. Strips the `{…}`
 * braces Scryfall wraps each symbol in, so the value reads as plain text in structured data. */
function manaCostPlain(cost: string | null | undefined): string | null {
  if (!cost) return null
  const stripped = cost.replace(/\{([^}]+)\}/g, '$1')
  return stripped || null
}

const COLOR_NAMES: Record<string, string> = {
  W: 'White',
  U: 'Blue',
  B: 'Black',
  R: 'Red',
  G: 'Green',
  C: 'Colorless',
}

/** `["W","U"]` → `"White/Blue"`; `[]` → `null`. Unknown letters pass through as-is. */
function colorNames(letters: readonly string[]): string | null {
  if (!letters.length) return null
  return letters.map((l) => COLOR_NAMES[l] ?? l).join('/')
}

/**
 * Assemble a meta description: the `lead` (always kept) plus the longest prefix of `clauses`, in
 * priority order, that fits within `max` once the fixed `tail` is appended — so the tail always
 * survives. Assembly STOPS at the first clause that doesn't fit, so a lower-priority clause can
 * never take a dropped higher-priority one's place (e.g. the sealed "what's in the box" summary
 * is never silently replaced by the price). Nullish/empty clauses are skipped *without* stopping
 * (an absent clause isn't a dropped one). Whitespace is collapsed. If `lead` + `tail` alone
 * already exceed `max` it's still returned (the item's name must survive — Google truncates).
 */
export function assembleMetaDescription(
  lead: string,
  clauses: (string | null | undefined)[],
  tail: string = TRACKING_TAIL,
  max: number = MAX_DESCRIPTION,
): string {
  let out = lead
  for (const clause of clauses) {
    if (!clause) continue
    if (`${out} ${clause} ${tail}`.length > max) break
    out += ` ${clause}`
  }
  return `${out} ${tail}`.replace(/\s+/g, ' ').trim()
}

/** One schema.org `PropertyValue`, or `null` when the value is absent (so `.filter` drops it
 * and no empty-valued property ever ships). `0` is a real value (a card's mana value) — kept. */
type PropVal = { '@type': 'PropertyValue'; name: string; value: string | number }
function prop(name: string, value: string | number | null | undefined): PropVal | null {
  if (value === null || value === undefined || value === '') return null
  return { '@type': 'PropertyValue', name, value }
}

// ---------- Card ----------

/** The SERP-snippet meta description for a card page: name — rarity/type · set · #number,
 * with the latest USD price appended when it fits. */
export function cardMetaDescription(c: Card): string {
  const descriptor = [c.rarity ? capitalize(c.rarity) : null, c.type_line].filter(Boolean).join(' ')
  const provenance = [c.set_name, `#${c.collector_number}`].filter(Boolean).join(' · ')
  const lead = `${c.name} — ${[descriptor || null, provenance].filter(Boolean).join(' · ')}.`
  const usd = formatUsd(c.prices.usd)
  return assembleMetaDescription(lead, [usd ? `Latest price ${usd}.` : null])
}

/** The richer JSON-LD `description` for a card (not the SERP snippet): a factual lead plus the
 * oracle text (both faces for a multi-faced card), with mana braces stripped, capped. */
function cardJsonLdDescription(c: Card): string {
  const descriptor = [
    c.rarity ? capitalize(c.rarity) : null,
    c.type_line,
    c.set_name ? `from ${c.set_name}` : null,
  ]
    .filter(Boolean)
    .join(' ')
  const oracle = c.faces.length
    ? c.faces
        .map((f) => f.oracle_text)
        .filter(Boolean)
        .join(' // ')
    : c.oracle_text
  const body = oracle ? oracle.replace(/\{([^}]+)\}/g, '$1') : ''
  const head = descriptor ? `${c.name} — ${descriptor}.` : `${c.name}.`
  return [head, body].filter(Boolean).join(' ').slice(0, MAX_JSON_LD_DESCRIPTION)
}

/** The schema.org `Product` node for a card. No `offers` (see the file header). */
export function cardProductNode(c: Card, image?: string): Record<string, unknown> {
  const props = [
    prop('Set', c.set_name),
    prop('Set code', c.set_code.toUpperCase()),
    prop('Collector number', c.collector_number),
    prop('Rarity', c.rarity ? capitalize(c.rarity) : null),
    prop('Mana cost', manaCostPlain(c.mana_cost)),
    prop('Mana value', c.cmc),
    prop('Color identity', colorNames(c.color_identity)),
    prop('Power', c.power),
    prop('Toughness', c.toughness),
    prop('Loyalty', c.loyalty),
    c.lang && c.lang !== 'en' ? prop('Language', c.lang) : null,
  ].filter((p): p is PropVal => p !== null)

  const node: Record<string, unknown> = {
    '@type': 'Product',
    name: c.name,
    brand: { '@type': 'Brand', name: c.set_name },
    description: cardJsonLdDescription(c),
    sku: `${c.set_code.toUpperCase()}-${c.collector_number}`,
    additionalProperty: props,
  }
  if (image) node.image = image
  if (c.type_line) node.category = c.type_line
  if (c.released_at && /^\d{4}-\d{2}-\d{2}/.test(c.released_at)) node.releaseDate = c.released_at.slice(0, 10)
  return node
}

// ---------- Sealed product ----------

/** A short "what's in the box" clause for the meta description: the first couple of
 * components as `qty× name`, in the API's display order, with a trailing "and more" when
 * the list is longer. `null` when the product has no ingested composition. */
export function contentsSummary(components: ProductComponent[]): string | null {
  const items = components.filter((c) => c.name && c.quantity >= 1)
  if (!items.length) return null
  const shown = items.slice(0, 2).map((c) => `${c.quantity}× ${c.name}`)
  return `Contains ${shown.join(', ')}${items.length > 2 ? ' and more' : ''}.`
}

/** The SERP-snippet meta description for a sealed product. Only appends the type/set context
 * the product name doesn't already carry (anti-stuffing), then the contents summary, then the
 * price, dropping the lowest-priority clause first if over budget. */
export function productMetaDescription(
  p: Product,
  typeLabel: string,
  setName: string,
  components: ProductComponent[],
): string {
  const nameLc = p.name.toLowerCase()
  const context = [typeLabel, setName]
    .filter((t) => t && !nameLc.includes(t.toLowerCase()))
    .join(' · ')
  const lead = context ? `${p.name} — ${context}.` : `${p.name}.`
  const usd = formatUsd(p.prices.usd)
  return assembleMetaDescription(lead, [
    contentsSummary(components),
    usd ? `Latest price ${usd}.` : null,
  ])
}

/** The JSON-LD `description` for a sealed product: a factual lead plus the FULL contents list
 * (directly serving #302 — "include what sealed products contain"), capped. */
function productJsonLdDescription(
  p: Product,
  typeLabel: string,
  setName: string,
  components: ProductComponent[],
): string {
  const lead = `${p.name} is a ${typeLabel}${setName ? ` from ${setName}` : ''}.`
  const items = components.filter((c) => c.name)
  if (!items.length) return lead.slice(0, MAX_JSON_LD_DESCRIPTION)
  const all = items.map((c) => `${c.quantity}× ${c.name}`).join(', ')
  return `${lead} Contents: ${all}.`.slice(0, MAX_JSON_LD_DESCRIPTION)
}

/** The schema.org `Product` node for a sealed product. Contents that resolve to a catalog
 * product/card are linked via `isRelatedTo` (valid on `Product`); no `offers` (see header). */
export function sealedProductNode(
  game: string,
  p: Product,
  typeLabel: string,
  setName: string,
  components: ProductComponent[],
  image?: string,
): Record<string, unknown> {
  const props = [
    prop('Set', setName || null),
    prop('Set code', p.set_code.toUpperCase()),
    prop('Product type', typeLabel || null),
  ].filter((x): x is PropVal => x !== null)

  const node: Record<string, unknown> = {
    '@type': 'Product',
    name: p.name,
    description: productJsonLdDescription(p, typeLabel, setName, components),
    sku: p.id,
    additionalProperty: props,
  }
  if (typeLabel) node.category = typeLabel
  if (setName) node.brand = { '@type': 'Brand', name: setName }
  if (image) node.image = image
  if (p.released_at && /^\d{4}-\d{2}-\d{2}/.test(p.released_at)) node.releaseDate = p.released_at.slice(0, 10)

  // Only components that resolve to a catalog product/card become linked entities; textual
  // line items (decks, physical extras) are covered by the prose `description` above. Capped
  // so a huge composition can't bloat the tag.
  const related = components
    .map((c): Record<string, unknown> | null =>
      c.product
        ? { '@type': 'Product', name: c.name, url: absoluteUrl(`/sealed/${game}/${c.product.id}`) }
        : c.card
          ? { '@type': 'Product', name: c.name, url: absoluteUrl(`/cards/${game}/cards/${c.card.id}`) }
          : null,
    )
    .filter((x): x is Record<string, unknown> => x !== null)
    .slice(0, 20)
  if (related.length) node.isRelatedTo = related
  return node
}

// ---------- Breadcrumbs + graph ----------

/** One breadcrumb step. Shares `PageBreadcrumbs.vue`'s prop shape so the same array drives
 * both the visible trail and the JSON-LD; the terminal (current-page) crumb omits `to`. */
export interface Crumb {
  label: string
  to?: string
}

/** A schema.org `BreadcrumbList` from a crumb trail. Each `item` is made absolute (Google
 * requires it); the terminal crumb (no `to`) omits `item`, which is valid. */
export function breadcrumbList(crumbs: Crumb[]): Record<string, unknown> {
  return {
    '@type': 'BreadcrumbList',
    itemListElement: crumbs.map((c, i) => {
      const element: Record<string, unknown> = {
        '@type': 'ListItem',
        position: i + 1,
        name: c.label,
      }
      const url = absoluteUrl(c.to)
      if (url) element.item = url
      return element
    }),
  }
}

/** Wrap one or more schema.org nodes in a single `@graph` object (what `usePageMeta` emits as
 * one JSON-LD `<script>`), dropping nullish nodes. `undefined` when every node is absent, so
 * nothing is emitted before the page's data has loaded. */
export function graph(
  ...nodes: (Record<string, unknown> | null | undefined)[]
): Record<string, unknown> | undefined {
  const present = nodes.filter((n): n is Record<string, unknown> => !!n)
  return present.length ? { '@context': 'https://schema.org', '@graph': present } : undefined
}

/** Home › Cards › {Set} › {Card} — 4 levels; the set page is itself a cards page, so the
 * Set crumb belongs on the cards trail. */
export function cardCrumbs(game: string, c: Card): Crumb[] {
  return [
    { label: 'Home', to: '/' },
    { label: 'Cards', to: `/cards/${game}/cards` },
    { label: c.set_name, to: `/cards/${game}/sets/${c.set_code}` },
    { label: c.name },
  ]
}

/** Home › Sealed › {Product} — 3 levels (there is no sealed-by-set route, so no Set crumb;
 * a Set crumb would have to point at the cards set page, cross-sectioning the trail). */
export function sealedCrumbs(game: string, p: Product): Crumb[] {
  return [
    { label: 'Home', to: '/' },
    { label: 'Sealed', to: `/sealed/${game}` },
    { label: p.name },
  ]
}
