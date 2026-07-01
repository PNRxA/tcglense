<script setup lang="ts">
import { computed, toRef } from 'vue'
import { useQuery } from '@tanstack/vue-query'
import { getCardPrints } from '@/lib/api'
import CardGrid from '@/components/cards/CardGrid.vue'
import { useOwnedCounts } from '@/composables/useCollection'

const props = defineProps<{ game: string; id: string }>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')

// Public prints endpoint, so a plain useQuery (no auth wrapper). Refs go straight
// into the queryKey so a card-to-card navigation (e.g. clicking another printing)
// refetches for the new card.
const query = useQuery({
  queryKey: ['card-prints', game, id],
  queryFn: () => getCardPrints(game.value, id.value),
})

const prints = computed(() => query.data.value?.data ?? [])
// Owned-count badges for signed-in users, overlaid on the printings grid.
const { ownership } = useOwnedCounts(game, prints)
</script>

<template>
  <!-- Hidden entirely until there's at least one other printing to show, so a
    one-printing card (the common case) adds nothing to the page. -->
  <section v-if="prints.length" class="mt-10">
    <h2 class="mb-3 text-sm font-semibold">Other printings ({{ prints.length }})</h2>
    <CardGrid :game="game" :cards="prints" :ownership="ownership" />
  </section>
</template>
