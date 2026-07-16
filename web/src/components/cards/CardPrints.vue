<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
import { ChevronDown } from '@lucide/vue'
import { useQuery } from '@tanstack/vue-query'
import { getCardPrints } from '@/lib/api'
import { PRICED_CATALOG_STALE_MS } from '@/lib/queryClient'
import CardGrid from '@/components/cards/CardGrid.vue'
import { useOwnedCounts } from '@/composables/useCollection'
import { useWishlistCounts } from '@/composables/useWishlist'

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

// Collapsed by default (issue #332), matching the sealed product page's card sections:
// the heading is a disclosure toggle showing the printing count, so a card with many
// reprints doesn't push a long grid onto the page until asked. Section-local — the
// component is reused across card-to-card navigation, so re-collapse when the id changes.
const expanded = ref(false)
watch(id, () => {
  expanded.value = false
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
    <CardGrid
      v-if="expanded"
      :game="game"
      :cards="prints"
      :ownership="ownership"
      :wishlist="wishlistOwnership"
    />
  </section>
</template>
