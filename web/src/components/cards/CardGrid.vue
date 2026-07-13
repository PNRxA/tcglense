<script setup lang="ts">
import { computed } from 'vue'
import type { Card, OwnedCountsMap } from '@/lib/api'
import CardTile from '@/components/cards/CardTile.vue'
import OwnedCountControl from '@/components/cards/OwnedCountControl.vue'
import OwnedMarkBadge from '@/components/cards/OwnedMarkBadge.vue'
import type { CardListTarget } from '@/composables/useOwnedCountEditor'
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
    // Which list the quick-add controls read/write (issue #167): the collection (default)
    // or the wish list — where the `ownership` map means wish-list membership instead.
    list?: CardListTarget
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
  }>(),
  {
    ownership: undefined,
    ghostUnowned: false,
    list: 'collection',
    ownedMarks: undefined,
    wishlist: undefined,
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

// The card's total wanted count from the wish-list map (regular + foil); 0 when absent.
function wishlistTotal(card: Card): number {
  const w = props.wishlist?.[card.id]
  return w ? w.quantity + w.foil_quantity : 0
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
        <OwnedCountControl
          v-if="auth.isAuthenticated"
          :game="game"
          :card-id="card.id"
          :name="card.name"
          :quantity="ownership?.[card.id]?.quantity ?? 0"
          :foil-quantity="ownership?.[card.id]?.foil_quantity ?? 0"
          :wishlist-quantity="wishlistTotal(card)"
          :list="list"
        />
      </template>
    </CardTile>
  </div>
</template>
