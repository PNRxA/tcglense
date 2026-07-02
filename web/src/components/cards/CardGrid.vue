<script setup lang="ts">
import { computed } from 'vue'
import type { Card, OwnedCountsMap } from '@/lib/api'
import CardTile from '@/components/cards/CardTile.vue'
import OwnedCountControl from '@/components/cards/OwnedCountControl.vue'
import type { CardListTarget } from '@/composables/useOwnedCountEditor'
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
  }>(),
  { ownership: undefined, ghostUnowned: false, list: 'collection' },
)

// A card is owned when the ownership map holds a positive count for it; in show-ghosts
// mode every other card is dimmed.
function isGhost(card: Card): boolean {
  if (!props.ghostUnowned) return false
  const owned = props.ownership?.[card.id]
  return !owned || owned.quantity + owned.foil_quantity <= 0
}

// The grid's column count follows the user's persisted size preference (set via
// CardSizeMenu); fewer columns render larger cards. The store reads localStorage
// synchronously, so the right density is applied on the first paint.
const cardSize = useCardSizeStore()
const gridClass = computed(() => CARD_SIZE_GRID_CLASS[cardSize.size])

// Quick-add controls (issue #95) are a signed-in feature.
const auth = useAuthStore()
</script>

<template>
  <div class="grid gap-x-4 gap-y-6" :class="gridClass">
    <CardTile v-for="card in cards" :key="card.id" :game="game" :card="card" :ghost="isGhost(card)">
      <template #badge>
        <OwnedCountControl
          v-if="auth.isAuthenticated"
          :game="game"
          :card-id="card.id"
          :name="card.name"
          :quantity="ownership?.[card.id]?.quantity ?? 0"
          :foil-quantity="ownership?.[card.id]?.foil_quantity ?? 0"
          :list="list"
        />
      </template>
    </CardTile>
  </div>
</template>
