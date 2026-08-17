<script setup lang="ts">
import { computed, reactive, toRef, watch } from 'vue'
import { RouterLink } from 'vue-router'
import { Layers } from '@lucide/vue'
import type { CardDeckRef, CardPreconRef } from '@/lib/api'
import { useAuthStore } from '@/stores/auth'
import { useDecksContainingQuery } from '@/composables/useDecks'
import { useCardPreconsQuery } from '@/composables/usePrecons'
import DeckIdentity from '@/components/decks/DeckIdentity.vue'
import PreconTile from '@/components/decks/PreconTile.vue'
import CollapsibleSection from '@/components/shared/CollapsibleSection.vue'

// The card-detail "Decks" section — the deck mirror of the sealed-products section beside
// it, and shaped the same way: one page-level heading, then a collapsible bucket per
// answer. Two buckets:
//
//  * "In your decks" — which of the signed-in user's decks run this card (any printing),
//    with a deck that's only *considering* it (maybeboard only) labelled rather than
//    hidden, and a note when the deck runs a different printing than the one on screen.
//    Signed out it renders nothing: the collection controls beside it already carry the
//    sign-in nudge, so a second one here would just repeat it.
//  * "Preconstructed decks" — the published decklists that include this card.
//
// The whole section renders nothing when both buckets are empty, like its siblings.
// Shown in both the full card page and the browse-grid modal (both mount
// CardDetailContent).
const props = defineProps<{ game: string; id: string }>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')

const auth = useAuthStore()
const ownedQuery = useDecksContainingQuery(game, id)
const owned = computed<CardDeckRef[]>(() => ownedQuery.data.value?.data ?? [])
const ownedVisible = computed(() => auth.isAuthenticated && owned.value.length > 0)

const preconQuery = useCardPreconsQuery(game, id)
const precons = computed<CardPreconRef[]>(() => preconQuery.data.value?.data ?? [])
const preconTotal = computed(() => preconQuery.data.value?.total ?? 0)

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

/** "DMB #80" — how a printing reads in a deck row's note. */
function printingLabel(p: CardDeckRef['printings'][number]): string {
  return `${p.set_code.toUpperCase()} #${p.collector_number}`
}

/** The note shown when a deck's copies aren't (all) the printing on screen: "As DMB #80"
 * when every copy is another printing, "Also as DMB #80" when this one is in there too. */
function printingsNote(entry: CardDeckRef): string {
  const others = entry.printings.filter((p) => p.id !== props.id)
  if (others.length === 0) return ''
  const labels = others.map(printingLabel).join(', ')
  const hasViewed = entry.printings.some((p) => p.id === props.id)
  return `${hasViewed ? 'Also as' : 'As'} ${labels}`
}

/** The chips that say how the card sits in a precon; the tile itself says what the deck
 * is. Quantity only when it adds something over "it's in there" (a pile of basics). */
function preconChips(entry: CardPreconRef): string[] {
  const out: string[] = []
  if (entry.commander) out.push('Commander')
  if (entry.foil) out.push('Foil')
  if (entry.quantity > 1) out.push(`×${entry.quantity}`)
  return out
}

// "Your decks" opens on arrival (it's the reader's own answer and rarely more than a
// handful of rows); the precon bucket waits collapsed with its count (a format staple is
// in hundreds of decks). Card-to-card navigation resets both.
const expanded = reactive({ owned: true, precons: false })
watch(id, () => {
  expanded.owned = true
  expanded.precons = false
})
</script>

<template>
  <section v-if="ownedVisible || precons.length">
    <h2 class="mb-3 text-base font-semibold tracking-tight">Decks</h2>
    <div class="space-y-3">
      <CollapsibleSection
        v-if="ownedVisible"
        v-model:expanded="expanded.owned"
        title="In your decks"
        :count="owned.length"
        blurb="Your decks that run this card (any printing)."
      >
        <ul class="space-y-2">
          <li v-for="entry in owned" :key="entry.deck.id">
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
                <p v-if="printingsNote(entry)" class="text-muted-foreground text-xs">
                  {{ printingsNote(entry) }}
                </p>
              </div>
              <DeckIdentity :deck="entry.deck" class="shrink-0" />
              <span
                class="shrink-0 rounded-md px-1.5 py-0.5 text-xs font-medium select-none"
                :class="
                  entry.quantity > 0 ? 'bg-muted text-muted-foreground' : 'bg-info/15 text-info'
                "
              >
                {{ entry.quantity > 0 ? copiesLabel(entry) : 'Considering' }}
              </span>
            </RouterLink>
          </li>
        </ul>
      </CollapsibleSection>

      <CollapsibleSection
        v-if="precons.length"
        v-model:expanded="expanded.precons"
        title="Preconstructed decks"
        :count="preconTotal"
        blurb="Published decklists that include this card (any printing)."
      >
        <div class="grid gap-3 sm:grid-cols-2">
          <div v-for="entry in precons" :key="entry.precon.slug" class="relative">
            <PreconTile :precon="entry.precon" :game="game" />
            <div
              v-if="preconChips(entry).length"
              class="pointer-events-none absolute top-2 right-2 flex flex-wrap justify-end gap-1"
            >
              <span
                v-for="chip in preconChips(entry)"
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
        <p v-if="preconTotal > precons.length" class="text-muted-foreground mt-3 text-xs">
          Showing the newest {{ precons.length.toLocaleString() }} of
          {{ preconTotal.toLocaleString() }} decks.
        </p>
      </CollapsibleSection>
    </div>
  </section>
</template>
