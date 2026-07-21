import { onUnmounted, computed, ref, toValue, watch, type MaybeRefOrGetter, type Ref } from 'vue'
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

/** Merge `changes` into the current URL query and `replace` — an undefined/empty value
 * drops its key, every other (e.g. a set view's `related`/`from`) is left untouched. A
 * no-op change is skipped so we never push an identical URL. Shared by the list controls. */
function patchQuery(
  route: ReturnType<typeof useRoute>,
  router: ReturnType<typeof useRouter>,
  changes: Record<string, string | undefined>,
): void {
  const next: LocationQueryRaw = { ...route.query }
  for (const [key, value] of Object.entries(changes)) {
    if (value === undefined || value === '') delete next[key]
    else next[key] = value
  }
  if (!queriesEqual(next, route.query)) router.replace({ query: next })
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
 *
 * `onSortCommit`, when given, returns extra query changes merged into the same atomic
 * `replace` that commits a new sort — the set/holding views use it to leave a grouped
 * (fixed-order) view for the flat sorted grid (`?view=all`) in one write, so the sort
 * and the view flip can't race two separate replaces.
 */
export function useCardSearch(
  defaultSort: MaybeRefOrGetter<string> = '',
  validSorts?: MaybeRefOrGetter<readonly string[] | undefined>,
  onSortCommit?: () => Record<string, string | undefined>,
) {
  const route = useRoute()
  const router = useRouter()

  // Merge changes into the URL query (drop empty keys, leave `related`/`from` untouched).
  const patch = (changes: Record<string, string | undefined>) => patchQuery(route, router, changes)

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
    // `onSortCommit` folds any view-flip (grouped → all cards) into the same write.
    set: (value) =>
      patch({
        sort: value && value !== toValue(defaultSort) ? value : undefined,
        page: undefined,
        ...onSortCommit?.(),
      }),
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

/**
 * The by-drop view's "filter drops by name" box, backed by the `?drop=` URL param so it's
 * shareable, bookmarkable and survives navigating to a card and back — mirroring how
 * {@link useCardSearch} keeps `?q`. Kept separate from that composable because the two
 * filter different units: `q` narrows the cards within each drop, `drop` narrows the drops
 * by their curated Secret Lair title. Only meaningful in the by-drop view — SetView renders
 * the box under the By-drop toggle and passes `dropQuery` into `useSetDropsQuery`.
 *
 * `dropInput` is the live text box, debounced 300ms into the committed `?drop`; `dropQuery`
 * is the committed value the query keys off. Committing a new filter restarts paging (page 3
 * of the old drop list is meaningless once the list narrows), and merges into the existing
 * query so a card view's `q`/scope keys are preserved.
 *
 * `active` (the caller's by-drop flag) lets the box drop a mid-debounce keystroke the moment
 * the view leaves by-drop. Toggling to the flat/related view is a *same-path* query change
 * (`?view=all`) the path watcher below can't see, so without this a half-typed filter would
 * fire ~300ms later and land a phantom `?drop=` on the flat-view URL — where the box is
 * unmounted (`v-if="byDrop"`) and can't clear it.
 */
export function useDropFilter(active?: Ref<boolean>) {
  const route = useRoute()
  const router = useRouter()

  const dropQuery = computed(() => readString(route.query.drop).trim())

  // The box mirrors the committed filter, debounced so we don't refetch on every keystroke.
  // Seed it from the URL (a shared/reloaded link).
  const dropInput = ref(dropQuery.value)
  let timer: ReturnType<typeof setTimeout> | undefined
  watch(dropInput, (value) => {
    clearTimeout(timer)
    timer = setTimeout(() => {
      const trimmed = value.trim()
      // Guard against re-committing an unchanged value (e.g. just synced from the URL).
      if (trimmed !== dropQuery.value)
        patchQuery(route, router, { drop: trimmed || undefined, page: undefined })
    }, 300)
  })
  onUnmounted(() => clearTimeout(timer))

  // Navigating to a different set (the path changes) must not carry a half-typed, not-yet-
  // committed filter across: cancel any pending debounce and resync the box to the URL.
  watch(
    () => route.path,
    () => {
      clearTimeout(timer)
      dropInput.value = dropQuery.value
    },
  )
  // Leaving the by-drop view (a same-path change the path watcher misses) must likewise drop
  // a pending edit and resync — otherwise it would commit a phantom ?drop= onto the flat URL.
  if (active) {
    watch(active, (on) => {
      if (on) return
      clearTimeout(timer)
      dropInput.value = dropQuery.value
    })
  }
  // The committed filter can also change under us without a path change (Back/forward, or
  // toggling to the flat view drops the key). Mirror it into the box.
  watch(dropQuery, (value) => {
    if (value !== dropInput.value.trim()) dropInput.value = value
  })

  return { dropInput, dropQuery }
}
