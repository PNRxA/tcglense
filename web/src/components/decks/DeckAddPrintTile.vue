<script setup lang="ts">
import { Plus } from '@lucide/vue'
import PrintingTile from '@/components/printings/PrintingTile.vue'
import type { Card } from '@/lib/api'

// One printing in the deck builder's "add cards" grid (issue #391): full card art plus
// set/number/rarity/price, the whole tile a button that adds one copy. Art makes visually
// similar printings (alt-art, borderless, foils) easy to tell apart — the old single-line
// row didn't. Deliberately purely presentational and ADDITIVE (a click emits `add` = +1),
// unlike the collection's QuickAddPrintTile absolute steppers: the parent owns the deck
// mutation and the optimistic count, so this component never writes.
defineProps<{
  game: string
  card: Card
  /** Copies of this printing already in the target section, for the progress badge. */
  count?: number
  /** An add for this printing is in flight — the add affordance shows a spinner. */
  loading?: boolean
  /** Automatic filing found no safe target; the parent asks for an explicit section. */
  disabled?: boolean
}>()
defineEmits<{ add: [] }>()
</script>

<template>
  <PrintingTile
    :game="game"
    :card="card"
    selectable
    :loading="loading"
    :disabled="disabled"
    :aria-label="
      disabled
        ? `Choose a section before adding ${card.name} (${card.set_name})`
        : `Add ${card.name} (${card.set_name})`
    "
    @select="$emit('add')"
  >
    <template #overlay>
      <!-- Copies already in the target section, so building a playset shows progress. -->
      <span
        v-if="count"
        class="bg-background/90 text-foreground absolute top-1 right-1 z-10 rounded-md border px-1.5 py-0.5 text-xs font-medium shadow tabular-nums select-none"
        >×{{ count }}</span
      >
      <!-- Add affordance, revealed on hover/focus (the whole tile is the button); while an
        add is in flight the shared tile's common loading marker takes this position. -->
      <span
        v-if="!disabled && !loading"
        class="bg-primary text-primary-foreground absolute right-1 bottom-1 z-10 flex size-6 items-center justify-center rounded-full opacity-0 shadow transition group-hover:opacity-100 group-focus-visible:opacity-100"
        aria-hidden="true"
      >
        <Plus class="size-4" />
      </span>
    </template>
  </PrintingTile>
</template>
