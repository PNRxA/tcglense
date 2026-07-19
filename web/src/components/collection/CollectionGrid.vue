<script setup lang="ts">
import { computed } from 'vue'
import type { CollectionEntry, OwnedCountsMap } from '@/lib/api'
import CardTile from '@/components/cards/CardTile.vue'
import OwnedCountControl from '@/components/cards/OwnedCountControl.vue'
import OwnedMarkBadge from '@/components/cards/OwnedMarkBadge.vue'
import ReadonlyOwnedBadge from '@/components/cards/ReadonlyOwnedBadge.vue'
import type { CardListTarget, OwnedCountSeed } from '@/composables/useOwnedCountEditor'
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
    // with a positive count shows a Heart "wanted" chip on its quick-add control. It is the
    // Heart's source on BOTH surfaces — flagging wish-listed cards on a collection grid, and
    // on the wishlist surface standing in for the entry's own want (fetched into the
    // order-independent `['wishlist-counts', …]` overlay) so a quick-add edit repaints the
    // heart in place instead of resorting the recency-sorted tiles. Absent (undefined) when no
    // overlay is threaded, so no heart renders.
    wishlist?: OwnedCountsMap
    // Whether the `wishlist` overlay has SETTLED for the currently rendered cards (issue #364
    // follow-up). Only meaningful on the wishlist surface, where a card absent from the overlay
    // is ambiguous: it may just not be covered yet (cold load / pagination), or it may have been
    // genuinely removed (want → 0). While the overlay is still loading (`false`) an absent card
    // falls back to the entry's own want so the heart shows at once; once it has settled (`true`)
    // an absent card is trusted as unwanted, so a quick-remove clears the heart in place instead
    // of the stale frozen-list entry pinning the old count. Defaults false; the collection
    // surface never sets it (its hearts come from the overlay alone, with no entry fallback).
    wishlistReady?: boolean
    // Read-only grid (the public collection browse, issues #361/#362): render a static owned
    // badge (the owner's counts) instead of the quick-add editor. This grid otherwise renders
    // OwnedCountControl unconditionally, so without this flag a signed-in viewer of someone
    // else's public collection would get an editor seeded with the owner's counts that writes
    // into their OWN collection.
    readonly?: boolean
  }>(),
  {
    list: 'collection',
    collectionCounts: undefined,
    ownedMarks: undefined,
    wishlist: undefined,
    wishlistReady: false,
    readonly: false,
  },
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

// The card's resting wish-list want (regular + foil split) feeding the control's appended Heart
// chip AND its wish-list row display seed. The order-independent `wishlist` overlay
// (`['wishlist-counts', …]`) is the authoritative source on BOTH surfaces: on a collection grid
// it flags a wish-listed card; on the wishlist surface it stands in for the entry's own want
// (not `entry.quantity`) so a quick-add edit repaints the heart in place rather than resorting
// the recency-sorted tiles under the open popover (the list refetch is deferred there) — issue
// #364 follow-up.
//
// Per-card fallback (issue #364 follow-up): the overlay is a sequential second fetch keyed off
// the just-painted list, so on a cold load and every pagination it lands a round-trip after the
// tiles. On the wishlist surface, while the overlay is still loading (`!wishlistReady`) a card it
// hasn't covered falls back to the entry's own wanted counts — immediate from the just-painted
// list — so the heart shows at once instead of blanking (and a wanted-but-unowned tile doesn't
// flash a bare "+"). Once the overlay has SETTLED (`wishlistReady`), an absent card is trusted as
// genuinely unwanted (e.g. a quick-remove took the want to 0): returning undefined clears the
// heart in place rather than the stale frozen-list entry pinning the old count. On a collection
// grid an absent overlay means the card simply isn't wish-listed, so it stays undefined.
function wishlistSeed(entry: CollectionEntry): OwnedCountSeed | undefined {
  const overlay = props.wishlist?.[entry.card.id]
  if (overlay) return overlay
  if (props.list === 'wishlist' && !props.wishlistReady) {
    return { quantity: entry.quantity, foil_quantity: entry.foil_quantity }
  }
  return undefined
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
        <!-- Read-only public grid: a static owned badge (the owner's counts), never an editor. -->
        <ReadonlyOwnedBadge
          v-if="readonly"
          :quantity="primaryCounts(entry).quantity"
          :foil-quantity="primaryCounts(entry).foil_quantity"
        />
        <OwnedCountControl
          v-else
          :game="game"
          :card-id="entry.card.id"
          :name="entry.card.name"
          :quantity="primaryCounts(entry).quantity"
          :foil-quantity="primaryCounts(entry).foil_quantity"
          :wishlist-seed="wishlistSeed(entry)"
        />
      </template>
    </CardTile>
  </div>
</template>
