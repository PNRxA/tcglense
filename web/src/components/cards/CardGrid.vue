<script setup lang="ts">
import { computed } from 'vue'
import type { Card, OwnedCountsMap } from '@/lib/api'
import CardTile from '@/components/cards/CardTile.vue'
import OwnedCountControl from '@/components/cards/OwnedCountControl.vue'
import OwnedMarkBadge from '@/components/cards/OwnedMarkBadge.vue'
import ReadonlyOwnedBadge from '@/components/cards/ReadonlyOwnedBadge.vue'
import type { CardListTarget, OwnedCountSeed } from '@/composables/useOwnedCountEditor'
import { useCardNavList } from '@/composables/useCardNavList'
import { CARD_SIZE_GRID_CLASS } from '@/lib/cardSize'
import { useCardSizeStore } from '@/stores/cardSize'
import { useAuthStore } from '@/stores/auth'

const props = withDefaults(
  defineProps<{
    game: string
    cards: Card[]
    // Owned counts keyed by card id (from `useOwnedCounts` — or `useWishlistCounts` for a
    // wish-list grid): a card present here shows its counts on the quick-add control's
    // trigger; an absent card shows an "add" affordance instead. Omitted for
    // non-collection grids; the controls only render while signed in, so a signed-out
    // visitor's grid carries neither badges nor add affordances.
    ownership?: OwnedCountsMap
    // Collection show-ghosts mode (issue #112): dim every card the viewer doesn't own so
    // owned cards pop and a set's gaps read at a glance. Ownership is decided from the same
    // `ownership` map, so a card not present there (or owned in zero copies) renders as a
    // ghost. Off for the plain catalog browse grids.
    ghostUnowned?: boolean
    // What this grid's own `ownership` map / counts represent (issue #167): the collection
    // (default — catalog & collection pages) or the wish list (the wishlist page, where
    // `ownership` means wish-list membership). The quick-add control is ALWAYS collection-
    // primary; on the wishlist surface its collection chips read `collectionCounts` and its
    // wanted Heart reads this grid's own `ownership`.
    list?: CardListTarget
    // Collection-owned counts keyed by card id: used ONLY on the wishlist surface
    // (list==='wishlist') to feed the quick-add control's primary (collection) count chips —
    // there the grid's own `ownership` map means wish-list membership, not collection
    // ownership, so the collection totals come from here. Absent on collection grids (the
    // grid's own `ownership` already holds collection counts).
    collectionCounts?: OwnedCountsMap
    // Collection-ownership counts keyed by card id (issue #213): a card present here with a
    // positive count gets an "Owned" marker overlaid, flagging cards you already own in your
    // *collection*. Used by the wish-list browse grids under the ghost button's "Show owned"
    // setting — distinct from `ownership`, which on those grids means wish-list membership.
    // Absent (undefined) everywhere else, so no marker renders.
    ownedMarks?: OwnedCountsMap
    // Wish-list wanted counts keyed by card id (issue #364 follow-up): a card present here
    // with a positive count shows a Heart "wanted" chip on its quick-add control, flagging
    // cards on your wish list. Passed only on collection-targeting grids (list==='collection');
    // on a wishlist grid the count chips already show wants, so this is omitted (undefined).
    wishlist?: OwnedCountsMap
    // Read-only grid (the public collection browse, issues #361/#362): render a static owned
    // badge from the `ownership` map — which here holds the OWNER's counts — instead of the
    // quick-add editor, ignoring auth. A signed-in viewer of someone else's public page must
    // never get an editor that would write the owner's counts into their own collection.
    readonly?: boolean
  }>(),
  {
    ownership: undefined,
    ghostUnowned: false,
    list: 'collection',
    collectionCounts: undefined,
    ownedMarks: undefined,
    wishlist: undefined,
    readonly: false,
  },
)

// A card is owned when the ownership map holds a positive count for it; in show-ghosts
// mode every other card is dimmed.
function isGhost(card: Card): boolean {
  if (!props.ghostUnowned) return false
  const owned = props.ownership?.[card.id]
  return !owned || owned.quantity + owned.foil_quantity <= 0
}

// Whether to flag this card as owned-in-collection (issue #213): present in `ownedMarks`
// with a positive count. Off unless the wish-list "Show owned" setting passed the marks in.
function isOwnedMark(card: Card): boolean {
  const owned = props.ownedMarks?.[card.id]
  return !!owned && owned.quantity + owned.foil_quantity > 0
}

// The counts feeding the quick-add control's PRIMARY (collection) count chips. On a
// collection grid the grid's own `ownership` map already holds collection counts; on the
// wishlist surface those counts are WANTS, so the collection totals come from the
// `collectionCounts` overlay instead.
function primaryCounts(card: Card): { quantity: number; foil_quantity: number } {
  const source = props.list === 'wishlist' ? props.collectionCounts : props.ownership
  return source?.[card.id] ?? { quantity: 0, foil_quantity: 0 }
}

// The card's resting wish-list want (regular + foil split) feeding the control's appended
// Heart chip AND its wish-list row display seed. On a collection grid that's the `wishlist`
// overlay; on the wishlist surface it's the grid's own `ownership` map (which there means
// wish-list membership). Undefined when absent — no heart, and the row seeds lazily on open.
function wishlistSeed(card: Card): OwnedCountSeed | undefined {
  const source = props.list === 'wishlist' ? props.ownership : props.wishlist
  return source?.[card.id]
}

// The grid's column count follows the user's persisted size preference (set via
// CardSizeMenu); fewer columns render larger cards. The store reads localStorage
// synchronously, so the right density is applied on the first paint.
const cardSize = useCardSizeStore()
const gridClass = computed(() => CARD_SIZE_GRID_CLASS[cardSize.size])

// Quick-add controls (issue #95) are a signed-in feature.
const auth = useAuthStore()

// Publish this grid's cards (in display order) so the card-detail modal can step prev/next
// through them with the arrow keys / its buttons (issue #275).
useCardNavList(
  () => props.game,
  () => props.cards.map((card) => card.id),
)
</script>

<template>
  <div class="grid gap-x-4 gap-y-6" :class="gridClass">
    <CardTile v-for="card in cards" :key="card.id" :game="game" :card="card" :ghost="isGhost(card)">
      <template #badge>
        <OwnedMarkBadge v-if="isOwnedMark(card)" />
        <!-- Read-only public grid: a static owned badge (the owner's counts), never an editor. -->
        <ReadonlyOwnedBadge
          v-if="readonly"
          :quantity="primaryCounts(card).quantity"
          :foil-quantity="primaryCounts(card).foil_quantity"
        />
        <OwnedCountControl
          v-else-if="auth.isAuthenticated"
          :game="game"
          :card-id="card.id"
          :name="card.name"
          :quantity="primaryCounts(card).quantity"
          :foil-quantity="primaryCounts(card).foil_quantity"
          :wishlist-seed="wishlistSeed(card)"
        />
      </template>
    </CardTile>
  </div>
</template>
