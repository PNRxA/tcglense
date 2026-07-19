import { computed, ref, watch, type Ref } from 'vue'
import { useInfiniteQuery } from '@tanstack/vue-query'
import { getCardPrintingsByName, type Card, type CardPage, type OwnedCountsMap } from '@/lib/api'
import { useOwnedCounts } from '@/composables/useCollection'
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
 *
 * `opts.collectionFilter` opts a caller (the deck add box and the change-printing dialog)
 * into a "limit to cards in my collection" toggle: when on, the loaded printings are also
 * narrowed to the ones the signed-in user owns. It shares the same loaded-page scope as the
 * text filter, and its owned-count lookup is fetched lazily (only while the toggle is on) and
 * cached, so flipping it back on is instant.
 */
export function usePrintingPicker(
  game: Ref<string>,
  name: Ref<string>,
  opts: { enabled?: Ref<boolean>; collectionFilter?: boolean } = {},
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

  // --- Optional "limit to cards in my collection" filter (opt-in) ---
  // The owned-count lookup is fetched only while the toggle is on (and the picker is
  // enabled), keyed on the loaded printings so a newly-loaded page refetches for the wider
  // set. It's the browse-badge counts hook, so held cards are present and unheld ones absent.
  const collectionFilterEnabled = opts.collectionFilter ?? false
  const collectionOnly = ref(false)
  const collectionQueryEnabled = computed(() => enabled.value && collectionOnly.value)
  const counts = collectionFilterEnabled
    ? useOwnedCounts(game, printings, { enabled: collectionQueryEnabled })
    : null
  const ownership = computed<OwnedCountsMap>(() => counts?.ownership.value ?? {})
  const collectionActive = computed(() => collectionFilterEnabled && collectionOnly.value)
  // Ownership still resolving after the toggle flips on — a caller shows a "checking your
  // collection" state instead of briefly reading the empty pre-fetch map as "none owned".
  const collectionFilterLoading = computed(
    () => collectionActive.value && !(counts?.ready.value ?? true),
  )

  const filteredPrintings = computed<Card[]>(() => {
    const byText = filterPrintings(printings.value, filter.value)
    if (!collectionActive.value) return byText
    return byText.filter((card) => {
      const held = ownership.value[card.id]
      return held !== undefined && held.quantity + held.foil_quantity > 0
    })
  })
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
  // The collection toggle clears back to its default-off only when the game changes (the
  // loaded printings and the meaning of "owned" both change). It deliberately survives a
  // name change so the deck add box keeps the toggle set across successive card picks.
  watch(game, () => {
    collectionOnly.value = false
  })

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
    collectionOnly,
    collectionFilterLoading,
  }
}
