import { computed, onUnmounted, ref, watch, type Ref } from 'vue'
import { useRouter } from 'vue-router'
import { keepPreviousData, useQuery } from '@tanstack/vue-query'
import { searchCatalog, type ApiError, type SearchResults } from '@/lib/api'
import {
  SEARCH_GROUP_LIMIT,
  SEARCH_MIN_CHARS,
  buildSearchGroups,
  searchAllOption,
  type SearchOption,
} from '@/lib/universalSearch'
import { useDecksQuery } from '@/composables/useDecks'
import { useAuthStore } from '@/stores/auth'

/** How long the box waits after the last keystroke before asking the API — the quick-add
 * box's own pause. */
export const SEARCH_DEBOUNCE_MS = 250

/** The universal search read (`GET /api/games/{game}/search`): one request per debounced
 * term, gated on the same minimum length as the quick-add autocomplete so a lone letter
 * never fires a catalog-wide match. A blank `game` (the registry still loading) also gates
 * it. Public, so a plain `useQuery`; the last answer stays up while the next one loads. */
export function useCatalogSearchQuery(game: Ref<string>, term: Ref<string>) {
  const trimmed = computed(() => term.value.trim())
  const enabled = computed(() => game.value !== '' && trimmed.value.length >= SEARCH_MIN_CHARS)
  return useQuery<SearchResults, ApiError>({
    // Refs go INSIDE the key so a new term (or game) refetches rather than serving the
    // first answer forever.
    queryKey: ['search', game, trimmed],
    queryFn: ({ signal }) => searchCatalog(game.value, trimmed.value, SEARCH_GROUP_LIMIT, signal),
    enabled,
    placeholderData: keepPreviousData,
    // Catalog names change at most daily; a minute spares the refetch when the visitor
    // backspaces to a term they just typed.
    staleTime: 60_000,
  })
}

/** What the dropdown is showing, beyond its rows: nothing yet (too short a term), the
 * first answer in flight, the API unreachable, nothing matched, or matches. */
export type SearchStatus = 'idle' | 'pending' | 'error' | 'empty' | 'results'

/**
 * The homepage search box's engine: the live text, its debounced query, the grouped option
 * list (the API's groups plus the signed-in user's own decks), the open/highlight state,
 * and the keyboard contract of a combobox — so `UniversalSearchBox.vue` stays a template.
 *
 * The user's decks come from the deck list the app already caches (`useDecksQuery`, the
 * `['decks', game]` entry), filtered client-side by the API's own name rule (see
 * `lib/universalSearch.ts`): the search read is public and identical for every visitor,
 * so per-user rows must never ride it. That query is gated on a ready term as well as on
 * being signed in, so merely landing on the homepage never fetches a deck list.
 */
export function useUniversalSearch(game: Ref<string>) {
  const router = useRouter()
  const auth = useAuthStore()

  // `term` is the live input; `debouncedTerm` is what drives the queries.
  const term = ref('')
  const debouncedTerm = ref('')
  const open = ref(false)
  const activeIndex = ref(-1)

  const trimmed = computed(() => term.value.trim())
  const ready = computed(() => trimmed.value.length >= SEARCH_MIN_CHARS)
  const searchedTerm = computed(() => debouncedTerm.value.trim())

  const query = useCatalogSearchQuery(game, debouncedTerm)
  // `useAuthedQuery` already ANDs "signed in" into `enabled`; this adds "a term to filter".
  const decksQuery = useDecksQuery(
    game,
    computed(() => searchedTerm.value.length >= SEARCH_MIN_CHARS),
  )

  const groups = computed(() =>
    searchedTerm.value.length >= SEARCH_MIN_CHARS
      ? buildSearchGroups({
          game: game.value,
          term: searchedTerm.value,
          results: query.data.value,
          decks: auth.isAuthenticated ? decksQuery.data.value?.data : undefined,
        })
      : [],
  )
  const footer = computed<SearchOption | null>(() =>
    searchedTerm.value.length >= SEARCH_MIN_CHARS
      ? searchAllOption(game.value, searchedTerm.value)
      : null,
  )
  /** Every row in keyboard order: the groups' rows, then the closing search-all row. */
  const options = computed<SearchOption[]>(() => {
    const rows = groups.value.flatMap((group) => group.options)
    if (footer.value) rows.push(footer.value)
    return rows
  })
  const optionIndex = computed(
    () => new Map(options.value.map((option, index) => [option.key, index])),
  )
  const activeOption = computed<SearchOption | null>(() => options.value[activeIndex.value] ?? null)

  // "In flight" while the debounce hasn't caught up with the live text, or the request is
  // running — so the box shows a spinner rather than a premature "no matches".
  const pending = computed(
    () => ready.value && (query.isFetching.value || trimmed.value !== searchedTerm.value),
  )
  const hasResults = computed(() => groups.value.length > 0)
  const status = computed<SearchStatus>(() => {
    if (!ready.value) return 'idle'
    if (hasResults.value) return 'results'
    if (pending.value && !query.data.value) return 'pending'
    if (query.isError.value) return 'error'
    if (query.data.value) return 'empty'
    return 'pending'
  })
  const showDropdown = computed(() => open.value && ready.value)

  let debounceTimer: ReturnType<typeof setTimeout> | undefined
  let blurTimer: ReturnType<typeof setTimeout> | undefined

  watch(term, (value) => {
    clearTimeout(debounceTimer)
    debounceTimer = setTimeout(() => {
      debouncedTerm.value = value
    }, SEARCH_DEBOUNCE_MS)
    // Typing (re)opens the dropdown once the term is long enough.
    if (value.trim().length >= SEARCH_MIN_CHARS) open.value = true
  })

  // A fresh option list invalidates the old highlight (its index may not exist now).
  watch(options, () => {
    activeIndex.value = -1
  })

  onUnmounted(() => {
    clearTimeout(debounceTimer)
    clearTimeout(blurTimer)
  })

  function onFocus() {
    clearTimeout(blurTimer)
    if (ready.value) open.value = true
  }

  function onBlur() {
    // Delay closing so an option's mousedown→click lands first (the option prevents its
    // own mousedown default, so a click keeps focus; a click elsewhere closes here).
    blurTimer = setTimeout(() => {
      open.value = false
    }, 120)
  }

  function close() {
    open.value = false
    activeIndex.value = -1
  }

  function highlight(key: string) {
    activeIndex.value = optionIndex.value.get(key) ?? -1
  }

  /** Go where a row points and put the dropdown away. */
  function pick(option: SearchOption) {
    close()
    router.push(option.to)
  }

  /** Enter with nothing highlighted: the full card search for what was typed. Uses the
   * live text, not the debounced one, so a fast typist's last letters aren't dropped. */
  function submit() {
    if (!ready.value || game.value === '') return
    pick(searchAllOption(game.value, trimmed.value))
  }

  function onKeydown(event: KeyboardEvent) {
    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault()
        if (!showDropdown.value) {
          if (ready.value) open.value = true
          return
        }
        if (options.value.length) {
          activeIndex.value = Math.min(activeIndex.value + 1, options.value.length - 1)
        }
        break
      case 'ArrowUp':
        event.preventDefault()
        if (options.value.length) {
          activeIndex.value = Math.max(activeIndex.value - 1, 0)
        }
        break
      case 'Enter': {
        event.preventDefault()
        const choice = showDropdown.value ? activeOption.value : null
        if (choice) pick(choice)
        else submit()
        break
      }
      case 'Escape':
        if (showDropdown.value) {
          event.preventDefault()
          close()
        }
        break
    }
  }

  return {
    term,
    searchedTerm,
    open,
    showDropdown,
    groups,
    footer,
    options,
    activeIndex,
    activeOption,
    pending,
    status,
    onFocus,
    onBlur,
    onKeydown,
    highlight,
    pick,
    submit,
    close,
  }
}
