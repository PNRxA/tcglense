<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
import { ChevronDown } from '@lucide/vue'
import { useQuery } from '@tanstack/vue-query'
import { getCardPrints } from '@/lib/api'
import { PRICED_CATALOG_STALE_MS } from '@/lib/queryClient'
import CardGrid from '@/components/cards/CardGrid.vue'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import CardSortMenu from '@/components/cards/CardSortMenu.vue'
import { useOwnedCounts } from '@/composables/useCollection'
import { useWishlistCounts } from '@/composables/useWishlist'
import { filterPrintings } from '@/lib/quickAddFilter'
import { PRINTING_DEFAULT_SORT, PRINTING_SORT_OPTIONS, sortPrintings } from '@/lib/printingSort'

const props = defineProps<{ game: string; id: string }>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')

// Public prints endpoint, so a plain useQuery (no auth wrapper). Refs go straight
// into the queryKey so a card-to-card navigation (e.g. clicking another printing)
// refetches for the new card.
const query = useQuery({
  queryKey: ['card-prints', game, id],
  queryFn: () => getCardPrints(game.value, id.value),
  // Prints embed Card.prices, which move only on the daily sync (#413).
  staleTime: PRICED_CATALOG_STALE_MS,
})

const prints = computed(() => query.data.value?.data ?? [])
// Owned-count badges for signed-in users, overlaid on the printings grid.
const { ownership } = useOwnedCounts(game, prints)
// Wish-list wanted counts for the same prints — a Heart chip flags wishlisted cards (#364).
const { ownership: wishlistOwnership } = useWishlistCounts(game, prints)

// Filter + sort over the other printings (issue #472). The `/prints` endpoint returns the
// full list in one response, so both operate client-side over every printing — no
// loaded-only caveat like the paginated picker has. The filter mirrors the shared picker's
// (`filterPrintings`); the sort is the same shared client-side reordering.
const filter = ref('')
const sort = ref(PRINTING_DEFAULT_SORT)
const visiblePrints = computed(() =>
  sortPrintings(filterPrintings(prints.value, filter.value), sort.value),
)

// Collapsed by default (issue #332), matching the sealed product page's card sections:
// the heading is a disclosure toggle showing the printing count, so a card with many
// reprints doesn't push a long grid onto the page until asked. Section-local — the
// component is reused across card-to-card navigation, so re-collapse and clear the filter
// (the sort is a harmless preference to keep) when the id changes.
const expanded = ref(false)
watch(id, () => {
  expanded.value = false
  filter.value = ''
})
</script>

<template>
  <!-- Hidden entirely until there's at least one other printing to show, so a
    one-printing card (the common case) adds nothing to the page. -->
  <section v-if="prints.length" class="mt-10">
    <button
      type="button"
      class="group -mx-1.5 mb-3 flex items-start gap-1.5 rounded-md px-1.5 py-1 text-left"
      :aria-expanded="expanded"
      @click="expanded = !expanded"
    >
      <ChevronDown
        class="text-muted-foreground group-hover:text-foreground mt-0.5 size-4 shrink-0 transition-transform"
        :class="expanded ? 'rotate-180' : ''"
      />
      <h2 class="text-sm font-semibold">
        Other printings
        <span class="text-muted-foreground font-normal">({{ prints.length }})</span>
      </h2>
    </button>
    <template v-if="expanded">
      <div v-if="prints.length > 1" class="mb-4 flex flex-wrap items-center gap-2">
        <CardSearchBox
          v-model="filter"
          class="w-full sm:w-72"
          placeholder="Filter by set, number, or rarity…"
          aria-label="Filter printings by set, number, or rarity"
        />
        <CardSortMenu v-model="sort" :options="PRINTING_SORT_OPTIONS" />
      </div>
      <p v-if="visiblePrints.length === 0" class="text-muted-foreground py-8 text-center text-sm">
        No printings match “{{ filter.trim() }}”.
      </p>
      <CardGrid
        v-else
        :game="game"
        :cards="visiblePrints"
        :ownership="ownership"
        :wishlist="wishlistOwnership"
      />
    </template>
  </section>
</template>
