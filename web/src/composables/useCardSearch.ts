import { onUnmounted, ref, watch, type Ref } from 'vue'
import { ApiError } from '@/lib/api'

/** Map a failed card query to its 422 (bad Scryfall query) message, else null. */
export function searchErrorMessage(error: unknown): string | null {
  return error instanceof ApiError && error.status === 422 ? error.message : null
}

/**
 * Shared search-box state for the catalog list views: a 300ms-debounced `query`,
 * a `page` that resets to 1 on each new query, and a full reset (clear the box +
 * page) whenever `resetOn` changes — e.g. navigating to another game or set.
 */
export function useCardSearch(resetOn: Ref<unknown>) {
  const page = ref(1)
  const searchInput = ref('')
  const query = ref('')

  // Debounce typing into the committed query.
  let timer: ReturnType<typeof setTimeout> | undefined
  watch(searchInput, (value) => {
    clearTimeout(timer)
    timer = setTimeout(() => {
      query.value = value.trim()
    }, 300)
  })
  onUnmounted(() => clearTimeout(timer))

  // A new query always restarts pagination. Driving the reset off `query` (rather
  // than the debounce timer) keeps a programmatic reset — e.g. clearing the box on
  // navigation below — from arming a stray timer that could later snap page to 1.
  watch(query, () => {
    page.value = 1
  })

  // Navigating to a different game/set starts fresh (search + page).
  watch(resetOn, () => {
    clearTimeout(timer)
    searchInput.value = ''
    query.value = ''
    page.value = 1
  })

  return { page, searchInput, query }
}
