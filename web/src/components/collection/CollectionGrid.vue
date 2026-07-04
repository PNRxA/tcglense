<script setup lang="ts">
import { computed } from 'vue'
import type { CollectionEntry, OwnedCountsMap } from '@/lib/api'
import CardTile from '@/components/cards/CardTile.vue'
import OwnedCountControl from '@/components/cards/OwnedCountControl.vue'
import OwnedMarkBadge from '@/components/cards/OwnedMarkBadge.vue'
import type { CardListTarget } from '@/composables/useOwnedCountEditor'
import { CARD_SIZE_GRID_CLASS } from '@/lib/cardSize'
import { useCardSizeStore } from '@/stores/cardSize'

// Same density-follows-preference grid as CardGrid, but each tile carries the quick-add
// control (issue #95) seeded from the owned entry, so counts can be adjusted inline
// without opening the card page. This view only renders for signed-in users. `list`
// retargets the controls at the wish list (issue #167) — the entries are then wish-list
// holdings, edited in place the same way.
const props = withDefaults(
  defineProps<{
    game: string
    entries: CollectionEntry[]
    list?: CardListTarget
    // Collection-ownership counts keyed by card id (issue #213): flags wishlisted cards you
    // already own in your collection with an "Owned" marker, under the wish list's "Show
    // owned" setting. Absent (undefined) on a plain collection grid, so no marker renders.
    ownedMarks?: OwnedCountsMap
  }>(),
  { list: 'collection', ownedMarks: undefined },
)

const cardSize = useCardSizeStore()
const gridClass = computed(() => CARD_SIZE_GRID_CLASS[cardSize.size])

function isOwnedMark(entry: CollectionEntry): boolean {
  const owned = props.ownedMarks?.[entry.card.id]
  return !!owned && owned.quantity + owned.foil_quantity > 0
}
</script>

<template>
  <div class="grid gap-x-4 gap-y-6" :class="gridClass">
    <CardTile v-for="entry in entries" :key="entry.card.id" :game="game" :card="entry.card">
      <template #badge>
        <OwnedMarkBadge v-if="isOwnedMark(entry)" />
        <OwnedCountControl
          :game="game"
          :card-id="entry.card.id"
          :name="entry.card.name"
          :quantity="entry.quantity"
          :foil-quantity="entry.foil_quantity"
          :list="list"
        />
      </template>
    </CardTile>
  </div>
</template>
