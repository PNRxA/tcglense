<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
import { RouterLink } from 'vue-router'
import { Layers } from '@lucide/vue'
import type { CardDeckRef } from '@/lib/api'
import { useAuthStore } from '@/stores/auth'
import { useDecksContainingQuery } from '@/composables/useDecks'
import DeckIdentity from '@/components/decks/DeckIdentity.vue'
import CollapsibleSection from '@/components/shared/CollapsibleSection.vue'

// The card-detail "In your decks" section: which of the signed-in user's decks run this
// card — any printing of it — with a deck that's only *considering* it (maybeboard only)
// labelled rather than hidden. Signed out, or with no deck holding the card, it renders
// nothing at all: the collection controls beside it already carry the sign-in nudge, so a
// second one here would just repeat it.
const props = defineProps<{ game: string; id: string }>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')

const auth = useAuthStore()
const query = useDecksContainingQuery(game, id)
const entries = computed<CardDeckRef[]>(() => query.data.value?.data ?? [])

/** "2 copies", "3 considered", or both — how the deck holds the card. */
function copiesLabel(entry: CardDeckRef): string {
  const parts: string[] = []
  if (entry.quantity > 0) {
    parts.push(`${entry.quantity} ${entry.quantity === 1 ? 'copy' : 'copies'}`)
  }
  if (entry.maybeboard_quantity > 0) {
    parts.push(`${entry.maybeboard_quantity} considered`)
  }
  return parts.join(' · ')
}

// Open by default: it's the reader's own answer and rarely more than a handful of rows.
const expanded = ref(true)
watch(id, () => {
  expanded.value = true
})
</script>

<template>
  <CollapsibleSection
    v-if="auth.isAuthenticated && entries.length"
    v-model:expanded="expanded"
    title="In your decks"
    :count="entries.length"
    blurb="Your decks that run this card (any printing)."
    heading="h2"
  >
    <ul class="space-y-2">
      <li v-for="entry in entries" :key="entry.deck.id">
        <RouterLink
          :to="`/decks/${game}/${entry.deck.id}`"
          class="bg-card hover:border-primary/50 flex items-center gap-3 rounded-lg border p-3 transition"
        >
          <Layers class="text-muted-foreground size-4 shrink-0" aria-hidden="true" />
          <div class="min-w-0 flex-1">
            <p class="truncate font-medium" :title="entry.deck.name">{{ entry.deck.name }}</p>
            <p class="text-muted-foreground text-sm">
              {{ entry.deck.card_count }} card{{ entry.deck.card_count === 1 ? '' : 's' }}
              <span v-if="entry.deck.format"> · {{ entry.deck.format }}</span>
            </p>
          </div>
          <DeckIdentity :deck="entry.deck" class="shrink-0" />
          <span
            class="shrink-0 rounded-md px-1.5 py-0.5 text-xs font-medium select-none"
            :class="entry.quantity > 0 ? 'bg-muted text-muted-foreground' : 'bg-info/15 text-info'"
          >
            {{ entry.quantity > 0 ? copiesLabel(entry) : 'Considering' }}
          </span>
        </RouterLink>
      </li>
    </ul>
  </CollapsibleSection>
</template>
