<script setup lang="ts">
import { computed } from 'vue'
import type { CollectionEntry, OwnedCountsMap } from '@/lib/api'
import CardTile from '@/components/cards/CardTile.vue'
import OwnedCountControl from '@/components/cards/OwnedCountControl.vue'
import OwnedMarkBadge from '@/components/cards/OwnedMarkBadge.vue'
import type { CardListTarget } from '@/composables/useOwnedCountEditor'
import { useCardNavList } from '@/composables/useCardNavList'
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
    // Wish-list wanted counts keyed by card id (issue #364 follow-up): a card present here
    // with a positive count shows a Heart "wanted" chip on its quick-add control, flagging
    // cards on your wish list. Passed only on collection-targeting grids (list==='collection');
    // on a wishlist grid the count chips already show wants, so this is omitted (undefined).
    wishlist?: OwnedCountsMap
  }>(),
  { list: 'collection', ownedMarks: undefined, wishlist: undefined },
)

const cardSize = useCardSizeStore()
const gridClass = computed(() => CARD_SIZE_GRID_CLASS[cardSize.size])

function isOwnedMark(entry: CollectionEntry): boolean {
  const owned = props.ownedMarks?.[entry.card.id]
  return !!owned && owned.quantity + owned.foil_quantity > 0
}

// The card's total wanted count from the wish-list map (regular + foil); 0 when absent.
function wishlistTotal(entry: CollectionEntry): number {
  const w = props.wishlist?.[entry.card.id]
  return w ? w.quantity + w.foil_quantity : 0
}

// Publish these entries' cards (in display order) so the card-detail modal can step prev/next
// through them with the arrow keys / its buttons (issue #275).
useCardNavList(
  () => props.game,
  () => props.entries.map((entry) => entry.card.id),
)
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
          :wishlist-quantity="wishlistTotal(entry)"
          :list="list"
        />
      </template>
    </CardTile>
  </div>
</template>
