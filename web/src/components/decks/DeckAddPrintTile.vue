<script setup lang="ts">
import { computed, toRef } from 'vue'
import { Plus } from '@lucide/vue'
import CardImage from '@/components/cards/CardImage.vue'
import { displayUsdPrice } from '@/lib/cardPrice'
import type { Card } from '@/lib/api'

// One printing in the deck builder's "add cards" grid (issue #391): full card art plus
// set/number/rarity/price, the whole tile a button that adds one copy. Art makes visually
// similar printings (alt-art, borderless, foils) easy to tell apart — the old single-line
// row didn't. Deliberately purely presentational and ADDITIVE (a click emits `add` = +1),
// unlike the collection's QuickAddPrintTile absolute steppers: the parent owns the deck
// mutation and the optimistic count, so this component never writes.
const props = defineProps<{
  game: string
  card: Card
  /** Copies of this printing already in the target section, for the progress badge. */
  count?: number
}>()
defineEmits<{ add: [] }>()

const card = toRef(props, 'card')
const price = computed(() => displayUsdPrice(props.card.prices))
</script>

<template>
  <button
    type="button"
    class="group focus-visible:ring-ring relative flex flex-col gap-1.5 rounded-lg border p-1.5 text-left transition outline-none hover:border-primary/50 focus-visible:ring-2"
    :aria-label="`Add ${card.name} (${card.set_name})`"
    @click="$emit('add')"
  >
    <div class="relative">
      <CardImage
        :game="game"
        :id="card.id"
        :name="card.name"
        :has-image="card.has_image"
        size="normal"
        class="w-full rounded-md"
      />
      <!-- Copies already in the target section, so building a playset shows progress. -->
      <span
        v-if="count"
        class="bg-background/90 text-foreground absolute top-1 right-1 z-10 rounded-md border px-1.5 py-0.5 text-xs font-medium shadow tabular-nums select-none"
        >×{{ count }}</span
      >
      <!-- Add affordance, revealed on hover/focus (the whole tile is the button). -->
      <span
        class="bg-primary text-primary-foreground absolute right-1 bottom-1 z-10 flex size-6 items-center justify-center rounded-full opacity-0 shadow transition group-hover:opacity-100 group-focus-visible:opacity-100"
        aria-hidden="true"
      >
        <Plus class="size-4" />
      </span>
    </div>

    <div class="min-w-0 px-0.5">
      <p class="truncate text-xs font-medium" :title="card.set_name">{{ card.set_name }}</p>
      <p class="text-muted-foreground flex flex-wrap items-center gap-x-1 text-xs">
        <span>{{ card.set_code.toUpperCase() }} · #{{ card.collector_number }}</span>
        <span v-if="card.rarity" class="capitalize">· {{ card.rarity }}</span>
        <span v-if="price" class="tabular-nums">· ${{ price.amount }}</span>
      </p>
    </div>
  </button>
</template>
