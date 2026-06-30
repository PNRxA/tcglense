import { onUnmounted, ref, watch, type Ref } from 'vue'
import { ApiError } from '@/lib/api'

/** Map a failed card query to its 422 (bad Scryfall query) message, else null. */
export function searchErrorMessage(error: unknown): string | null {
  return error instanceof ApiError && error.status === 422 ? error.message : null
}

/**
 * Shared list controls for the catalog card views: a 300ms-debounced `query`, a
 * `sort` (a `field:dir` value, see lib/cardSort), and a `page` that resets to 1
 * whenever the query or sort changes. Navigating to another game/set (`resetOn`
 * changes) resets everything — search, sort and page — back to a clean slate.
 */
export function useCardSearch(resetOn: Ref<unknown>, defaultSort = '') {
  const page = ref(1)
  const searchInput = ref('')
  const query = ref('')
  const sort = ref(defaultSort)

  // Debounce typing into the committed query.
  let timer: ReturnType<typeof setTimeout> | undefined
  watch(searchInput, (value) => {
    clearTimeout(timer)
    timer = setTimeout(() => {
      query.value = value.trim()
    }, 300)
  })
  onUnmounted(() => clearTimeout(timer))

  // A new query or sort always restarts pagination. Driving the query reset off
  // `query` (rather than the debounce timer) keeps a programmatic reset — e.g.
  // clearing the box on navigation below — from arming a stray timer that could
  // later snap page to 1.
  watch([query, sort], () => {
    page.value = 1
  })

  // Navigating to a different game/set starts fresh (search + sort + page).
  watch(resetOn, () => {
    clearTimeout(timer)
    searchInput.value = ''
    query.value = ''
    sort.value = defaultSort
    page.value = 1
  })

  return { page, searchInput, query, sort }
}
