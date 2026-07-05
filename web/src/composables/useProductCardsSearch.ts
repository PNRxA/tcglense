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

/**
 * The sealed-product "Cards in this product" list controls (issue #222 search + the sort that
 * came with the filter-helper/size/sort parity work), backed by the URL `?q=`/`?sort=` so they
 * survive opening a card from the results and pressing Back (the issue #58 idiom the catalog
 * already uses) and are shareable/reload-safe. Trimmed from the catalog's `useCardSearch`: there
 * is no shared `page` to track — each card section owns and resets its own pagination — so this
 * carries only the search and the sort.
 *
 * `searchInput` is the live text box, debounced 300ms into the committed `query` (the URL `?q=`);
 * a blank query drops the key (a clean canonical URL). `sort` is a writable `field:dir` option
 * value backed by `?sort=`, clamped to `validSorts` (an unknown/hand-edited value falls back to
 * `defaultSort`), and dropped from the URL when it equals `defaultSort`. Navigating to a different
 * product (the path changes) cancels a half-typed search and resyncs the box to the destination,
 * and a Back/forward that changes `?q=` under us is mirrored back into the box.
 *
 * `defaultSort`/`validSorts` may be plain values or refs/getters, matching `useCardSearch`.
 */
export function useProductCardsSearch(
  defaultSort: MaybeRefOrGetter<string> = '',
  validSorts?: MaybeRefOrGetter<readonly string[] | undefined>,
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

  const query = computed(() => readString(route.query.q).trim())

  const sort = computed({
    get: () => {
      const raw = readString(route.query.sort)
      const valid = toValue(validSorts)
      return raw && (!valid || valid.includes(raw)) ? raw : toValue(defaultSort)
    },
    // The default sort rides the URL implicitly (drop the key); each section resets its own
    // page when the sort changes, so there's no shared page to reset here.
    set: (value) => patch({ sort: value && value !== toValue(defaultSort) ? value : undefined }),
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
      if (trimmed !== query.value) patch({ q: trimmed || undefined })
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

  return { searchInput, query, sort }
}
