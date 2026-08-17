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
const props = defineProps<{
  game: string
  id: string
  /** The card's name, forwarded to the keyword matcher. It matters more here than on
   * oracle text: a ruling routinely opens with the card's own title ("Gift of
   * Immortality returns to the battlefield…"), and without this the matcher would put
   * the keyword action Gift on that first word. */
  cardName?: string
}>()
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

// Open by default — unlike the other collapsibles (#332), which hide long *lists* of
// related rows (printings, sealed buckets). Rulings are the card's own rules text: a
// card that has them usually has one or two, they're what a reader came to the page to
// check, and the section is already hidden entirely when there are none — so there's
// nothing to save by collapsing it. Section-local state: the component is reused across
// card-to-card navigation, so re-open when the id changes (a reader who collapsed one
// card's rulings hasn't asked for the next card's to be hidden).
const expanded = ref(true)
watch(id, () => {
  expanded.value = true
})
</script>

<template>
  <!-- Hidden entirely until there's at least one ruling, so the common case (a card with
    no rulings) adds nothing to the page. Headed like the "Sealed products" and "Decks"
    groups above it, so the block doesn't read as part of whichever group precedes it. -->
  <section v-if="rulings.length">
    <h2 class="mb-3 text-base font-semibold tracking-tight">Rulings</h2>
    <CollapsibleSection
      v-model:expanded="expanded"
      title="Notes and Rules Information"
      :count="rulings.length"
      blurb="Official rulings and clarifications for this card, from Scryfall."
    >
      <ul class="space-y-3">
        <li
          v-for="(ruling, index) in rulings"
          :key="index"
          class="border-b pb-3 last:border-b-0 last:pb-0"
        >
          <p class="text-sm leading-relaxed whitespace-pre-line">
            <ManaSymbols :text="ruling.comment" keywords :game="game" :card-name="cardName" />
          </p>
          <p class="text-muted-foreground mt-1 text-xs">
            {{ sourceLabel(ruling.source) }}
            <template v-if="ruling.published_at"> · {{ ruling.published_at }}</template>
          </p>
        </li>
      </ul>
    </CollapsibleSection>
  </section>
</template>
