import type { QueryClient } from '@tanstack/vue-query'
import type { Card, CardSet, Product } from '@/lib/api'

// Detail-page cache scavengers: when a card / product / set detail mounts, the full
// object is often already in a list (or related) cache entry the user just came from.
// Reading it seeds the detail query's `placeholderData`, so the page paints instantly
// while the real fetch confirms in the background (placeholderData, not initialData — a
// 404 still gives way to the error branch). Query keys are built from refs but serialize
// to plain values at runtime; the family prefixes below mirror the list/detail composables
// and the invalidation filters (invalidateCollectionData / invalidateWishlistData). Every
// shape check is defensive — a matched entry may be a keepPreviousData placeholder
// (undefined) or hold an unrelated shape — and the first hit wins.

// Pull the `.data` array out of a Page-like / DataBody-like payload; [] for anything else.
function pageRows(payload: unknown): unknown[] {
  const data = (payload as { data?: unknown } | null)?.data
  return Array.isArray(data) ? data : []
}

// A nested field off a row object (the holding's `.card`, the sealed ref's `.product`).
function nested(row: unknown, key: 'card' | 'product'): unknown {
  return typeof row === 'object' && row !== null ? (row as Record<string, unknown>)[key] : undefined
}

function isCard(value: unknown): value is Card {
  return (
    typeof value === 'object' &&
    value !== null &&
    typeof (value as { id?: unknown }).id === 'string' &&
    typeof (value as { name?: unknown }).name === 'string'
  )
}

function isProduct(value: unknown): value is Product {
  return (
    typeof value === 'object' &&
    value !== null &&
    typeof (value as { id?: unknown }).id === 'string' &&
    typeof (value as { product_type?: unknown }).product_type === 'string'
  )
}

function isSet(value: unknown): value is CardSet {
  return (
    typeof value === 'object' &&
    value !== null &&
    typeof (value as { code?: unknown }).code === 'string' &&
    typeof (value as { name?: unknown }).name === 'string'
  )
}

// Each card family paired with how a row yields its Card: list/print families ARE the
// card; the collection/wishlist holdings nest it under `.card`.
type RowToCard = (row: unknown) => unknown
const rowAsCard: RowToCard = (row) => row
const holdingCard: RowToCard = (row) => nested(row, 'card')

const CARD_FAMILIES: ReadonlyArray<readonly [string, RowToCard]> = [
  ['cards', rowAsCard],
  ['set-cards', rowAsCard],
  ['collection', holdingCard],
  ['wishlist', holdingCard],
  ['card-prints', rowAsCard],
  ['card-printings', rowAsCard],
]

export function findCardInCache(qc: QueryClient, game: string, id: string): Card | undefined {
  for (const [family, toCard] of CARD_FAMILIES) {
    for (const [, payload] of qc.getQueriesData({ queryKey: [family, game] })) {
      for (const row of pageRows(payload)) {
        const card = toCard(row)
        if (isCard(card) && card.id === id) return card
      }
    }
  }
  return undefined
}

export function findProductInCache(qc: QueryClient, game: string, id: string): Product | undefined {
  // Product-list pages: the row IS the product.
  for (const [, payload] of qc.getQueriesData({ queryKey: ['products', game] })) {
    for (const row of pageRows(payload)) {
      if (isProduct(row) && row.id === id) return row
    }
  }
  // Card→sealed sections: each ref wraps its product under `.product`.
  for (const [, payload] of qc.getQueriesData({ queryKey: ['card-sealed', game] })) {
    for (const row of pageRows(payload)) {
      const product = nested(row, 'product')
      if (isProduct(product) && product.id === id) return product
    }
  }
  return undefined
}

export function findSetInCache(qc: QueryClient, game: string, code: string): CardSet | undefined {
  for (const [, payload] of qc.getQueriesData({ queryKey: ['sets', game] })) {
    for (const row of pageRows(payload)) {
      if (isSet(row) && row.code === code) return row
    }
  }
  return undefined
}
