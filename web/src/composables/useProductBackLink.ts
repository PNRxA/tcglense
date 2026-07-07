import { computed, ref, type Ref } from 'vue'
import { onBeforeRouteUpdate, useRouter } from 'vue-router'

/**
 * The sealed-product "back" link, mirroring the in-app location the user arrived from
 * rather than always pointing at the sealed browse (issue #203). A product page is
 * reached three ways: from the per-game sealed browse (a product tile), from a card's
 * "Sealed products" section (the card's full detail page, or the browse-grid card modal
 * `?card=<id>` which can sit over any list route), or from another sealed product's
 * "What's in the box" section (a linked sub-product it contains). vue-router records the previous
 * entry's path in history state (null on a direct load or a freshly-opened tab); we
 * resolve it so "back" returns to exactly that page — re-opening the card modal, or
 * preserving the sealed browse's search/filter/page state — and fall back to the
 * per-game sealed browse otherwise.
 *
 * Held in a ref and refreshed on each product→product navigation, mirroring
 * useCardBackLink: this view is reused across the `sealed-product` route, so setup()
 * won't re-run and a captured referrer would otherwise freeze on the first product's.
 */
export function useProductBackLink(game: Ref<string>) {
  const router = useRouter()

  const cameFrom = ref(router.options.history.state.back)
  onBeforeRouteUpdate((_to, from) => {
    cameFrom.value = from.fullPath
  })
  const cameFromRoute = computed(() =>
    typeof cameFrom.value === 'string' ? router.resolve(cameFrom.value) : null,
  )

  // The default target: the per-game sealed browse (the previous hard-coded link).
  const fallback = computed(() => ({ to: `/sealed/${game.value}`, label: 'Sealed products' }))

  return computed(() => {
    const from = cameFromRoute.value
    if (!from) return fallback.value

    // Opened from a card — its full detail page (route `card`) or the browse-grid modal
    // (`?card=<id>` over a list). Back returns there, keeping the full path so the modal
    // re-opens over the exact list it sat on.
    if (from.name === 'card' || 'card' in from.query) {
      return { to: from.fullPath, label: 'Card' }
    }

    // Opened from another sealed product's "What's in the box" (a linked sub-product it
    // contains) — back returns to that product's page.
    if (from.name === 'sealed-product') {
      return { to: from.fullPath, label: 'Sealed product' }
    }

    // Opened from the sealed browse — return to it with its search/filter/page intact.
    if (from.name === 'game-sealed') {
      return { to: from.fullPath, label: 'Sealed products' }
    }

    return fallback.value
  })
}
