<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { RouterLink, useRouter } from 'vue-router'
import { ClipboardCopy, Copy, Layers, Package } from '@lucide/vue'
import { Button, buttonVariants } from '@/components/ui/button'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import CardSizeMenu from '@/components/cards/CardSizeMenu.vue'
import CardTile from '@/components/cards/CardTile.vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import ManaSymbols from '@/components/cards/ManaSymbols.vue'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import DeckCardRow from '@/components/decks/DeckCardRow.vue'
import DeckColorFilter from '@/components/decks/DeckColorFilter.vue'
import DeckSectionNav from '@/components/decks/DeckSectionNav.vue'
import DeckTextList from '@/components/decks/DeckTextList.vue'
import DeckViewMenu from '@/components/decks/DeckViewMenu.vue'
import { useCurrency } from '@/composables/useCurrency'
import { useDeckCardDisplay } from '@/composables/useDeckCardDisplay'
import { useGameName } from '@/composables/useCatalog'
import { useCopyPreconMutation, usePreconQuery } from '@/composables/usePrecons'
import { ApiError } from '@/lib/api'
import { DECK_CARD_SIZE_GRID_CLASS } from '@/lib/cardSize'
import { deckListText } from '@/lib/deckText'
import { deckSectionTargetId } from '@/lib/deckSectionNav'
import { colorLettersToText } from '@/lib/mana'
import { formatReleaseLabel } from '@/lib/releaseDate'
import { preconBoards } from '@/lib/precons'
import { usePageMeta } from '@/lib/seo'
import { useAuthStore } from '@/stores/auth'
import { useCardSizeStore } from '@/stores/cardSize'
import { useDeckViewStore } from '@/stores/deckView'

// One preconstructed deck's full list: `/decks/:game/precons/:slug`. Public and indexable —
// a published decklist is catalog data — with one authed action, "Copy to my decks", which
// creates a normal deck of the visitor's and lands them in the builder.
//
// The card list is rendered by the *deck* display engine, not a second renderer: the API's
// boards are adapted into sections by `lib/precons`, so the filters, the section nav, the
// grid/list/text views and the card-size preference are all the ones the deck page uses, and
// this page can't drift from it.
const props = defineProps<{ game: string; slug: string }>()
const game = toRef(props, 'game')
const slug = toRef(props, 'slug')
const gameName = useGameName(game)
const money = useCurrency()
const auth = useAuthStore()
const router = useRouter()

const preconQuery = usePreconQuery(game, slug)
const precon = computed(() => preconQuery.data.value)

const { sections, entries } = (() => {
  const adapted = computed(() => preconBoards(precon.value?.cards ?? []))
  return {
    sections: computed(() => adapted.value.sections),
    entries: computed(() => adapted.value.entries),
  }
})()

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
} = useDeckCardDisplay({ cards: entries, sections })

const cardSize = useCardSizeStore()
const deckView = useDeckViewStore()

// "released 14 Nov 2026" / "releases 14 Nov 2026" — MTGJSON ships upcoming sets, and the
// browse defaults to newest-first, so a precon that hasn't come out yet leads the grid. The
// shared helper flips the verb by tense (and parses the date as local midnight, so the day
// doesn't slip a timezone).
const releaseLabel = computed(() => {
  const label = formatReleaseLabel(precon.value?.released_at, 'short')?.label
  // Only the leading verb is lowercased: this sits mid-sentence in the meta line, but the
  // month keeps its capital ("released Jun 20, 2024", not "jun").
  return label ? label.charAt(0).toLowerCase() + label.slice(1) : null
})

const identityText = computed(() => {
  const letters = precon.value?.color_identity
  if (!letters) return ''
  return letters.length ? colorLettersToText(letters) : '{C}'
})

/** Copy the whole list to the clipboard from the text view, as the deck pages do. */
const copiedList = ref(false)
function copyDeckList() {
  const text = deckListText(visibleSections.value, cardsBySection.value)
  if (!text) return
  void navigator.clipboard.writeText(text).then(() => {
    copiedList.value = true
    setTimeout(() => (copiedList.value = false), 2000)
  })
}

// Copy into the visitor's own decks. Offered to any signed-in visitor; gate on
// `sessionResolved` so the button doesn't flash in and out while the session restores.
const copyMutation = useCopyPreconMutation()
const copyError = ref('')
const canCopy = computed(() => auth.sessionResolved && auth.isAuthenticated)

async function copyToMyDecks() {
  copyError.value = ''
  try {
    const created = await copyMutation.mutateAsync({ game: game.value, slug: slug.value })
    void router.push(`/decks/${created.game}/${created.id}`)
  } catch (error) {
    copyError.value =
      error instanceof ApiError ? error.message : 'The deck could not be copied. Please retry.'
  }
}

usePageMeta({
  title: computed(() =>
    precon.value ? `${precon.value.name} — ${precon.value.deck_type}` : 'Preconstructed deck',
  ),
  description: computed(() =>
    precon.value
      ? `The full decklist for ${precon.value.name}, the ${precon.value.deck_type} from ` +
        `${precon.value.set_name ?? precon.value.set_code.toUpperCase()} — ` +
        `${precon.value.card_count} cards, with prices, on TCGLense.`
      : undefined,
  ),
  canonicalPath: computed(() => `/decks/${game.value}/precons/${slug.value}`),
})
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-8">
    <LoadingRow v-if="preconQuery.isPending.value" label="Loading deck…" />
    <div v-else-if="preconQuery.isError.value" class="py-20 text-center">
      <div class="bg-muted mx-auto flex size-12 items-center justify-center rounded-lg">
        <Layers class="size-6" aria-hidden="true" />
      </div>
      <h1 class="mt-4 text-xl font-semibold">Deck not found</h1>
      <p class="text-muted-foreground mt-1">We don't have a preconstructed deck with that name.</p>
      <RouterLink
        :class="[buttonVariants({ variant: 'outline' }), 'mt-6']"
        :to="`/decks/${game}/precons`"
      >
        Browse preconstructed decks
      </RouterLink>
    </div>

    <template v-else-if="precon">
      <PageBreadcrumbs
        :items="[
          { label: 'Decks', to: '/decks' },
          { label: gameName, to: `/decks/${game}` },
          { label: 'Preconstructed', to: `/decks/${game}/precons` },
          { label: precon.name },
        ]"
      />

      <header class="mb-6 flex flex-wrap items-start justify-between gap-3">
        <div class="min-w-0">
          <h1 class="flex flex-wrap items-center gap-2 text-2xl font-semibold tracking-tight">
            {{ precon.name }}
            <ManaSymbols v-if="identityText" :text="identityText" class="leading-none" />
          </h1>
          <p class="text-muted-foreground mt-1 text-sm">
            {{ precon.deck_type }}
            <span v-if="precon.set_name">
              ·
              <RouterLink :to="`/cards/${game}/sets/${precon.set_code}`" class="hover:underline">
                {{ precon.set_name }}
              </RouterLink>
            </span>
            <span v-if="releaseLabel"> · {{ releaseLabel }}</span>
            · {{ precon.card_count }} card{{ precon.card_count === 1 ? '' : 's' }}
            <span v-if="precon.sideboard_count"> · +{{ precon.sideboard_count }} sideboard</span>
            <span v-if="money.formatUsd(precon.summary.total_value_usd)">
              · singles worth {{ money.formatUsd(precon.summary.total_value_usd) }}</span
            >
          </p>
        </div>
        <div class="flex shrink-0 flex-col items-end gap-1">
          <div class="flex flex-wrap items-center gap-2">
            <!-- The sealed product it ships in, when the catalog has it: its price is what
              the deck costs to buy, next to what its singles are worth. -->
            <RouterLink
              v-if="precon.product"
              :class="buttonVariants({ variant: 'outline', size: 'sm' })"
              :to="`/sealed/${game}/${precon.product.id}`"
            >
              <Package class="size-4" aria-hidden="true" />
              <span v-if="money.formatUsd(precon.product.prices.usd)">
                Buy sealed · {{ money.formatUsd(precon.product.prices.usd) }}
              </span>
              <span v-else>Sealed product</span>
            </RouterLink>
            <Button
              v-if="canCopy"
              size="sm"
              :disabled="copyMutation.isPending.value"
              @click="copyToMyDecks"
            >
              <Copy class="size-4" aria-hidden="true" />
              {{ copyMutation.isPending.value ? 'Copying…' : 'Copy to my decks' }}
            </Button>
            <RouterLink
              v-else-if="auth.sessionResolved"
              :class="buttonVariants({ variant: 'outline', size: 'sm' })"
              :to="{ path: '/login', query: { redirect: `/decks/${game}/precons/${slug}` } }"
            >
              <Copy class="size-4" aria-hidden="true" /> Sign in to copy
            </RouterLink>
          </div>
          <p v-if="copyError" class="text-destructive max-w-xs text-right text-xs">
            {{ copyError }}
          </p>
        </div>
      </header>

      <!-- Card list controls, the same set the deck pages carry. -->
      <div v-if="entries.length > 0" class="mb-4 flex flex-wrap items-center gap-x-3 gap-y-2">
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
            </h2>

            <DeckTextList
              v-if="deckView.mode === 'text'"
              :game="game"
              :entries="cardsBySection.get(section.id) ?? []"
            />

            <div v-else-if="deckView.mode === 'list'" class="-mx-1.5 divide-y">
              <DeckCardRow
                v-for="entry in cardsBySection.get(section.id) ?? []"
                :key="`${entry.card.id}-${entry.section_id}`"
                :game="game"
                :entry="entry"
                :legality-status="null"
              >
                <template #control>
                  <span class="text-sm font-medium tabular-nums"
                    >×{{ entry.quantity + entry.foil_quantity }}</span
                  >
                </template>
              </DeckCardRow>
            </div>

            <div v-else class="grid gap-3" :class="DECK_CARD_SIZE_GRID_CLASS[cardSize.size]">
              <CardTile
                v-for="entry in cardsBySection.get(section.id) ?? []"
                :key="`${entry.card.id}-${entry.section_id}`"
                :game="game"
                :card="entry.card"
              >
                <template #badge>
                  <span
                    class="bg-background/90 text-foreground absolute bottom-1.5 left-1.5 z-20 cursor-default rounded-md border px-1.5 py-0.5 text-xs font-medium shadow select-none tabular-nums"
                    >×{{ entry.quantity + entry.foil_quantity }}</span
                  >
                  <!-- Which of those copies are foil. The count above is every copy the deck
                    ships (the two finish rows are folded, as the copy endpoint folds them), so
                    a printing that comes partly foil says how many rather than tagging the
                    whole tile "Foil". -->
                  <span
                    v-if="entry.foil_quantity > 0"
                    class="bg-background/90 text-foil pointer-events-none absolute right-1.5 bottom-1.5 z-20 inline-flex items-center rounded-md border px-1.5 py-0.5 text-xs font-medium shadow select-none"
                  >
                    {{ entry.quantity > 0 ? `${entry.foil_quantity} foil` : 'Foil' }}
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
