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
// without opening the card page. This view only renders for signed-in users. `list` says
// what the entries themselves represent (issue #167): the collection (default) or the wish
// list (the wishlist page). The quick-add control is ALWAYS collection-primary; on the
// wishlist surface its collection chips read the `collectionCounts` overlay instead of the
// entries' own (wanted) counts, and its wanted Heart reads the entries' counts.
const props = withDefaults(
  defineProps<{
    game: string
    entries: CollectionEntry[]
    list?: CardListTarget
    // Collection-owned counts keyed by card id: used ONLY on the wishlist surface
    // (list==='wishlist') to feed the quick-add control's primary (collection) count chips —
    // there the entries' own counts are wish-list wants, so the collection totals come from
    // here. Absent on a plain collection grid (the entries already hold collection counts).
    collectionCounts?: OwnedCountsMap
    // Collection-ownership counts keyed by card id (issue #213): flags wishlisted cards you
    // already own in your collection with an "Owned" marker, under the wish list's "Show
    // owned" setting. Absent (undefined) on a plain collection grid, so no marker renders.
    ownedMarks?: OwnedCountsMap
    // Wish-list wanted counts keyed by card id (issue #364 follow-up): a card present here
    // with a positive count shows a Heart "wanted" chip on its quick-add control, flagging
    // cards on your wish list. Passed only on collection-targeting grids (list==='collection');
    // on a wishlist grid the entries already hold wants, so this is omitted (undefined).
    wishlist?: OwnedCountsMap
  }>(),
  { list: 'collection', collectionCounts: undefined, ownedMarks: undefined, wishlist: undefined },
)

const cardSize = useCardSizeStore()
const gridClass = computed(() => CARD_SIZE_GRID_CLASS[cardSize.size])

function isOwnedMark(entry: CollectionEntry): boolean {
  const owned = props.ownedMarks?.[entry.card.id]
  return !!owned && owned.quantity + owned.foil_quantity > 0
}

// The counts feeding the quick-add control's PRIMARY (collection) count chips. On a
// collection grid that's the entry's own counts; on the wishlist surface the entry's counts
// are WANTS, so the collection totals come from the `collectionCounts` overlay instead.
function primaryCounts(entry: CollectionEntry): { quantity: number; foil_quantity: number } {
  if (props.list === 'wishlist') {
    return props.collectionCounts?.[entry.card.id] ?? { quantity: 0, foil_quantity: 0 }
  }
  return { quantity: entry.quantity, foil_quantity: entry.foil_quantity }
}

// The card's total wanted count feeding the control's appended Heart chip (regular + foil).
// On a collection grid that's the `wishlist` overlay; on the wishlist surface it's the
// entry's own counts (which there are wish-list wants). 0 when absent.
function wantedTotal(entry: CollectionEntry): number {
  if (props.list === 'wishlist') return entry.quantity + entry.foil_quantity
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
          :quantity="primaryCounts(entry).quantity"
          :foil-quantity="primaryCounts(entry).foil_quantity"
          :wishlist-quantity="wantedTotal(entry)"
        />
      </template>
    </CardTile>
  </div>
</template>
