<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink } from 'vue-router'
import { Layers } from '@lucide/vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import CardTile from '@/components/cards/CardTile.vue'
import DeckStats from '@/components/decks/DeckStats.vue'
import { usePublicDeckQuery } from '@/composables/useDecks'
import { useCurrency } from '@/composables/useCurrency'
import type { DeckCardEntry } from '@/lib/api'
import { usePageMeta } from '@/lib/seo'

// The read-only, shareable public deck (issue #363): `/u/:handle/decks/:id`. Anyone can
// view; no edit controls. Indexable so shared links preview and rank.
const props = defineProps<{ handle: string; id: string }>()
const money = useCurrency()
const handle = computed(() => props.handle)
const deckId = computed(() => Number(props.id))
const deckQuery = usePublicDeckQuery(handle, deckId)
const deck = computed(() => deckQuery.data.value)

// The public game slug is carried in the URL as a handle only; the deck's game is on each
// card. Author display name strips the discriminator (`alice-0001` -> `alice`).
const author = computed(() => props.handle.replace(/-\d{1,4}$/, ''))

usePageMeta({
  title: computed(() => (deck.value ? `${deck.value.name} by ${author.value}` : 'Deck')),
  description: computed(() =>
    deck.value ? `${deck.value.name} — a deck shared by ${author.value} on TCGLense.` : undefined,
  ),
  canonicalPath: computed(() => `/u/${props.handle}/decks/${props.id}`),
})

const sections = computed(() => deck.value?.sections ?? [])
const cardsBySection = computed(() => {
  const map = new Map<number, DeckCardEntry[]>()
  for (const s of sections.value) map.set(s.id, [])
  for (const c of deck.value?.cards ?? []) map.get(c.section_id)?.push(c)
  return map
})
const visibleSections = computed(() =>
  sections.value.filter((s) => (cardsBySection.value.get(s.id)?.length ?? 0) > 0),
)
function copies(entry: DeckCardEntry): number {
  return entry.quantity + entry.foil_quantity
}
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-8">
    <LoadingRow v-if="deckQuery.isPending.value" label="Loading deck…" />
    <div v-else-if="deckQuery.isError.value" class="py-20 text-center">
      <div class="bg-muted mx-auto flex size-12 items-center justify-center rounded-lg">
        <Layers class="size-6" aria-hidden="true" />
      </div>
      <h1 class="mt-4 text-xl font-semibold">Deck not found</h1>
      <p class="text-muted-foreground mt-1">This deck is private or doesn't exist.</p>
    </div>

    <template v-else-if="deck">
      <header class="mb-6">
        <h1 class="text-2xl font-semibold tracking-tight">{{ deck.name }}</h1>
        <p class="text-muted-foreground mt-1 text-sm">
          by
          <RouterLink :to="`/u/${handle}`" class="hover:text-foreground underline">{{
            author
          }}</RouterLink>
          · {{ deck.summary.total_cards }} card{{ deck.summary.total_cards === 1 ? '' : 's' }}
          <span v-if="deck.format"> · {{ deck.format }}</span>
          <span v-if="money.formatUsd(deck.summary.total_value_usd)">
            · {{ money.formatUsd(deck.summary.total_value_usd) }}</span
          >
        </p>
        <p v-if="deck.description" class="text-muted-foreground mt-2 text-sm">
          {{ deck.description }}
        </p>
      </header>

      <DeckStats :cards="deck.cards" :sections="deck.sections" />

      <section v-for="section in visibleSections" :key="section.id" class="mb-8">
        <h2 class="mb-3 border-b pb-1.5 font-medium">
          {{ section.name }}
          <span class="text-muted-foreground text-sm"
            >({{ cardsBySection.get(section.id)?.length ?? 0 }})</span
          >
        </h2>
        <div class="grid grid-cols-3 gap-3 sm:grid-cols-4 md:grid-cols-5 lg:grid-cols-6">
          <CardTile
            v-for="entry in cardsBySection.get(section.id) ?? []"
            :key="`${entry.card.id}-${entry.section_id}`"
            :game="deck.game"
            :card="entry.card"
          >
            <template #badge>
              <span
                class="bg-background/90 text-foreground absolute bottom-1.5 left-1.5 z-20 cursor-default rounded-md border px-1.5 py-0.5 text-xs font-medium shadow select-none tabular-nums"
                >×{{ copies(entry) }}</span
              >
            </template>
          </CardTile>
        </div>
      </section>
    </template>
  </div>
</template>
