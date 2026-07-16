import { computed, ref, watch, type Ref } from 'vue'
import { useInfiniteQuery } from '@tanstack/vue-query'
import { getCardPrintingsByName, type Card, type CardPage } from '@/lib/api'
import { filterPrintings } from '@/lib/quickAddFilter'

/**
 * Shared exact-name printing discovery for collection/wish-list quick add, deck add,
 * deck replacement, and the scanner. Every caller uses the same infinite-query key:
 * `['card-printings', game, name]`; pages are accumulated newest-first and fetched at
 * the API's maximum 200-card page size.
 *
 * Filtering is deliberately client-side over the pages already loaded. The shared grid
 * labels that scope whenever more pages remain, so a zero-match result never implies it
 * searched printings the user has not loaded yet.
 */
export function usePrintingPicker(
  game: Ref<string>,
  name: Ref<string>,
  opts: { enabled?: Ref<boolean> } = {},
) {
  const enabled = computed(() => name.value.length > 0 && (opts.enabled?.value ?? true))
  const filter = ref('')

  // A reactive ref belongs directly in the key: changing game/name starts the matching
  // family, while every caller shares the same predictable three-part key shape.
  const query = useInfiniteQuery({
    queryKey: ['card-printings', game, name],
    queryFn: ({ pageParam, signal }) =>
      getCardPrintingsByName(game.value, name.value, pageParam, signal),
    initialPageParam: 1,
    getNextPageParam: (lastPage: CardPage) => (lastPage.has_more ? lastPage.page + 1 : undefined),
    enabled,
    staleTime: 60_000,
  })

  const printings = computed<Card[]>(
    () => query.data.value?.pages.flatMap((page) => page.data) ?? [],
  )
  const filteredPrintings = computed<Card[]>(() => filterPrintings(printings.value, filter.value))
  const total = computed(() => query.data.value?.pages[0]?.total ?? 0)
  const loadedCount = computed(() => printings.value.length)
  const failed = computed(() => query.isError.value || query.isFetchNextPageError.value)

  function resetFilter() {
    filter.value = ''
  }

  async function loadMore(): Promise<void> {
    if (!query.hasNextPage.value || query.isFetchingNextPage.value) return
    await query.fetchNextPage()
  }

  // A new card name and each newly-opened picker start unfiltered. Resetting on enable
  // also covers reopening the same name, which would otherwise reuse the instance state.
  watch([game, name], resetFilter)
  if (opts.enabled) {
    watch(opts.enabled, (isEnabled) => {
      if (isEnabled) resetFilter()
    })
  }

  return {
    ...query,
    filter,
    printings,
    filteredPrintings,
    total,
    loadedCount,
    failed,
    loadMore,
    resetFilter,
  }
}
