<script setup lang="ts">
import { computed, ref } from 'vue'
import { RouterLink, useRouter } from 'vue-router'
import { ClipboardCopy, Copy, Layers } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import CardSizeMenu from '@/components/cards/CardSizeMenu.vue'
import CardTile from '@/components/cards/CardTile.vue'
import UpdatingCue from '@/components/cards/UpdatingCue.vue'
import DeckBracket from '@/components/decks/DeckBracket.vue'
import DeckColorFilter from '@/components/decks/DeckColorFilter.vue'
import DeckLegalityBanner from '@/components/decks/DeckLegalityBanner.vue'
import DeckCardRow from '@/components/decks/DeckCardRow.vue'
import DeckSectionNav from '@/components/decks/DeckSectionNav.vue'
import DeckGoldfish from '@/components/decks/DeckGoldfish.vue'
import DeckStats from '@/components/decks/DeckStats.vue'
import DeckTextList from '@/components/decks/DeckTextList.vue'
import DeckViewMenu from '@/components/decks/DeckViewMenu.vue'
import { useCopyPublicDeckMutation, usePublicDeckQuery } from '@/composables/useDecks'
import { usePublicDeckLegalityQuery } from '@/composables/useDeckAnalysis'
import { useCurrency } from '@/composables/useCurrency'
import { useDeckCardDisplay } from '@/composables/useDeckCardDisplay'
import { useAuthStore } from '@/stores/auth'
import { ApiError, type DeckCardEntry } from '@/lib/api'
import { DECK_CARD_SIZE_GRID_CLASS } from '@/lib/cardSize'
import { deckListText } from '@/lib/deckText'
import { deckSectionTargetId } from '@/lib/deckSectionNav'
import { DECK_ISSUE_TEXT_CLASS, deckIssueLabel } from '@/lib/legality'
import { usePageMeta } from '@/lib/seo'
import { useCardSizeStore } from '@/stores/cardSize'
import { useDeckViewStore } from '@/stores/deckView'

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

// Grouping + the card filter (issue #562) come from the display engine shared with the
// owner view; the size menu writes the same persisted preference every grid reads.
const sections = computed(() => deck.value?.sections ?? [])
const allCards = computed<DeckCardEntry[]>(() => deck.value?.cards ?? [])
const {
  filterQuery,
  filterColors,
  filterActive,
  clearFilters,
  cardsBySection,
  visibleSections,
  sectionNavItems,
  matchCount,
  totalCount,
} = useDeckCardDisplay({ cards: allCards, sections })
const cardSize = useCardSizeStore()
const deckView = useDeckViewStore()
function copies(entry: DeckCardEntry): number {
  return entry.quantity + entry.foil_quantity
}

// The visitor's copy of the shared list, for the text view's copy button.
const copiedList = ref(false)
function copyDeckList() {
  const text = deckListText(visibleSections.value, cardsBySection.value)
  if (!text) return
  void navigator.clipboard.writeText(text).then(() => {
    copiedList.value = true
    setTimeout(() => (copiedList.value = false), 2000)
  })
}

// Format legality (issues #557/#596), mirroring the owner view: the public mirror of the
// same server read, so a shared deck and its owner's copy can never disagree about the
// verdict. Null when the format isn't a legality-tracked one.
const legalityQuery = usePublicDeckLegalityQuery(handle, deckId)
const legality = computed(() => legalityQuery.data.value?.data ?? null)
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
            <span v-if="deck.maybeboard_summary.total_cards > 0">
              · +{{ deck.maybeboard_summary.total_cards }} maybeboard</span
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

      <!-- Is this deck legal in its format? (issue #557) — the server's verdict (#596), so
        it lands after the deck itself; the owner view says the same while it's in flight. -->
      <p v-if="legalityQuery.isPending.value" class="text-muted-foreground mb-4 text-sm">
        <UpdatingCue label="Checking format legality…" />
      </p>
      <DeckLegalityBanner v-else-if="legality" :legality="legality" class="mb-4" />

      <!-- Estimated Commander bracket, mirroring the owner view: the same server read, so a
        shared deck and its owner's copy can't disagree about its power level either. -->
      <DeckBracket
        v-if="deck.summary.total_cards > 0"
        :game="deck.game"
        :deck-id="deck.id"
        :handle="handle"
      />

      <DeckStats :game="deck.game" :deck-id="deck.id" :sections="deck.sections" :handle="handle" />

      <!-- Goldfish a sample hand from the shared deck (issue #596). -->
      <DeckGoldfish :game="deck.game" :deck-id="deck.id" :handle="handle" />

      <!-- Card list controls (issue #562), mirroring the owner view: client-side text +
        colour filters over the loaded deck, and the shared card-size preference. -->
      <div v-if="allCards.length > 0" class="mb-4 flex flex-wrap items-center gap-x-3 gap-y-2">
        <CardSearchBox
          v-model="filterQuery"
          class="w-full sm:w-60"
          placeholder="Filter cards…"
          aria-label="Filter cards by name, type, text, set, number, rarity, or language"
        />
        <DeckColorFilter v-model="filterColors" />
        <DeckViewMenu />
        <CardSizeMenu v-if="deckView.mode === 'grid'" />
        <Button v-if="deckView.mode === 'text'" variant="outline" size="sm" @click="copyDeckList">
          <ClipboardCopy class="size-4" /> {{ copiedList ? 'Copied!' : 'Copy list' }}
        </Button>
      </div>
      <p v-if="filterActive" class="text-muted-foreground mb-4 text-sm" aria-live="polite">
        Showing {{ matchCount }} of {{ totalCount }} card{{ totalCount === 1 ? '' : 's' }}.
        <button type="button" class="text-primary underline" @click="clearFilters">
          Clear filters
        </button>
      </p>
      <p
        v-if="filterActive && visibleSections.length === 0"
        class="text-muted-foreground py-12 text-center"
      >
        No cards in this deck match your filter.
      </p>

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
            <h2 class="mb-3 flex items-center gap-2 border-b pb-1.5 font-medium">
              {{ section.name }}
              <span class="text-muted-foreground text-sm"
                >({{ cardsBySection.get(section.id)?.length ?? 0 }})</span
              >
              <span
                v-if="section.is_maybeboard"
                class="text-muted-foreground rounded-md border px-1.5 py-0.5 text-xs font-medium"
                title="Cards here are not counted in the deck's totals, legality, or analytics"
                >Maybeboard</span
              >
            </h2>
            <!-- Text view (issue #570): the shared list as plain names and counts, ready
              to read or copy. -->
            <DeckTextList
              v-if="deckView.mode === 'text'"
              :game="deck.game"
              :entries="cardsBySection.get(section.id) ?? []"
            />

            <!-- Compact list view: the deck's facts in aligned columns; the copy count that
              owns the tile's bottom-left corner becomes this row's leading column. -->
            <div v-else-if="deckView.mode === 'list'" class="-mx-1.5 divide-y">
              <DeckCardRow
                v-for="entry in cardsBySection.get(section.id) ?? []"
                :key="`${entry.card.id}-${entry.section_id}`"
                :game="deck.game"
                :entry="entry"
                :legality-status="legality?.card_statuses[entry.card.id] ?? null"
              >
                <template #control>
                  <span class="text-sm font-medium tabular-nums">×{{ copies(entry) }}</span>
                </template>
              </DeckCardRow>
            </div>

            <div v-else class="grid gap-3" :class="DECK_CARD_SIZE_GRID_CLASS[cardSize.size]">
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
                  <!-- Format-legality breach chip (issue #557): bottom-right (the copy
                    count owns bottom-left), matching the owner view; pointer-events-none
                    keeps the tile's stretched link clickable through it. -->
                  <span
                    v-if="legality?.card_statuses[entry.card.id]"
                    class="bg-background/90 pointer-events-none absolute right-1.5 bottom-1.5 z-20 inline-flex items-center rounded-md border px-1.5 py-0.5 text-xs font-medium shadow select-none"
                    :class="DECK_ISSUE_TEXT_CLASS[legality.card_statuses[entry.card.id]!]"
                  >
                    {{ deckIssueLabel(legality.card_statuses[entry.card.id]!) }}
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
