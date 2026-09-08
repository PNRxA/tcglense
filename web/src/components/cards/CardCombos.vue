<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
import { useQuery } from '@tanstack/vue-query'
import { ExternalLink } from '@lucide/vue'
import { getCardCombos } from '@/lib/api'
import { STRUCTURAL_CATALOG_STALE_MS } from '@/lib/queryClient'
import CollapsibleSection from '@/components/shared/CollapsibleSection.vue'
import { useDetailModalLink } from '@/composables/useDetailModalLink'

// **Combos with this card** (issue #683): the Commander Spellbook combos the card is a
// piece of, most-played first. Keyed on its gameplay identity (oracle id) server-side, so
// every printing of the card shows the same list — the same stance the rulings block takes.
//
// Nothing here is inferred: what several cards do together is a curated fact, so the block
// names its source, links every combo back to it, and never states a total of its own —
// `total` is the response's, and a card that is a piece of thousands says so rather than
// implying the fifty listed are all of them.
//
// Renders nothing when the card is in no combo, which is also what an instance with no
// combo data synced looks like from here: an empty list, and so no claim either way.
const props = defineProps<{
  game: string
  id: string
}>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')

// Public endpoint, so a plain useQuery (no auth wrapper). Refs go straight into the
// queryKey so a card-to-card navigation refetches for the new card. Combos move only with
// the dataset's own sync, so they're structural-cadence data.
const query = useQuery({
  queryKey: ['card-combos', game, id],
  queryFn: () => getCardCombos(game.value, id.value),
  staleTime: STRUCTURAL_CATALOG_STALE_MS,
})

const combos = computed(() => query.data.value?.combos ?? [])
const total = computed(() => query.data.value?.total ?? 0)
const source = computed(() => query.data.value?.source ?? 'Commander Spellbook')
const sourceUrl = computed(() => query.data.value?.source_url ?? 'https://commanderspellbook.com')

// Collapsed by default, unlike the rulings above: a staple is a piece of dozens of combos,
// which is a longer list than the card's own rules text and not what most visits came for.
// Section-local state, re-collapsed when the card changes (issue #332's idiom).
const expanded = ref(false)
watch(id, () => {
  expanded.value = false
})

const { hrefFor, onActivate, warm } = useDetailModalLink()
</script>

<template>
  <!-- Hidden entirely until the card is a piece of at least one combo, so the common case
    adds nothing to the page — and so an instance with no combo data shows no empty block. -->
  <section v-if="total > 0">
    <h2 class="mb-3 text-base font-semibold tracking-tight">Combos</h2>
    <CollapsibleSection
      v-model:expanded="expanded"
      title="Combos with this card"
      :count="total"
      :blurb="`Card interactions this card is a piece of, from ${source}.`"
    >
      <ul class="space-y-3">
        <li v-for="combo in combos" :key="combo.id" class="border-b pb-3 last:border-b-0 last:pb-0">
          <!-- The pieces, in the combo's own order. The card being viewed is shown like the
            rest but not linked — a link back to this very page would go nowhere. -->
          <ul class="flex flex-wrap items-center gap-1.5">
            <li v-for="piece in combo.pieces" :key="piece.oracle_id">
              <a
                v-if="piece.card_id && piece.card_id !== id"
                :href="hrefFor('card', game, piece.card_id)"
                class="inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-xs hover:underline"
                @click="onActivate($event, 'card', game, piece.card_id)"
                @pointerenter="warm('card')"
                @focusin="warm('card')"
              >
                {{ piece.name }}
                <span v-if="piece.quantity > 1" class="text-muted-foreground tabular-nums"
                  >×{{ piece.quantity }}</span
                >
              </a>
              <span
                v-else
                class="bg-muted/60 inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-xs"
              >
                {{ piece.name }}
                <span v-if="piece.quantity > 1" class="text-muted-foreground tabular-nums"
                  >×{{ piece.quantity }}</span
                >
              </span>
            </li>
          </ul>

          <ul v-if="combo.produces.length" class="mt-2 flex flex-wrap gap-1.5">
            <li
              v-for="result in combo.produces"
              :key="result"
              class="bg-success/15 text-success rounded-md px-1.5 py-0.5 text-xs"
            >
              {{ result }}
            </li>
          </ul>

          <a
            :href="combo.url"
            target="_blank"
            rel="noopener noreferrer"
            class="text-muted-foreground hover:text-foreground mt-2 inline-flex items-center gap-1 text-xs"
          >
            View on {{ source }}
            <ExternalLink class="size-3" aria-hidden="true" />
          </a>
        </li>
      </ul>

      <!-- The list is capped; the count above it is not. Say which is which rather than
        letting fifty rows read as the whole answer. -->
      <p v-if="total > combos.length" class="text-muted-foreground mt-3 text-xs">
        …and {{ total - combos.length }} more on {{ source }}.
      </p>

      <p class="text-muted-foreground mt-3 text-xs">
        Combo data from
        <a
          :href="sourceUrl"
          target="_blank"
          rel="noopener noreferrer"
          class="underline underline-offset-2"
          >{{ source }}</a
        >.
      </p>
    </CollapsibleSection>
  </section>
</template>
