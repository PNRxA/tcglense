import { onUnmounted, computed, ref, toValue, watch, type MaybeRefOrGetter } from 'vue'
import { useRoute, useRouter, type LocationQuery, type LocationQueryRaw } from 'vue-router'
import { ApiError } from '@/lib/api'

/** Map a failed card query to its 422 (bad Scryfall query) message, else null. */
export function searchErrorMessage(error: unknown): string | null {
  return error instanceof ApiError && error.status === 422 ? error.message : null
}

function readString(value: unknown): string {
  return typeof value === 'string' ? value : ''
}

/** A `?page=` value is honoured only when it's an integer past the first page. */
function readPage(value: unknown): number {
  const n = Number(value)
  return Number.isInteger(n) && n > 1 ? n : 1
}

/** Compare a candidate query against the live route query, treating absent,
 * empty and null the same, so we never `replace` to an identical URL. */
function queriesEqual(a: LocationQueryRaw, b: LocationQuery): boolean {
  const keys = new Set([...Object.keys(a), ...Object.keys(b)])
  for (const key of keys) {
    if (String(a[key] ?? '') !== String(b[key] ?? '')) return false
  }
  return true
}

/**
 * Shared list controls for the catalog card views — backed by the URL query so the
 * page, search and sort survive navigating away and back (open a card, press Back —
 * issue #58) and are shareable/bookmarkable/reload-safe, matching how the set view
 * already keeps `related`/`from` in the URL.
 *
 * `searchInput` is the live text box, debounced 300ms into the committed `?q`.
 * `page`, `query` and `sort` read from the route and write back via `router.replace`
 * (so paging/searching doesn't pile up history entries between the list and a card
 * opened from it). Committing a new search or sort restarts paging. Writes merge
 * into the existing query, so unrelated keys (a set view's `related`/`from`) are
 * preserved. `validSorts`, when given, clamps an unknown `?sort=` (e.g. a
 * hand-edited URL) back to the default rather than letting the API reject it.
 *
 * `defaultSort`/`validSorts` may be plain values or refs/getters: a view whose sort
 * set depends on a mode (e.g. the collection view's owned vs. show-ghosts toggle,
 * which swaps the collection sorts for the catalog ones) passes getters so the
 * committed sort re-clamps to the active mode's default when the mode flips.
 */
export function useCardSearch(
  defaultSort: MaybeRefOrGetter<string> = '',
  validSorts?: MaybeRefOrGetter<readonly string[] | undefined>,
) {
  const route = useRoute()
  const router = useRouter()

  // Merge changes into the URL query: an undefined/empty value drops its key, every
  // other (e.g. `related`/`from`) is left untouched. Replace, never push.
  function patch(changes: Record<string, string | undefined>) {
    const next: LocationQueryRaw = { ...route.query }
    for (const [key, value] of Object.entries(changes)) {
      if (value === undefined || value === '') delete next[key]
      else next[key] = value
    }
    if (!queriesEqual(next, route.query)) router.replace({ query: next })
  }

  const query = computed(() => readString(route.query.q).trim())

  const page = computed({
    get: () => readPage(route.query.page),
    set: (value) => patch({ page: value > 1 ? String(value) : undefined }),
  })

  const sort = computed({
    get: () => {
      const raw = readString(route.query.sort)
      const valid = toValue(validSorts)
      return raw && (!valid || valid.includes(raw)) ? raw : toValue(defaultSort)
    },
    // A new sort restarts paging — page 3 of the old order is meaningless in the new.
    set: (value) =>
      patch({ sort: value && value !== toValue(defaultSort) ? value : undefined, page: undefined }),
  })

  // The text box mirrors the committed query, debounced so we don't rewrite the URL
  // on every keystroke. Seed it from the current URL (a shared/reloaded link).
  const searchInput = ref(query.value)
  let timer: ReturnType<typeof setTimeout> | undefined
  watch(searchInput, (value) => {
    clearTimeout(timer)
    timer = setTimeout(() => {
      const trimmed = value.trim()
      // A new search restarts paging. Guard against re-committing an unchanged value
      // (e.g. the box was just synced from the URL below) to avoid a stray replace.
      if (trimmed !== query.value) patch({ q: trimmed || undefined, page: undefined })
    }, 300)
  })
  onUnmounted(() => clearTimeout(timer))

  // Navigating to a different list (a different set/game — the path changes) must not
  // carry a half-typed, not-yet-committed search onto the destination: cancel any
  // pending debounce and resync the box to wherever we landed. A query-only change
  // (paging, sorting, a set view's scope toggle) keeps the same path, so the in-list
  // search keeps debouncing normally.
  watch(
    () => route.path,
    () => {
      clearTimeout(timer)
      searchInput.value = query.value
    },
  )

  // The committed query can also change under us without a path change — landing back
  // on the same list via Back/forward, or a programmatic update. Mirror it into the box.
  watch(query, (value) => {
    if (value !== searchInput.value.trim()) searchInput.value = value
  })

  return { page, searchInput, query, sort }
}
