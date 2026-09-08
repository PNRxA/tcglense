import { computed, toValue, type MaybeRefOrGetter, type Ref } from 'vue'
import { keepPreviousData, useQuery, useQueryClient } from '@tanstack/vue-query'
import {
  getCardSealed,
  getProduct,
  getProductCards,
  getProductCardSections,
  getProductContainers,
  getProductContents,
  getProductEv,
  getProductFacets,
  listProducts,
  openProduct,
} from '@/lib/api'
import type { ProductCardSectionKey } from '@/lib/api'
import { PRODUCT_CARDS_DEFAULT_SORT, toSortParam } from '@/lib/cardSort'
import { findProductInCache } from '@/lib/placeholders'
import { PRICED_CATALOG_STALE_MS } from '@/lib/queryClient'

/**
 * Shared reads for the sealed-products section. These are PUBLIC catalog endpoints
 * (no auth), so they use plain `useQuery` — not the `useAuthedQuery` wrapper — and
 * carry their reactive params inside the query key so a change refetches.
 */

/** Sealed products per page in the browse grid. */
export const PRODUCT_PAGE_SIZE = 60

/** Cards per page in a product's "cards in this product" section. */
export const PRODUCT_CARDS_PAGE_SIZE = 60

/** Reactive controls for the product-list query, all carried in the key so a change
 * refetches; `defaultSort` backs an empty sort. */
interface ProductListQueryOptions {
  page: Ref<number>
  query: Ref<string>
  set: Ref<string>
  type: Ref<string>
  sort: Ref<string>
  // Plain value or ref/getter: the sealed browse passes a computed so the fallback sort tracks
  // the active set (Secret Lair defaults to newest-first), matching the same computed handed to
  // `useCardSearch`'s `defaultSort`.
  defaultSort: MaybeRefOrGetter<string>
}

/** A page of a game's sealed products (name search + set/type filters + sort).
 * `keepPreviousData` holds the current grid up while the next page loads. */
export function useProductsQuery(game: Ref<string>, opts: ProductListQueryOptions) {
  return useQuery({
    queryKey: ['products', game, opts.query, opts.set, opts.type, opts.sort, opts.page],
    queryFn: ({ signal }) =>
      listProducts(
        game.value,
        {
          q: opts.query.value || undefined,
          set: opts.set.value || undefined,
          type: opts.type.value || undefined,
          page: opts.page.value,
          pageSize: PRODUCT_PAGE_SIZE,
          ...toSortParam(opts.sort.value, toValue(opts.defaultSort)),
        },
        signal,
      ),
    placeholderData: keepPreviousData,
    // Product rows carry price_usd, which moves only on the daily sync (#413).
    staleTime: PRICED_CATALOG_STALE_MS,
  })
}

/** One sealed product by id. Keyed on `['product', game, id]`. */
export function useProductQuery(game: Ref<string>, id: Ref<string>) {
  const qc = useQueryClient()
  return useQuery({
    queryKey: ['product', game, id],
    queryFn: () => getProduct(game.value, id.value),
    // Seed from the (warm) product-list / card-sealed caches so the detail paints
    // instantly; the real fetch still runs (placeholderData, not initialData) and reads
    // the current refs so a product→product navigation re-evaluates.
    placeholderData: () => findProductInCache(qc, game.value, id.value),
    // Carries price_usd — daily cadence (#413).
    staleTime: PRICED_CATALOG_STALE_MS,
  })
}

/** A page of one display section of the cards a sealed product contains / can be pulled
 * from (the sealed-detail "Cards in this product" blocks — each section paginates on its
 * own, issue #224). Public read, so a plain `useQuery`; `keepPreviousData` holds the
 * current grid up while the next page loads. The `section` is fixed per block, so it sits
 * in the key as a plain value alongside the reactive `game`/`id`/`page`. `q` is the shared
 * card search (issue #222); it goes in the key (as a ref) so committing a search refetches,
 * and the caller resets `page` to 1 alongside it. `sort` is the shared card sort (a
 * `field:dir` option, or the `default` sentinel for the product's natural order): it too
 * goes in the key so changing it refetches, with the caller resetting `page` to 1.
 * `component` addresses an **unlisted** box component's cards instead of a plain section
 * (`section` then narrows within it); fixed per block like `section`, so it sits in the
 * key as a plain value. `enabled` gates the fetch — a collapsed section block passes its
 * expanded state so pages are only pulled once the user reveals the section (issue #291). */
export function useProductCardsQuery(
  game: Ref<string>,
  id: Ref<string>,
  page: Ref<number>,
  section: ProductCardSectionKey,
  component: string | undefined,
  q: Ref<string>,
  sort: Ref<string>,
  enabled?: Ref<boolean>,
) {
  return useQuery({
    queryKey: ['product-cards', game, id, section, component ?? null, q, sort, page],
    enabled,
    queryFn: ({ signal }) => {
      // The default sentinel maps to *no* `sort` param (the API's natural order); any other
      // option splits into the API's orthogonal `sort`/`dir` pair.
      const sortParam =
        sort.value && sort.value !== PRODUCT_CARDS_DEFAULT_SORT
          ? toSortParam(sort.value, PRODUCT_CARDS_DEFAULT_SORT)
          : undefined
      return getProductCards(
        game.value,
        id.value,
        page.value,
        PRODUCT_CARDS_PAGE_SIZE,
        section,
        component,
        q.value || undefined,
        sortParam?.sort,
        sortParam?.dir,
        signal,
      )
    },
    placeholderData: keepPreviousData,
    // Card grids embed Card.prices — daily cadence (#413).
    staleTime: PRICED_CATALOG_STALE_MS,
  })
}

/** The non-empty display sections (+ counts) of a sealed product's cards — the manifest
 * driving which "Cards in this product" blocks to render (issue #224). Public read, so a
 * plain `useQuery`; refs go in the key so a product-to-product navigation refetches. `q` is
 * the shared card search (issue #222): with it in the key, committing a search refetches
 * the filtered manifest (fewer sections + recomputed counts). `keepPreviousData` holds the
 * current sections up while the filtered manifest loads, so committing a search doesn't
 * flash an empty "no matches" state mid-fetch. */
export function useProductCardSectionsQuery(game: Ref<string>, id: Ref<string>, q: Ref<string>) {
  return useQuery({
    queryKey: ['product-card-sections', game, id, q],
    queryFn: ({ signal }) =>
      getProductCardSections(game.value, id.value, q.value || undefined, signal),
    placeholderData: keepPreviousData,
    // Recomputed only by the daily sync (#413).
    staleTime: PRICED_CATALOG_STALE_MS,
  })
}

/** A sealed product's structural composition — "what's in the box" (the sealed-detail
 * "What's in the box" section). Public read, so a plain `useQuery`; refs go in the key so a
 * product-to-product navigation refetches. */
export function useProductContentsQuery(game: Ref<string>, id: Ref<string>) {
  return useQuery({
    queryKey: ['product-contents', game, id],
    queryFn: ({ signal }) => getProductContents(game.value, id.value, signal),
    // Component lines embed linked Product rows (with prices) — daily cadence (#413).
    staleTime: PRICED_CATALOG_STALE_MS,
  })
}

/** The parent sealed products whose composition directly contains this product (the
 * sealed-detail "Included in" section). Public read, keyed on the reactive route refs. */
export function useProductContainersQuery(game: Ref<string>, id: Ref<string>) {
  return useQuery({
    queryKey: ['product-containers', game, id],
    queryFn: ({ signal }) => getProductContainers(game.value, id.value, signal),
    // Parents embed full Product rows (with prices) — daily cadence (#413).
    staleTime: PRICED_CATALOG_STALE_MS,
  })
}

/** The distinct product types + sets that have products, for the filter dropdowns.
 * Effectively static per game, so it stays fresh a while. */
export function useProductFacetsQuery(game: Ref<string>) {
  return useQuery({
    queryKey: ['product-facets', game],
    queryFn: () => getProductFacets(game.value),
    staleTime: 60 * 60 * 1000,
  })
}

/** What an average pack of everything one copy of this product opens is worth (issue
 * #682) — or `data: null` when the product has no booster sheets, which is how the panel
 * knows to render nothing. Public read, so a plain `useQuery`; the refs go in the key so a
 * product-to-product navigation refetches. The payload is priced off the same daily card
 * prices every other catalog read carries, so it shares their staleness (#413). */
export function useProductEvQuery(game: Ref<string>, id: Ref<string>) {
  return useQuery({
    queryKey: ['product-ev', game, id],
    queryFn: ({ signal }) => getProductEv(game.value, id.value, signal),
    staleTime: PRICED_CATALOG_STALE_MS,
  })
}

/** One seeded opening of a sealed product (issue #682). The response is a pure function of
 * `(id, seed, copies)` — the whole point of the seed — so it can never go stale:
 * `staleTime: Infinity` keeps a re-render (or a shared URL revisited) off the network, and
 * a NEW seed is a new key rather than a refetch. `enabled` holds the request until the
 * caller has minted a seed (nothing opens on page load unless a `?pack=` URL supplied one),
 * and `keepPreviousData` holds the last run's cards up while the next one lands so "open
 * another" doesn't flash an empty panel. */
export function usePackOpeningQuery(
  game: Ref<string>,
  id: Ref<string>,
  seed: Ref<number | null>,
  copies: Ref<number>,
) {
  return useQuery({
    queryKey: ['product-open', game, id, seed, copies],
    enabled: computed(() => seed.value !== null),
    queryFn: ({ signal }) =>
      openProduct(game.value, id.value, { seed: seed.value ?? 0, copies: copies.value }, signal),
    placeholderData: keepPreviousData,
    staleTime: Infinity,
  })
}

/** The sealed products a card is found in / can be pulled from (the card-detail
 * "Sealed products" section). Public read, so a plain `useQuery`; refs go in the key so a
 * card-to-card navigation refetches. */
export function useCardSealedQuery(game: Ref<string>, id: Ref<string>) {
  return useQuery({
    queryKey: ['card-sealed', game, id],
    queryFn: () => getCardSealed(game.value, id.value),
    // Product rows with prices — daily cadence (#413).
    staleTime: PRICED_CATALOG_STALE_MS,
  })
}
