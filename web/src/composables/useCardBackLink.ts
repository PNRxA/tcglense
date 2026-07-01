import { computed, ref, type Ref } from 'vue'
import { onBeforeRouteUpdate, useRouter } from 'vue-router'
import type { Card } from '@/lib/api'

// Labels for the catalog list routes a card is reachable from, keyed by route name.
const FROM_LABELS: Record<string, string> = {
  'game-cards': 'All cards',
  game: 'All sets',
}

/**
 * The card-detail "back" link, mirroring the in-app location the user arrived from
 * rather than always pointing at the set (issue #18). vue-router records the previous
 * entry's path in history state (null on a direct load or a freshly-opened tab); we
 * resolve it to a route so the link can reflect the actual path — the all-cards list,
 * a search, or a set page. Held in a ref and refreshed on each card→card navigation
 * (e.g. clicking another printing), since this view is reused across those routes so
 * setup() won't re-run — captured once, the link would stay frozen on the first
 * card's referrer (issue #63). Falls back to the card's set otherwise.
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

  return computed(() => {
    const from = cameFromRoute.value
    // Honour the previous page only when it's an in-app catalog list for this game;
    // a deep link or an unrelated referrer falls through to the set page below.
    if (from && from.params.game === game.value) {
      if (from.name === 'set') {
        // Came from a set page: every card there is in that set, so the set name fits.
        return { to: from.fullPath, label: card.value?.set_name ?? 'Set' }
      }
      const label = FROM_LABELS[from.name as string]
      if (label) return { to: from.fullPath, label }
    }
    return {
      to: card.value ? `/cards/${game.value}/sets/${card.value.set_code}` : `/cards/${game.value}`,
      label: card.value?.set_name ?? 'Back',
    }
  })
}
