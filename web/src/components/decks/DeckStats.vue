<script setup lang="ts">
import { computed, ref, toRef, useId, watch } from 'vue'
import { ChevronDown } from '@lucide/vue'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Skeleton } from '@/components/ui/skeleton'
import ManaSymbols from '@/components/cards/ManaSymbols.vue'
import UpdatingCue from '@/components/cards/UpdatingCue.vue'
import DeckStatBars from '@/components/decks/DeckStatBars.vue'
import {
  useDeckStatsQuery,
  usePreconStatsQuery,
  usePublicDeckStatsQuery,
} from '@/composables/useDeckAnalysis'
import type { DeckSection } from '@/lib/api'

// The deck page's analytics panel. Everything it shows is computed server-side (issue
// #596) — `GET /api/decks/{game}/{deck_id}/stats`, or its public mirror — so a CLI can ask
// the same question and get the same numbers. What's left here is the two controls that
// pick *which* question: which sections count as the shuffled library, and which card the
// draw odds are for.
//
// The full panel is most of a screen — three labelled distributions and an interactive draw
// calculator — and it sits above the card list on a page whose subject is the card list, so
// it **rests collapsed** and opens on "Details". What stays visible is a summary chosen to be
// worth the two rows it costs: the land count *with its share*, the average mana value, the
// colour weights, and the mana curve as a strip. None of those are on the page already (the
// header prints the deck's size, format and value), and all of them are read straight off the
// same response the expanded body reads — the summary is the summary *of* the detail, exactly
// as `DeckBracket`'s chips are, so the two readings can never make different claims. That is
// also why the summary stays on screen while open rather than being swapped out: hiding it on
// expand would jump the layout and cut the tie between the two.
//
// The disclosure is `DeckBracket`'s, class for class (rotating chevron, `aria-expanded`,
// body mounted only while open) — including its **per-mount** state. A remembered preference
// was the tempting alternative, but the two disclosures sit ~100px apart on this page and
// behaving differently across a reload is exactly the disagreement a reader notices; a
// remembered "expanded" would also hand the full-height panel back to phone visitors on every
// deck they open, which is the thing this change exists to stop.
//
// The resting rows read `analytics.deck` only. `analytics.library` is chosen by the section
// checkboxes, which the collapsed panel hides — so a library-derived number in the summary
// would be an answer to a question the viewer can't see. The one exception is deliberate and
// gated on the viewer having asked it themselves (see `restingOdds` below).
//
// `handle` puts the panel in public mode (a deck someone shared), exactly as
// `ProductHoldingSection` does: the surface is fixed per mount, so the query hook is
// selected once below.
const props = defineProps<{
  game: string
  /** Addressing is fixed at mount and mutually exclusive: a deck id (the owner's own deck),
   *  a `handle` + deck id (a shared deck), or a `preconSlug` (a published catalog decklist,
   *  which has no deck row and no numeric id). Exactly one applies. */
  deckId?: number
  sections: DeckSection[]
  handle?: string
  preconSlug?: string
}>()

// `null` means "whatever the server picks" — the default library is its answer, not ours,
// and asking for it by name would need us to know the rule that chooses it. Once the user
// touches a checkbox this holds their explicit selection, and an empty array is a real
// answer ("no sections") rather than a fallback to the default.
const chosenSections = ref<number[] | null>(null)
// The user's explicit pick, or null to let the server choose the most-copied card.
const chosenCard = ref<string | null>(null)
const cardsSeen = ref(7)

// A section added, removed, or renamed invalidates an explicit selection made against the
// old list, so fall back to the server's default rather than silently dropping ids.
watch(
  () => props.sections.map((s) => `${s.id}:${s.name}:${s.is_maybeboard}`).join('|'),
  () => {
    chosenSections.value = null
  },
)

// Deliberately the raw pick, not the validated one below: the validated card is derived
// *from* the response, so feeding it back into the request would be a cycle. The server
// already defines the fallback for a `card=` it can't find (the most-copied one), so a pick
// that has left the library costs nothing to keep sending — and it means re-adding the
// section it lived in restores the viewer's choice rather than silently forgetting it.
const params = computed(() => ({
  sections: chosenSections.value ?? undefined,
  card: chosenCard.value ?? undefined,
}))

const game = toRef(props, 'game')
const deckId = computed(() => props.deckId ?? 0)
const handle = computed(() => props.handle ?? '')
const preconSlug = computed(() => props.preconSlug ?? '')
// Three addressing modes, selected ONCE at mount (a component never changes mode), so only
// the hook for the mode this page is in ever fetches.
const statsQuery = props.preconSlug
  ? usePreconStatsQuery(game, preconSlug, params)
  : props.handle
    ? usePublicDeckStatsQuery(handle, deckId, params)
    : useDeckStatsQuery(game, deckId, params)
const analytics = computed(() => statsQuery.data.value)
const stats = computed(() => analytics.value?.deck)
const library = computed(() => analytics.value?.library)
const odds = computed(() => analytics.value?.odds ?? null)

// Every number here is now a round trip (issue #596 moved the maths server-side), so the
// panel has a latency the props-derived version couldn't have — and it needs to say so.
//
// `pending` is the first fetch: nothing has arrived, so the panel draws its own shape as a
// skeleton instead of being absent until the response lands and then shoving the card list
// down the page. `updating` is a fetch over numbers already on screen — a new library
// selection, a different card, or a deck edit invalidating the analysis. Those keep the
// previous response (keepPreviousData) rather than blanking, which is exactly why they need
// a cue: without one, changing a checkbox looks like it did nothing at all.
const pending = computed(() => statsQuery.isPending.value)
const failed = computed(() => statsQuery.isError.value)
const updating = computed(() => statsQuery.isFetching.value && !pending.value)

// The checkbox model reads through to the server's default until the user overrides it, so
// the panel never has to reproduce the rule that picks the default library.
const drawSectionIds = computed<number[]>({
  get: () => chosenSections.value ?? analytics.value?.default_library_section_ids ?? [],
  set: (value) => {
    chosenSections.value = value
  },
})
const allSectionsSelected = computed(() => drawSectionIds.value.length === props.sections.length)
const noSectionsSelected = computed(() => drawSectionIds.value.length === 0)
function selectAllSections() {
  chosenSections.value = props.sections.map((section) => section.id)
}
function deselectAllSections() {
  chosenSections.value = []
}

// A pick can leave the library — deselect the section it lived in — and the server then
// falls back to the most-copied card. The select has to follow that fallback: showing the
// dead pick would put a name on a percentage that belongs to a different card, and there
// would be no matching option for it either.
const activeCard = computed(() => {
  const picked = chosenCard.value
  if (picked == null) return null
  const offered = library.value?.card_odds
  return offered && !offered.some((item) => item.name === picked) ? null : picked
})

// The select shows the server's pick until the user chooses otherwise; writing to it is
// what makes the next request ask about a different card.
const selectedCard = computed<string>({
  get: () => activeCard.value ?? odds.value?.name ?? '',
  set: (value) => {
    chosenCard.value = value
  },
})

// The response carries the whole odds curve (P(≥1) at 1..N cards seen), so the slider is
// instant and the probability maths stays in the one place it now lives.
//
// The clamp is at READ time rather than a watcher writing back to `cardsSeen`: a watcher
// only fires when `maxCardsSeen` *changes*, so a response already in the vue-query cache
// (navigate away and back) would leave `cardsSeen` at 7 against a shorter curve and read
// `undefined` — rendering 0% for a card that is certain to be drawn. Reading through also
// means the slider keeps its position when the library grows back.
const maxCardsSeen = computed(() => Math.max(1, odds.value?.curve.length ?? 1))
const seenIndex = computed(() => Math.min(Math.max(1, cardsSeen.value), maxCardsSeen.value))
const selectedProbability = computed(() => odds.value?.curve[seenIndex.value - 1] ?? 0)
const probabilityLabel = computed(
  () => `${(selectedProbability.value * 100).toFixed(1).replace('.0', '')}%`,
)

// ---------- The resting summary ----------

const expanded = ref(false)
const detailsId = useId()

/** Rendered by both the summary line and the expanded tile, so one field can never print as
 * two different strings. `—` is the answer for a deck with no nonlands to average. */
const manaValueLabel = computed(() => stats.value?.average_mana_value?.toFixed(2) ?? '—')

/** Lands as a share of the deck. The denominator is printed beside it rather than left to be
 * inferred: `total_copies` is the deck **proper** — everything outside a maybeboard, so a
 * sideboard and a command zone are both in it — which is the same figure the page header
 * prints as "N cards", but *not* the 60 or 99 a player has in mind when they say "38%". A
 * visible "of 100" is what stops the percentage being read against the wrong deck. */
const landShare = computed(() => {
  const total = stats.value?.total_copies ?? 0
  return total > 0 ? Math.round(((stats.value?.land_copies ?? 0) / total) * 100) : 0
})

/** A curve of nothing but zeroes — a deck that is still only lands, or only sections the
 * server found no mana values in — is worded rather than drawn: eight empty tracks under a
 * 0–7+ axis read as a broken widget, not as "no spells yet". */
const hasCurve = computed(() => stats.value?.mana_curve.some((item) => item.count > 0) ?? false)

/** The one library-derived line in the summary, and it only appears once the viewer has
 * picked a card themselves — the server's default pick is whatever the deck holds most of,
 * which on a Commander deck is a basic land, and headlining "Forest · 100%" would be noise
 * dressed as an answer. Because the pool behind it is chosen by controls the collapsed panel
 * hides, the line spells its own library size out rather than leaving a bare percentage. */
const restingOdds = computed(() => (activeCard.value ? odds.value : null))
</script>

<template>
  <!-- One card, three bodies. The header — name, cue and disclosure — is shared across all
    three so the panel keeps its shape from the first paint: the numbers land in reserved
    space instead of appearing out of nowhere and pushing the deck down the page, and the
    "Details" control doesn't pop into the header's right edge when the response arrives. -->
  <Card
    v-if="pending || failed || (stats && stats.total_copies > 0)"
    class="mb-6"
    :aria-busy="pending || updating || undefined"
  >
    <CardHeader class="flex flex-row items-center justify-between gap-3 space-y-0">
      <CardTitle class="text-base">Deck analytics</CardTitle>
      <div class="flex shrink-0 items-center gap-3">
        <!-- A region that is always in the DOM and swaps its contents, rather than one
          created alongside its message: a live region born with its text in the same tick is
          unreliably announced, which would make the cue useless to the readers who need it
          most. -->
        <span class="text-muted-foreground text-xs" aria-live="polite">
          <UpdatingCue v-if="pending" label="Crunching numbers…" />
          <UpdatingCue v-else-if="updating" label="Recalculating…" />
        </span>
        <!-- `aria-label` because `DeckBracket` puts a second button reading exactly
          "Details" a few rows up this page; the visible word stays inside the accessible
          name, so pointing at it by voice still works. -->
        <button
          v-if="!failed"
          type="button"
          class="text-muted-foreground hover:text-foreground focus-visible:ring-ring/50 flex shrink-0 items-center gap-1 rounded-sm text-xs font-medium outline-none focus-visible:ring-3 disabled:opacity-50"
          aria-label="Details for deck analytics"
          :aria-expanded="expanded"
          :aria-controls="expanded ? detailsId : undefined"
          :disabled="pending"
          @click="expanded = !expanded"
        >
          Details
          <ChevronDown
            class="size-3.5 transition-transform"
            :class="expanded ? 'rotate-180' : ''"
            aria-hidden="true"
          />
        </button>
      </div>
    </CardHeader>

    <CardContent v-if="failed">
      <p class="text-destructive text-sm">These numbers couldn't be worked out. Please retry.</p>
    </CardContent>

    <!-- The resting shape, so the first response fills a space the panel already occupies. -->
    <CardContent v-else-if="pending" class="space-y-4">
      <div class="flex flex-wrap items-center gap-x-4 gap-y-1.5">
        <Skeleton class="h-4 w-28" />
        <Skeleton class="h-4 w-32" />
        <Skeleton class="h-4 w-24" />
      </div>
      <div>
        <Skeleton class="mb-1.5 h-3 w-36" />
        <Skeleton class="h-9 w-full rounded-sm" />
      </div>
    </CardContent>

    <CardContent v-else-if="stats" class="space-y-4">
      <!-- The resting summary: what this deck *is*, in two rows. -->
      <div class="flex flex-wrap items-center gap-x-5 gap-y-1.5 text-sm">
        <span :title="`${stats.land_copies} of the deck's ${stats.total_copies} cards are lands`">
          <span class="text-muted-foreground">Lands</span>
          <span class="ml-1.5 font-semibold tabular-nums">{{ stats.land_copies }}</span>
          <span class="text-muted-foreground tabular-nums">
            · {{ landShare }}% of {{ stats.total_copies }}</span
          >
        </span>
        <span>
          <span class="text-muted-foreground">Avg mana value</span>
          <span class="ml-1.5 font-semibold tabular-nums">{{ manaValueLabel }}</span>
        </span>
        <!-- Colour weights, not just which colours: how heavily a deck leans on each is the
          manabase question, and the pips cost the same room a bare identity would. The server
          only sends colours the deck actually plays. Own line on a phone (`w-full`) so the
          wrap never falls in the middle of the list. -->
        <ul
          v-if="stats.colors.length"
          class="flex w-full flex-wrap items-center gap-x-3 gap-y-1 sm:w-auto"
        >
          <li
            v-for="item in stats.colors"
            :key="item.key"
            class="inline-flex items-center gap-1"
            :title="`${item.label}: ${item.count} copies`"
          >
            <ManaSymbols :text="`{${item.key}}`" class="leading-none" aria-hidden="true" />
            <span class="sr-only">{{ item.label }}:</span>
            <span class="tabular-nums">{{ item.count }}</span>
          </li>
        </ul>
      </div>

      <DeckStatBars
        v-if="hasCurve"
        class="max-w-xs"
        title="Mana curve (nonlands)"
        :items="stats.mana_curve"
        layout="columns"
      />
      <p v-else class="text-muted-foreground text-xs">No nonland spells yet.</p>

      <p v-if="restingOdds" class="text-muted-foreground text-xs">
        <span class="text-foreground font-medium">{{ restingOdds.name }}</span> ·
        <span class="text-foreground font-semibold tabular-nums">{{ probabilityLabel }}</span>
        to see one in {{ seenIndex }} cards from a
        <span class="tabular-nums">{{ restingOdds.library_size }}</span
        >-card library
      </p>

      <!-- Everything the summary above is a summary of. The mana curve and the colours are
        deliberately drawn twice while open — here with their exact counts, above as the shape
        — because they are one set of numbers rendered two ways, and dropping either reading
        is what would let the collapsed and expanded panels disagree. -->
      <div v-if="expanded" :id="detailsId" class="space-y-6 border-t pt-5">
        <div class="grid grid-cols-2 gap-3 sm:grid-cols-4">
          <div class="bg-muted/50 rounded-md p-3">
            <p class="text-muted-foreground text-xs">Copies</p>
            <p class="mt-1 text-xl font-semibold tabular-nums">{{ stats.total_copies }}</p>
          </div>
          <div class="bg-muted/50 rounded-md p-3">
            <p class="text-muted-foreground text-xs">Unique printings</p>
            <p class="mt-1 text-xl font-semibold tabular-nums">{{ stats.unique_cards }}</p>
          </div>
          <div class="bg-muted/50 rounded-md p-3">
            <p class="text-muted-foreground text-xs">Average mana value</p>
            <p class="mt-1 text-xl font-semibold tabular-nums">{{ manaValueLabel }}</p>
          </div>
          <div class="bg-muted/50 rounded-md p-3">
            <p class="text-muted-foreground text-xs">Lands</p>
            <p class="mt-1 text-xl font-semibold tabular-nums">{{ stats.land_copies }}</p>
          </div>
        </div>

        <div class="grid gap-6 md:grid-cols-3">
          <DeckStatBars title="Mana curve (nonlands)" :items="stats.mana_curve" />
          <DeckStatBars title="Colour identity" :items="stats.colors" />
          <DeckStatBars title="Card types" :items="stats.card_types" />
        </div>

        <section class="border-t pt-5">
          <h3 class="text-sm font-semibold">Draw probability</h3>
          <p class="text-muted-foreground mt-1 text-xs">
            Chance of seeing at least one copy without replacement.
          </p>
          <fieldset v-if="sections.length" class="mt-3">
            <legend class="flex w-full items-center justify-between gap-2 text-xs font-medium">
              <span>Library sections</span>
              <span class="flex items-center gap-2">
                <button
                  type="button"
                  class="text-primary font-medium hover:underline disabled:opacity-50"
                  :disabled="allSectionsSelected"
                  @click="selectAllSections"
                >
                  Select all
                </button>
                <span class="text-muted-foreground" aria-hidden="true">·</span>
                <button
                  type="button"
                  class="text-primary font-medium hover:underline disabled:opacity-50"
                  :disabled="noSectionsSelected"
                  @click="deselectAllSections"
                >
                  Deselect all
                </button>
              </span>
            </legend>
            <div class="mt-1.5 flex flex-wrap gap-x-4 gap-y-1.5">
              <label
                v-for="section in sections"
                :key="section.id"
                class="flex items-center gap-1.5 text-xs"
              >
                <input
                  v-model="drawSectionIds"
                  type="checkbox"
                  :value="section.id"
                  class="accent-primary size-3.5 rounded border"
                />
                {{ section.name }}
                <span
                  v-if="section.is_maybeboard"
                  class="text-muted-foreground text-[0.65rem] tracking-wide uppercase"
                  >maybe</span
                >
              </label>
            </div>
            <!-- The library size belongs to the response, not the checkboxes: while the next
            one is in flight it describes the *previous* selection, so say that rather than
            print a count that contradicts the boxes the viewer just ticked. -->
            <p class="text-muted-foreground mt-1.5 text-xs" aria-live="polite">
              <template v-if="updating"><UpdatingCue /></template>
              <template v-else>
                {{ library?.total_copies ?? 0 }} cards from {{ drawSectionIds.length }} selected
                {{ drawSectionIds.length === 1 ? 'section' : 'sections' }}.
              </template>
            </p>
          </fieldset>
          <p v-if="!odds" class="text-muted-foreground mt-4 text-sm">
            Select at least one section containing cards to calculate draw odds.
          </p>
          <div
            v-else
            class="mt-3 grid gap-4 sm:grid-cols-[minmax(0,1fr)_minmax(12rem,1fr)_auto] sm:items-end"
          >
            <label class="space-y-1.5 text-sm">
              <span class="block text-xs font-medium">Card</span>
              <Select v-model="selectedCard">
                <SelectTrigger class="w-full" aria-label="Card for draw probability">
                  <SelectValue placeholder="Choose a card" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem
                    v-for="item in library?.card_odds ?? []"
                    :key="item.name"
                    :value="item.name"
                  >
                    {{ item.name }} ({{ item.copies }})
                  </SelectItem>
                </SelectContent>
              </Select>
            </label>
            <label class="space-y-1.5 text-sm">
              <span class="flex justify-between gap-2 text-xs font-medium">
                Cards seen <span class="tabular-nums">{{ seenIndex }}</span>
              </span>
              <input
                :value="seenIndex"
                type="range"
                min="1"
                :max="maxCardsSeen"
                class="accent-primary h-9 w-full"
                @input="cardsSeen = Number(($event.target as HTMLInputElement).value)"
              />
            </label>
            <!-- Dimmed rather than blanked while the next answer is in flight: the previous
            percentage is the honest thing to leave on screen (the panel is built on
            keepPreviousData), and swapping a two-line tile for a spinner would reflow the
            row every time the card select changes. -->
            <div
              class="bg-primary/10 min-w-24 rounded-md px-4 py-2 text-center transition-opacity"
              :class="{ 'opacity-40': updating }"
            >
              <p class="text-primary text-2xl font-semibold tabular-nums">{{ probabilityLabel }}</p>
              <p class="text-muted-foreground text-xs">at least one</p>
            </div>
          </div>
        </section>
      </div>
    </CardContent>
  </Card>
</template>
