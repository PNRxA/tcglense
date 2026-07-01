<script setup lang="ts">
import { computed } from 'vue'
import type { Card, OwnedCountsMap } from '@/lib/api'
import CardTile from '@/components/cards/CardTile.vue'
import OwnedCountControl from '@/components/cards/OwnedCountControl.vue'
import { CARD_SIZE_GRID_CLASS } from '@/lib/cardSize'
import { useCardSizeStore } from '@/stores/cardSize'
import { useAuthStore } from '@/stores/auth'

defineProps<{
  game: string
  cards: Card[]
  // Owned counts keyed by card id (from `useOwnedCounts`): a card present here shows its
  // owned counts on the quick-add control's trigger; an absent card shows an "add"
  // affordance instead. Omitted for non-collection grids; the controls only render while
  // signed in, so a signed-out visitor's grid carries neither badges nor add affordances.
  ownership?: OwnedCountsMap
}>()

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
    <CardTile v-for="card in cards" :key="card.id" :game="game" :card="card">
      <template #badge>
        <OwnedCountControl
          v-if="auth.isAuthenticated"
          :game="game"
          :card-id="card.id"
          :name="card.name"
          :quantity="ownership?.[card.id]?.quantity ?? 0"
          :foil-quantity="ownership?.[card.id]?.foil_quantity ?? 0"
        />
      </template>
    </CardTile>
  </div>
</template>
