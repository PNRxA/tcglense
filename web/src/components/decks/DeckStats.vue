<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import DeckStatBars from '@/components/decks/DeckStatBars.vue'
import { useDeckStatsQuery, usePublicDeckStatsQuery } from '@/composables/useDeckAnalysis'
import type { DeckSection } from '@/lib/api'

// The deck page's analytics panel. Everything it shows is computed server-side (issue
// #596) — `GET /api/decks/{game}/{deck_id}/stats`, or its public mirror — so a CLI can ask
// the same question and get the same numbers. What's left here is the two controls that
// pick *which* question: which sections count as the shuffled library, and which card the
// draw odds are for.
//
// `handle` puts the panel in public mode (a deck someone shared), exactly as
// `ProductHoldingSection` does: the surface is fixed per mount, so the query hook is
// selected once below.
const props = defineProps<{
  game: string
  deckId: number
  sections: DeckSection[]
  handle?: string
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
const deckId = toRef(props, 'deckId')
const handle = computed(() => props.handle ?? '')
const statsQuery = props.handle
  ? usePublicDeckStatsQuery(handle, deckId, params)
  : useDeckStatsQuery(game, deckId, params)
const analytics = computed(() => statsQuery.data.value)
const stats = computed(() => analytics.value?.deck)
const library = computed(() => analytics.value?.library)
const odds = computed(() => analytics.value?.odds ?? null)

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
</script>

<template>
  <Card v-if="stats && stats.total_copies > 0" class="mb-6">
    <CardHeader>
      <CardTitle class="text-base">Deck analytics</CardTitle>
    </CardHeader>
    <CardContent class="space-y-6">
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
          <p class="mt-1 text-xl font-semibold tabular-nums">
            {{ stats.average_mana_value?.toFixed(2) ?? '—' }}
          </p>
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
          <p class="text-muted-foreground mt-1.5 text-xs">
            {{ library?.total_copies ?? 0 }} cards from {{ drawSectionIds.length }} selected
            {{ drawSectionIds.length === 1 ? 'section' : 'sections' }}.
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
          <div class="bg-primary/10 min-w-24 rounded-md px-4 py-2 text-center">
            <p class="text-primary text-2xl font-semibold tabular-nums">{{ probabilityLabel }}</p>
            <p class="text-muted-foreground text-xs">at least one</p>
          </div>
        </div>
      </section>
    </CardContent>
  </Card>
</template>
