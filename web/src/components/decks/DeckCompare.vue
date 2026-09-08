<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { ArrowRightLeft, GitCompareArrows } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import StaleNotice from '@/components/cards/StaleNotice.vue'
import UpdatingCue from '@/components/cards/UpdatingCue.vue'
import { useDeckDiffQuery, useDecksQuery } from '@/composables/useDecks'
import { useDetailModalLink } from '@/composables/useDetailModalLink'
import type { DeckDiffSection } from '@/lib/api'
import {
  DECK_DIFF_CHANGE_CLASS,
  DECK_DIFF_CHANGE_LABEL,
  countsLabel,
  deltaLabel,
  foilLabel,
  groupByChange,
  summaryLabel,
} from '@/lib/deckDiff'

// **Compare with another deck** (issue #674): what changed between this deck and another of
// the owner's — the question that follows "make a v2" the moment there is one.
//
// Every number here is the server's (`GET /api/decks/{game}/{deck_id}/diff/{other_id}`),
// folded by card *name* across printings and finishes, so a printing swap is not a change
// and a playset split across two arts is one card. The panel only chooses which deck to
// compare with and how to lay the answer out:
//
// * **The picked deck rides the URL** (`?compare=<id>`), so a comparison is a link the owner
//   can come back to or hand to a second tab, and the browser's back button undoes a pick.
// * **Two layouts of one response.** "By section" lists each section (matched by name) with
//   its own added / removed / changed rows — a card moved between sections shows in both,
//   which is what happened there. "Whole deck" is the section-agnostic fold over the deck
//   proper, where such a move nets to nothing. The header summary always counts the latter.
// * **Maybeboards** appear in the section view flagged, as the deck page shows them, and
//   never in the whole-deck view or its summary — a card under consideration isn't a change
//   to the deck (issue #570).
const props = defineProps<{
  game: string
  deckId: number
}>()

const game = toRef(props, 'game')
const deckId = toRef(props, 'deckId')

const route = useRoute()
const router = useRouter()

// The other decks in this game — everything but the deck on screen. The list is the same
// `['decks', game]` entry the deck index reads, so it's usually already cached.
const decksQuery = useDecksQuery(game)
const others = computed(() =>
  (decksQuery.data.value?.data ?? []).filter((deck) => deck.id !== props.deckId),
)

/** The `?compare=` id, or null when absent or not a positive integer. */
function idFromQuery(value: unknown): number | null {
  const raw = Array.isArray(value) ? value[0] : value
  if (typeof raw !== 'string' || !/^\d+$/.test(raw)) return null
  const id = Number(raw)
  return id > 0 && id !== props.deckId ? id : null
}

const otherId = ref<number | null>(idFromQuery(route.query.compare))
watch(
  () => route.query.compare,
  (value) => {
    otherId.value = idFromQuery(value)
  },
)

/** The Select's string model: `''` for nothing picked (the trigger shows its placeholder). */
const selection = computed({
  get: () => (otherId.value == null ? '' : String(otherId.value)),
  set: (value: string) => {
    const id = /^\d+$/.test(value) ? Number(value) : null
    otherId.value = id
    // `replace`, not `push`: re-picking within one visit shouldn't stack history entries.
    const query = { ...route.query }
    if (id == null) delete query.compare
    else query.compare = String(id)
    void router.replace({ query })
  },
})

const otherIdForQuery = computed(() => otherId.value ?? 0)
const enabled = computed(() => otherId.value != null)
const diffQuery = useDeckDiffQuery(game, deckId, otherIdForQuery, enabled)
const diff = computed(() => (enabled.value ? diffQuery.data.value : undefined))
const pending = computed(() => enabled.value && diffQuery.isPending.value)
const updating = computed(() => enabled.value && diffQuery.isFetching.value && !pending.value)

const layout = ref<'sections' | 'deck'>('sections')

/** The whole-deck fold, shaped like a single section so both layouts share one renderer. */
const wholeDeck = computed<DeckDiffSection[]>(() =>
  diff.value && diff.value.cards.length
    ? [
        {
          name: 'Whole deck',
          base_section_id: null,
          other_section_id: null,
          is_maybeboard: false,
          entries: diff.value.cards,
          unchanged: diff.value.summary.unchanged,
        },
      ]
    : [],
)
const sections = computed<DeckDiffSection[]>(() =>
  layout.value === 'deck' ? wholeDeck.value : (diff.value?.sections ?? []),
)

const { hrefFor, onActivate, warm } = useDetailModalLink()
</script>

<template>
  <Card class="mt-6 gap-3 py-4" :aria-busy="updating || undefined">
    <CardHeader class="pb-0">
      <div class="flex flex-wrap items-center justify-between gap-x-4 gap-y-2">
        <CardTitle class="flex items-center gap-2 text-base">
          <GitCompareArrows class="size-4" aria-hidden="true" /> Compare with another deck
        </CardTitle>
        <!-- The picker is a plain select: a user has a few decks, not a searchable list. -->
        <Select v-if="others.length" v-model="selection">
          <SelectTrigger class="w-full sm:w-64" aria-label="Deck to compare with">
            <SelectValue placeholder="Pick a deck…" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem v-for="other in others" :key="other.id" :value="String(other.id)">
              {{ other.name }}
              <span class="text-muted-foreground tabular-nums">
                · {{ other.card_count }} card{{ other.card_count === 1 ? '' : 's' }}</span
              >
            </SelectItem>
          </SelectContent>
        </Select>
      </div>
      <p class="text-muted-foreground text-xs" aria-live="polite">
        <template v-if="updating"><UpdatingCue label="Recomparing…" /></template>
        <template v-else-if="diff">
          <span class="font-medium">{{ diff.base.name }}</span>
          <ArrowRightLeft class="mx-1 inline size-3 align-[-2px]" aria-hidden="true" />
          <RouterLink :to="`/decks/${game}/${diff.other.id}`" class="font-medium underline">{{
            diff.other.name
          }}</RouterLink>
          · {{ summaryLabel(diff.summary) }}
        </template>
        <template v-else-if="!others.length && !decksQuery.isPending.value">
          Duplicate this deck (in the settings menu) to make a version you can compare against.
        </template>
        <template v-else>
          What changed between this deck and another of yours — added, removed, and count or finish
          changes, by card.
        </template>
      </p>
    </CardHeader>

    <CardContent v-if="enabled" class="space-y-4">
      <p v-if="pending" class="text-muted-foreground text-sm">
        <UpdatingCue label="Comparing…" />
      </p>
      <!-- `isLoadingError`, not `isError` (issue #622): a hiccuped refetch keeps the last
        comparison and says so above it rather than blanking the panel. -->
      <p v-else-if="diffQuery.isLoadingError.value" class="text-muted-foreground text-sm">
        Couldn't compare these decks.
      </p>
      <template v-else-if="diff">
        <StaleNotice
          v-if="diffQuery.isRefetchError.value"
          label="Couldn't refresh — showing the comparison as it last loaded."
        />
        <div class="flex flex-wrap items-center gap-2">
          <Button
            size="sm"
            :variant="layout === 'sections' ? 'secondary' : 'ghost'"
            :aria-pressed="layout === 'sections'"
            @click="layout = 'sections'"
            >By section</Button
          >
          <Button
            size="sm"
            :variant="layout === 'deck' ? 'secondary' : 'ghost'"
            :aria-pressed="layout === 'deck'"
            @click="layout = 'deck'"
            >Whole deck</Button
          >
          <span class="text-muted-foreground ml-auto text-xs tabular-nums">
            {{ diff.base.total_cards }} → {{ diff.other.total_cards }} cards
          </span>
        </div>

        <p v-if="!sections.length" class="text-muted-foreground text-sm">
          <template v-if="layout === 'deck' && diff.sections.length">
            The two decks play the same cards — only which section they're filed under differs.
          </template>
          <template v-else>These two decks hold the same cards in the same counts.</template>
        </p>

        <section v-for="section in sections" :key="section.name" class="space-y-2">
          <div class="flex items-baseline justify-between gap-2 border-b pb-1">
            <h3 class="flex items-center gap-2 text-sm font-medium">
              {{ section.name }}
              <span
                v-if="section.is_maybeboard"
                class="text-muted-foreground rounded-md border px-1.5 py-0.5 text-xs font-medium"
                >Maybeboard</span
              >
            </h3>
            <span v-if="section.unchanged > 0" class="text-muted-foreground text-xs tabular-nums"
              >{{ section.unchanged }} unchanged</span
            >
          </div>
          <div v-for="group in groupByChange(section.entries)" :key="group.change">
            <h4 class="text-muted-foreground mb-1 text-xs font-medium uppercase tracking-wide">
              {{ DECK_DIFF_CHANGE_LABEL[group.change] }}
            </h4>
            <ul class="divide-y">
              <li
                v-for="entry in group.entries"
                :key="entry.name"
                class="flex items-center gap-3 py-1 text-sm"
              >
                <!-- The delta chip carries the change's token; the row's text stays neutral,
                  so a long list of removals doesn't read as one red block. -->
                <span
                  class="inline-flex w-10 shrink-0 justify-center rounded-md px-1.5 py-0.5 text-xs font-medium tabular-nums"
                  :class="DECK_DIFF_CHANGE_CLASS[entry.change]"
                  :title="DECK_DIFF_CHANGE_LABEL[entry.change]"
                  >{{ deltaLabel(entry.delta) }}</span
                >
                <a
                  :href="hrefFor('card', game, entry.card.id)"
                  class="min-w-0 flex-1 truncate hover:underline"
                  @click="onActivate($event, 'card', game, entry.card.id)"
                  @pointerenter="warm('card')"
                  @focusin="warm('card')"
                  >{{ entry.name }}</a
                >
                <span class="text-muted-foreground shrink-0 text-xs tabular-nums">{{
                  [countsLabel(entry), foilLabel(entry)].filter(Boolean).join(' · ')
                }}</span>
              </li>
            </ul>
          </div>
        </section>
      </template>
    </CardContent>
  </Card>
</template>
