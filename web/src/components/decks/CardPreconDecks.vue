<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
import type { CardPreconRef } from '@/lib/api'
import { useCardPreconsQuery } from '@/composables/usePrecons'
import PreconTile from '@/components/decks/PreconTile.vue'
import CollapsibleSection from '@/components/shared/CollapsibleSection.vue'

// The card-detail "Preconstructed decks" section: the published decklists that include
// this card — any printing of it, on any board — newest first. Renders nothing when no
// precon has it, like the sealed-products section beside it. Shown in both the full card
// page and the browse-grid modal (both mount CardDetailContent).
const props = defineProps<{ game: string; id: string }>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')

const query = useCardPreconsQuery(game, id)
const entries = computed<CardPreconRef[]>(() => query.data.value?.data ?? [])
const total = computed(() => query.data.value?.total ?? 0)

/** The chips that say how the card sits in a deck; the tile itself says what the deck is.
 * Quantity only when it adds something over "it's in there" (a playset, a pile of basics). */
function chips(entry: CardPreconRef): string[] {
  const out: string[] = []
  if (entry.commander) out.push('Commander')
  if (entry.foil) out.push('Foil')
  if (entry.quantity > 1) out.push(`×${entry.quantity}`)
  return out
}

// Collapsed by default with the count on the heading (a format staple is in hundreds of
// decks); card-to-card navigation re-collapses it.
const expanded = ref(false)
watch(id, () => {
  expanded.value = false
})
</script>

<template>
  <CollapsibleSection
    v-if="entries.length"
    v-model:expanded="expanded"
    title="Preconstructed decks"
    :count="total"
    blurb="Published decklists that include this card (any printing)."
    heading="h2"
  >
    <div class="grid gap-3 sm:grid-cols-2">
      <div v-for="entry in entries" :key="entry.precon.slug" class="relative">
        <PreconTile :precon="entry.precon" :game="game" />
        <div
          v-if="chips(entry).length"
          class="pointer-events-none absolute top-2 right-2 flex flex-wrap justify-end gap-1"
        >
          <span
            v-for="chip in chips(entry)"
            :key="chip"
            class="rounded-md px-1.5 py-0.5 text-xs font-medium select-none"
            :class="chip === 'Foil' ? 'bg-foil/15 text-foil' : 'bg-muted text-muted-foreground'"
          >
            {{ chip }}
          </span>
        </div>
      </div>
    </div>
    <!-- One browse-sized page is plenty for a panel; say so when it isn't everything. -->
    <p v-if="total > entries.length" class="text-muted-foreground mt-3 text-xs">
      Showing the newest {{ entries.length.toLocaleString() }} of {{ total.toLocaleString() }}
      decks.
    </p>
  </CollapsibleSection>
</template>
