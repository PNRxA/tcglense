<script setup lang="ts">
import { computed, toRef } from 'vue'
import { RouterLink } from 'vue-router'
import ProductSetTile from '@/components/products/ProductSetTile.vue'
import { buttonVariants } from '@/components/ui/button'
import {
  useCollectionProductSetsQuery,
  useCollectionProductSummaryQuery,
} from '@/composables/useCollection'
import {
  useWishlistProductSetsQuery,
  useWishlistProductSummaryQuery,
} from '@/composables/useWishlist'
import { useSetsQuery } from '@/composables/useCatalog'
import { useCurrency } from '@/composables/useCurrency'
import type { CardListTarget } from '@/composables/useOwnedCountEditor'
import type { CardSet } from '@/lib/api'

// The sealed-products slice of the collection / wish-list landing: like the CARDS side, it
// shows the set tiles you click into (a set-scoped products list) rather than rendering the
// products inline. One tile per set the user holds sealed products in, newest set first.
const props = defineProps<{ game: string; list: CardListTarget }>()
const game = toRef(props, 'game')
const money = useCurrency()

const basePath = computed(() => (props.list === 'wishlist' ? '/wishlist' : '/collection'))
const countNoun = computed(() => (props.list === 'wishlist' ? 'wanted' : 'owned'))

// Every held-product set (unpaginated, newest set first) — the tiles. The section renders only
// once this has at least one set, so an empty holding shows nothing.
const setsQuery =
  props.list === 'wishlist'
    ? useWishlistProductSetsQuery(game)
    : useCollectionProductSetsQuery(game)
const sets = computed(() => setsQuery.data.value?.data ?? [])

// The header count is the unique-product tally from the surface's product summary. The landing
// already mounts this query, so vue-query dedupes; while it's pending the count span self-hides.
const summaryQuery =
  props.list === 'wishlist'
    ? useWishlistProductSummaryQuery(game)
    : useCollectionProductSummaryQuery(game)
const summary = computed(() => summaryQuery.data.value)

// The public (cached) catalog set list — the same source the card landing uses — resolves each
// held-set code to its catalog row for the tile's icon + release date. A held set with no
// catalog row is simply absent from the map (the tile falls back gracefully).
const catalogSetsQuery = useSetsQuery(game)
const catalogSetByCode = computed(() => {
  const map: Record<string, CardSet> = {}
  for (const set of catalogSetsQuery.data.value?.data ?? []) map[set.code] = set
  return map
})
</script>

<template>
  <section v-if="sets.length">
    <div class="mb-4 flex items-center justify-between gap-2">
      <h2 class="text-lg font-semibold">
        Sealed products
        <span v-if="summary" class="text-muted-foreground ml-1 text-sm font-normal">
          {{ summary.unique_products }} {{ countNoun }}
        </span>
      </h2>
      <div class="flex items-center gap-2">
        <RouterLink
          :to="`/sealed/${game}`"
          :class="buttonVariants({ variant: 'outline', size: 'sm' })"
        >
          Browse sealed
        </RouterLink>
        <RouterLink :to="`${basePath}/${game}/products`" :class="buttonVariants({ size: 'sm' })">
          View all
        </RouterLink>
      </div>
    </div>

    <!-- One tile per held-product set (server order = newest set first), each linking to the
         surface's set-scoped products list — matching the card landing's held-sets grid. -->
    <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
      <ProductSetTile
        v-for="set in sets"
        :key="set.code"
        :game="game"
        :set="set"
        :catalog-set="catalogSetByCode[set.code]"
        :to="`${basePath}/${game}/products/sets/${set.code}`"
        :value="money.formatUsd(set.total_value_usd)"
      />
    </div>
  </section>
</template>
