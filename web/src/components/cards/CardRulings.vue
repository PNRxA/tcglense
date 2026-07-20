<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
import { useQuery } from '@tanstack/vue-query'
import { getCardRulings } from '@/lib/api'
import { STRUCTURAL_CATALOG_STALE_MS } from '@/lib/queryClient'
import CollapsibleSection from '@/components/shared/CollapsibleSection.vue'
import ManaSymbols from '@/components/cards/ManaSymbols.vue'

// A card's "Notes and Rules Information" (issue #522): the official rulings Scryfall
// records for the card, keyed on its gameplay identity (oracle id) so every printing
// shows the same list. Renders nothing when the card has no rulings.
const props = defineProps<{ game: string; id: string }>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')

// Public rulings endpoint, so a plain useQuery (no auth wrapper). Refs go straight into
// the queryKey so a card-to-card navigation refetches for the new card. Rulings carry no
// prices and move only on the daily sync, so they're structural-cadence data.
const query = useQuery({
  queryKey: ['card-rulings', game, id],
  queryFn: () => getCardRulings(game.value, id.value),
  staleTime: STRUCTURAL_CATALOG_STALE_MS,
})

const rulings = computed(() => query.data.value?.data ?? [])

// A friendlier label for the ruling's source than the raw slug.
const SOURCE_LABELS: Record<string, string> = {
  wotc: 'Wizards of the Coast',
  scryfall: 'Scryfall',
}
const sourceLabel = (source: string) => SOURCE_LABELS[source] ?? source

// Collapsed by default, matching the other detail sections (issue #332). Section-local:
// the component is reused across card-to-card navigation, so re-collapse when the id
// changes.
const expanded = ref(false)
watch(id, () => {
  expanded.value = false
})
</script>

<template>
  <!-- Hidden entirely until there's at least one ruling, so the common case (a card with
    no rulings) adds nothing to the page. -->
  <CollapsibleSection
    v-if="rulings.length"
    v-model:expanded="expanded"
    title="Notes and Rules Information"
    :count="rulings.length"
    blurb="Official rulings and clarifications for this card, from Scryfall."
    heading="h2"
  >
    <ul class="space-y-3">
      <li
        v-for="(ruling, index) in rulings"
        :key="index"
        class="border-b pb-3 last:border-b-0 last:pb-0"
      >
        <p class="text-sm leading-relaxed whitespace-pre-line">
          <ManaSymbols :text="ruling.comment" />
        </p>
        <p class="text-muted-foreground mt-1 text-xs">
          {{ sourceLabel(ruling.source) }}
          <template v-if="ruling.published_at"> · {{ ruling.published_at }}</template>
        </p>
      </li>
    </ul>
  </CollapsibleSection>
</template>
