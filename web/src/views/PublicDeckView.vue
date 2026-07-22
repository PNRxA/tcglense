<script setup lang="ts">
import { computed, ref } from 'vue'
import { RouterLink, useRouter } from 'vue-router'
import { Copy, Layers } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import CardTile from '@/components/cards/CardTile.vue'
import DeckLegalityBanner from '@/components/decks/DeckLegalityBanner.vue'
import DeckSectionNav from '@/components/decks/DeckSectionNav.vue'
import DeckStats from '@/components/decks/DeckStats.vue'
import { useCopyPublicDeckMutation, usePublicDeckQuery } from '@/composables/useDecks'
import { useCurrency } from '@/composables/useCurrency'
import { useAuthStore } from '@/stores/auth'
import { ApiError, type DeckCardEntry } from '@/lib/api'
import { deckSectionTargetId } from '@/lib/deckSectionNav'
import { evaluateDeckLegality, legalityLabel } from '@/lib/legality'
import { usePageMeta } from '@/lib/seo'

// The read-only, shareable public deck (issue #363): `/u/:handle/decks/:id`. Anyone can
// view; the only control is "Copy to my decks" for a signed-in visitor (issue #502).
// Indexable so shared links preview and rank.
const props = defineProps<{ handle: string; id: string }>()
const money = useCurrency()
const auth = useAuthStore()
const router = useRouter()
const handle = computed(() => props.handle)
const deckId = computed(() => Number(props.id))
const deckQuery = usePublicDeckQuery(handle, deckId)
const deck = computed(() => deckQuery.data.value)

// Copy-to-my-decks (issue #502): offered to any signed-in visitor except the deck's own
// owner (they already have it). Gate on `sessionResolved` so the button doesn't flash in and
// out while the session restores on first paint.
const copyMutation = useCopyPublicDeckMutation()
const copyError = ref('')
const isOwnDeck = computed(() => !!deck.value?.handle && auth.user?.handle === deck.value.handle)
const canCopy = computed(() => auth.sessionResolved && auth.isAuthenticated && !isOwnDeck.value)

async function copyDeck() {
  copyError.value = ''
  try {
    const created = await copyMutation.mutateAsync({ handle: handle.value, deckId: deckId.value })
    void router.push(`/decks/${created.game}/${created.id}`)
  } catch (error) {
    copyError.value =
      error instanceof ApiError ? error.message : 'The deck could not be copied. Please retry.'
  }
}

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
const sectionNavItems = computed(() =>
  visibleSections.value.map((section) => ({
    id: section.id,
    name: section.name,
    count: cardsBySection.value.get(section.id)?.length ?? 0,
  })),
)
function copies(entry: DeckCardEntry): number {
  return entry.quantity + entry.foil_quantity
}

// Format legality (issue #557), mirroring the owner view: computed from the cards the
// page already holds; null when the format isn't a legality-tracked one.
const legality = computed(() =>
  deck.value ? evaluateDeckLegality(deck.value.format, deck.value.cards) : null,
)
// Breach chips sit top-left; the copy-count badge owns bottom-left here.
const LEGALITY_CHIP_TEXT: Record<string, string> = {
  banned: 'text-red-600 dark:text-red-400',
  not_legal: 'text-muted-foreground',
  restricted: 'text-amber-600 dark:text-amber-400',
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
      <header class="mb-6 flex flex-wrap items-start justify-between gap-3">
        <div class="min-w-0">
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
        </div>
        <div v-if="canCopy" class="flex shrink-0 flex-col items-end gap-1">
          <Button
            variant="outline"
            size="sm"
            :disabled="copyMutation.isPending.value"
            @click="copyDeck"
          >
            <Copy class="size-4" aria-hidden="true" />
            {{ copyMutation.isPending.value ? 'Copying…' : 'Copy to my decks' }}
          </Button>
          <p v-if="copyError" class="text-destructive max-w-xs text-right text-xs">
            {{ copyError }}
          </p>
        </div>
      </header>

      <!-- Is this deck legal in its format? (issue #557) -->
      <DeckLegalityBanner v-if="legality" :legality="legality" class="mb-4" />

      <DeckStats :cards="deck.cards" :sections="deck.sections" />

      <div
        v-if="visibleSections.length > 0"
        class="xl:grid xl:grid-cols-[12rem_minmax(0,1fr)] xl:gap-6"
      >
        <DeckSectionNav :items="sectionNavItems" />
        <div class="min-w-0">
          <section
            v-for="section in visibleSections"
            :id="deckSectionTargetId(section.id)"
            :key="section.id"
            class="mb-8 scroll-mt-16"
          >
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
                  <!-- Format-legality breach chip (issue #557), matching the owner view. -->
                  <span
                    v-if="legality?.statusByCardId.get(entry.card.id)"
                    class="bg-background/90 absolute top-1.5 left-1.5 z-20 inline-flex cursor-default items-center rounded-md border px-1.5 py-0.5 text-xs font-medium shadow select-none"
                    :class="LEGALITY_CHIP_TEXT[legality.statusByCardId.get(entry.card.id)!]"
                    :title="`${legalityLabel(legality.statusByCardId.get(entry.card.id)!)} in ${legality.formatLabel}`"
                  >
                    {{ legalityLabel(legality.statusByCardId.get(entry.card.id)!) }}
                  </span>
                </template>
              </CardTile>
            </div>
          </section>
        </div>
      </div>
    </template>
  </div>
</template>
