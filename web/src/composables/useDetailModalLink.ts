import { useRoute, useRouter, type LocationQueryRaw } from 'vue-router'
import { PRODUCT_CARDS_MODAL_SEARCH_KEYS } from '@/composables/useProductCardsSearch'
import { applyDetailOrigin, type DetailOriginKind } from '@/lib/detailOrigin'
import { loadCardDetailDialog } from '@/components/cards/detailDialogLoader'
import { loadProductDetailDialog } from '@/components/products/detailDialogLoader'

// The two detail surfaces the shared modal (mounted once in App.vue) can overlay: a card
// (`?card=<id>`) or a sealed product (`?product=<id>`). Both always live in the same game.
export type DetailModalKind = DetailOriginKind // 'card' | 'product'

// Per surface: the OTHER surface's query key (the item a cross-surface swap leaves behind —
// dropped from the URL and recorded as the "opened from" origin so the modal can offer a
// "← Back to <origin>" crumb), the canonical full page the anchor keeps as its href (so
// modifier/middle click, new-tab, and crawlers still get the real page), and the loader that
// warms the dialog chunk on first hover.
const SURFACES = {
  card: {
    other: 'product',
    path: (game: string, id: string) => `/cards/${game}/cards/${id}`,
    warm: loadCardDetailDialog,
  },
  product: {
    other: 'card',
    path: (game: string, id: string) => `/sealed/${game}/${id}`,
    warm: loadProductDetailDialog,
  },
} as const

// Warm each dialog chunk at most once per session. `import()` already dedupes the request; the
// set skips even the repeat call, mirroring the per-tile module flag it replaces.
const warmed = new Set<DetailModalKind>()

/**
 * Open the shared card/product detail modal over the CURRENT route — the transition CardTile
 * and ProductTile perform from a browse grid, factored into one seam so every surface behaves
 * identically. A plain left-click rewrites the URL query (`?card=`/`?product=`) instead of
 * navigating, so the list underneath keeps its scroll/search/page state and the browser's Back
 * closes the modal. Reused by the sealed-product "What's in the box" / "Included in" link lists
 * so their rows open the same modal rather than leaving the page (issue #485).
 */
export function useDetailModalLink() {
  const route = useRoute()
  const router = useRouter()

  // The canonical full-page URL for an item — the href the anchor keeps for non-plain clicks.
  function hrefFor(kind: DetailModalKind, game: string, id: string): string {
    return router.resolve(SURFACES[kind].path(game, id)).href
  }

  // Rewrite the URL to open `kind`'s modal over the current route (the tile-click transition).
  function open(kind: DetailModalKind, game: string, id: string): void {
    const other = SURFACES[kind].other
    const query: LocationQueryRaw = { ...route.query, [kind]: id }
    // The other surface's open item, if any, is the one this swap leaves behind — dropped from
    // the URL and stashed as the origin so the modal can point one tap back to it.
    const from = typeof route.query[other] === 'string' ? route.query[other] : null
    delete query[other]
    // A namespaced product-card search still in the URL was typed for a now-closed product
    // modal (issue #448); the surface we open starts fresh.
    for (const key of Object.values(PRODUCT_CARDS_MODAL_SEARCH_KEYS)) delete query[key]
    applyDetailOrigin(query, other, from)
    // A route without a `:game` path param (the public deck page) can't feed the shared dialog
    // its game from the path, so carry it in the query — CardTile's/ProductTile's idiom.
    if (typeof route.params.game !== 'string' || !route.params.game) query.game = game
    void router.push({ query })
  }

  // Anchor click handler: leave modifier/middle clicks (and anything already handled) to the
  // browser so the real page still opens; a plain left-click opens the modal in place.
  function onActivate(event: MouseEvent, kind: DetailModalKind, game: string, id: string): void {
    if (event.defaultPrevented) return
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
      return
    }
    event.preventDefault()
    open(kind, game, id)
  }

  // Fire-and-forget prefetch of a surface's dialog chunk on first hover/focus.
  function warm(kind: DetailModalKind): void {
    if (warmed.has(kind)) return
    warmed.add(kind)
    void SURFACES[kind].warm()
  }

  return { hrefFor, open, onActivate, warm }
}
