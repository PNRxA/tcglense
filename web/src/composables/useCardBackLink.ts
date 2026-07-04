import { computed, ref, type Ref } from 'vue'
import { onBeforeRouteUpdate, useRouter, type RouteLocationResolved } from 'vue-router'
import type { Card } from '@/lib/api'

// The card-detail page is shared by every section that lists cards — the catalog, a
// collection, and a wish list all open it (via the browse-grid modal's "Open full
// page"). Each of those list routes carries a `:game` param, so the previous page can
// be honoured whichever section it came from. Non-set list routes get a fixed label;
// set-scoped routes fall through to the card's own set name below.
const FROM_LABELS: Record<string, string> = {
  'game-cards': 'All cards',
  game: 'All sets',
  'game-collection-cards': 'Collection',
  'game-collection': 'Collection',
  'wishlist-cards': 'Wish list',
  'game-wishlist': 'Wish list',
}

// List routes scoped to a single set (catalog / collection / wish list): every card on
// them belongs to that set, so the card's set name is the natural back-link label.
const SET_SCOPED_ROUTES = new Set(['set', 'game-collection-set', 'wishlist-set'])

/**
 * The card-detail "back" link, mirroring the in-app location the user arrived from
 * rather than always pointing at the set (issue #18). vue-router records the previous
 * entry's path in history state (null on a direct load or a freshly-opened tab); we
 * resolve it to a route so the link can reflect the actual path — the catalog, a
 * collection, or a wish list (all cards, a set, or a search). Held in a ref and
 * refreshed on each card→card navigation (e.g. clicking another printing), since this
 * view is reused across those routes so setup() won't re-run — captured once, the link
 * would stay frozen on the first card's referrer (issue #63). Falls back to the card's
 * set otherwise.
 *
 * A card is reached from a list via the browse-grid modal (`?card=<id>`), so the
 * referrer path carries that modal param; we strip it so "back" returns to the clean
 * list underneath rather than re-opening the modal over it.
 */
export function useCardBackLink(game: Ref<string>, card: Ref<Card | undefined>) {
  const router = useRouter()

  const cameFrom = ref(router.options.history.state.back)
  onBeforeRouteUpdate((_to, from) => {
    cameFrom.value = from.fullPath
  })
  const cameFromRoute = computed(() =>
    typeof cameFrom.value === 'string' ? router.resolve(cameFrom.value) : null,
  )

  // The referrer path minus the browse-grid modal's `?card=<id>` — back should land on
  // the list itself, not re-open the card modal. Other list state (page, search, sort,
  // ghosts…) is preserved.
  const backPath = (from: RouteLocationResolved) => {
    if (!('card' in from.query)) return from.fullPath
    const query = { ...from.query }
    delete query.card
    return router.resolve({ path: from.path, query, hash: from.hash }).fullPath
  }

  return computed(() => {
    const from = cameFromRoute.value
    // Honour the previous page only when it's an in-app card list for this game;
    // a deep link or an unrelated referrer falls through to the set page below.
    if (from && from.params.game === game.value) {
      if (SET_SCOPED_ROUTES.has(from.name as string)) {
        // A set-scoped list: every card there is in that set, so the set name fits.
        return { to: backPath(from), label: card.value?.set_name ?? 'Set' }
      }
      const label = FROM_LABELS[from.name as string]
      if (label) return { to: backPath(from), label }
    }
    return {
      to: card.value ? `/cards/${game.value}/sets/${card.value.set_code}` : `/cards/${game.value}`,
      label: card.value?.set_name ?? 'Back',
    }
  })
}
