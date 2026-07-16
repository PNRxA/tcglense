import { computed, onUnmounted, ref, toValue, watch, type MaybeRefOrGetter } from 'vue'
import { useRoute, useRouter, type LocationQuery, type LocationQueryRaw } from 'vue-router'

function readString(value: unknown): string {
  return typeof value === 'string' ? value : ''
}

/** Compare a candidate query against the live route query, treating absent, empty and null
 * the same, so we never `replace` to an identical URL. */
function queriesEqual(a: LocationQueryRaw, b: LocationQuery): boolean {
  const keys = new Set([...Object.keys(a), ...Object.keys(b)])
  for (const key of keys) {
    if (String(a[key] ?? '') !== String(b[key] ?? '')) return false
  }
  return true
}

/** The pair of URL query keys one "Cards in this product" list rides. Which pair a caller takes
 * depends on whether it owns the route it renders over — see the two constants below. */
export interface ProductCardsSearchKeys {
  /** Holds the committed Scryfall-style filter. */
  q: string
  /** Holds the `field:dir` sort. */
  sort: string
}

/** The keys for the full product page (`/sealed/:game/:id`, SealedProductView). It is the only
 * consumer of `?q=`/`?sort=` on its own route, so it takes the plain names — the ones existing
 * links and bookmarks already carry. */
export const PRODUCT_CARDS_SEARCH_KEYS: ProductCardsSearchKeys = { q: 'q', sort: 'sort' }

/** The keys for the detail modal (ProductDetailDialog). It overlays a *browse* route
 * (`/sealed/:game`) whose own list controls (`useCardSearch`) already own `?q=`/`?sort=`, and
 * neither surface can see the other — so sharing the plain names silently crosses them: the
 * modal would read the browse's product-name search as its Scryfall card filter (the two sort
 * option sets overlap, so a borrowed sort wouldn't even fail the clamp), and every modal
 * search/sort write would destroy the list state underneath. Namespacing keeps the modal's card
 * search deep-linkable and shareable while leaving the browse untouched. These keys belong to
 * the open *product*, not the overlay session: every transition that changes or removes
 * `?product=` strips them — a prev/next step and close (DetailDialogShell), the surface swaps
 * (CardTile/ProductTile) — and only a deep link arriving with `?product=` keeps them (#448). */
export const PRODUCT_CARDS_MODAL_SEARCH_KEYS: ProductCardsSearchKeys = { q: 'pq', sort: 'psort' }

/**
 * The sealed-product "Cards in this product" list controls (issue #222 search + the sort that
 * came with the filter-helper/size/sort parity work), backed by the URL so they survive opening
 * a card from the results and pressing Back (the issue #58 idiom the catalog already uses) and
 * are shareable/reload-safe. Trimmed from the catalog's `useCardSearch`: there is no shared
 * `page` to track — each card section owns and resets its own pagination — so this carries only
 * the search and the sort.
 *
 * `searchInput` is the live text box, debounced 300ms into the committed `query` (the URL
 * `keys.q`); a blank query drops the key (a clean canonical URL). `sort` is a writable
 * `field:dir` option value backed by `keys.sort`, clamped to `validSorts` (an unknown/hand-edited
 * value falls back to `defaultSort`), and dropped from the URL when it equals `defaultSort`.
 * Navigating to a different product (`id` changes) cancels a half-typed search and resyncs
 * the box to the destination, and a Back/forward that changes the committed query under us is
 * mirrored back into the box.
 *
 * `keys` names the two params, so a list rendered over a route that already owns `?q=`/`?sort=`
 * can namespace itself out of the way ({@link PRODUCT_CARDS_MODAL_SEARCH_KEYS}). Every other key
 * in the query — the host route's included — is preserved on write either way.
 *
 * `id` is the product whose cards the list shows — the signal a "different product" is read
 * from. It must NOT be inferred from `route.path`: the full page does change its path when it
 * steps products, but the modal steps by rewriting only `?product=` (DetailDialogShell.goTo),
 * so a path watch never fires there and a half-typed search leaks across (issue #448).
 *
 * `id`/`defaultSort`/`validSorts` may be plain values or refs/getters, matching `useCardSearch`.
 */
export function useProductCardsSearch(
  id: MaybeRefOrGetter<string>,
  defaultSort: MaybeRefOrGetter<string> = '',
  validSorts?: MaybeRefOrGetter<readonly string[] | undefined>,
  keys: ProductCardsSearchKeys = PRODUCT_CARDS_SEARCH_KEYS,
) {
  const route = useRoute()
  const router = useRouter()

  // Merge changes into the URL query, preserving any unrelated query keys; an undefined/empty
  // value drops its key. Replace (never push) so searching/sorting doesn't pile up history
  // entries between the product and a card opened from it.
  function patch(changes: Record<string, string | undefined>) {
    const next: LocationQueryRaw = { ...route.query }
    for (const [key, value] of Object.entries(changes)) {
      if (value === undefined || value === '') delete next[key]
      else next[key] = value
    }
    if (!queriesEqual(next, route.query)) router.replace({ query: next })
  }

  const query = computed(() => readString(route.query[keys.q]).trim())

  const sort = computed({
    get: () => {
      const raw = readString(route.query[keys.sort])
      const valid = toValue(validSorts)
      return raw && (!valid || valid.includes(raw)) ? raw : toValue(defaultSort)
    },
    // The default sort rides the URL implicitly (drop the key); each section resets its own
    // page when the sort changes, so there's no shared page to reset here.
    set: (value) =>
      patch({ [keys.sort]: value && value !== toValue(defaultSort) ? value : undefined }),
  })

  // The text box mirrors the committed query, debounced so we don't rewrite the URL on every
  // keystroke. Seed it from the current URL (a shared/reloaded link).
  const searchInput = ref(query.value)
  let timer: ReturnType<typeof setTimeout> | undefined
  watch(searchInput, (value) => {
    clearTimeout(timer)
    timer = setTimeout(() => {
      const trimmed = value.trim()
      // Guard against re-committing an unchanged value (e.g. the box was just synced below).
      if (trimmed !== query.value) patch({ [keys.q]: trimmed || undefined })
    }, 300)
  })
  onUnmounted(() => clearTimeout(timer))

  // A different product must not carry a half-typed, not-yet-committed search across: cancel
  // the pending debounce and resync the box to wherever we landed. Keyed on the product id, not
  // `route.path` — in the modal, stepping products rewrites only the query (issue #448).
  watch(
    () => toValue(id),
    () => {
      clearTimeout(timer)
      searchInput.value = query.value
    },
  )

  // The committed query can also change under us without a path change (Back/forward, or a
  // programmatic update). Mirror it into the box.
  watch(query, (value) => {
    if (value !== searchInput.value.trim()) searchInput.value = value
  })

  return { searchInput, query, sort }
}
