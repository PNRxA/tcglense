import { computed, onUnmounted, ref, watch } from 'vue'
import { useRoute, useRouter, type LocationQueryRaw } from 'vue-router'

function readString(value: unknown): string {
  return typeof value === 'string' ? value : ''
}

/**
 * The sealed-product "Cards in this product" search (issue #222), backed by the URL `?q=`
 * so it survives opening a card from the results and pressing Back (the issue #58 idiom the
 * catalog already uses) and is shareable/reload-safe. Trimmed from the catalog's
 * `useCardSearch` to just `q` — each card section owns its own page, so there's no shared
 * page/sort to track here.
 *
 * `searchInput` is the live text box, debounced 300ms into the committed `query` (the URL);
 * a blank query drops the key (a clean canonical URL). Navigating to a different product
 * (the path changes) cancels a half-typed search and resyncs the box to the destination, and
 * a Back/forward that changes `?q=` under us is mirrored back into the box.
 */
export function useProductCardsSearch() {
  const route = useRoute()
  const router = useRouter()

  const query = computed(() => readString(route.query.q).trim())

  // Commit into the URL, preserving any unrelated query keys; replace (never push) so
  // searching doesn't pile up history entries between the product and a card opened from it.
  function commit(value: string) {
    const next: LocationQueryRaw = { ...route.query }
    if (value) next.q = value
    else delete next.q
    if (String(next.q ?? '') !== String(route.query.q ?? '')) router.replace({ query: next })
  }

  // The text box mirrors the committed query, debounced so we don't rewrite the URL on every
  // keystroke. Seed it from the current URL (a shared/reloaded link).
  const searchInput = ref(query.value)
  let timer: ReturnType<typeof setTimeout> | undefined
  watch(searchInput, (value) => {
    clearTimeout(timer)
    timer = setTimeout(() => {
      const trimmed = value.trim()
      // Guard against re-committing an unchanged value (e.g. the box was just synced below).
      if (trimmed !== query.value) commit(trimmed)
    }, 300)
  })
  onUnmounted(() => clearTimeout(timer))

  // A different product (the path changes) must not carry a half-typed, not-yet-committed
  // search across: cancel the pending debounce and resync the box to wherever we landed.
  watch(
    () => route.path,
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

  return { searchInput, query }
}
