<script setup lang="ts">
import { computed, ref, toRef, useId } from 'vue'
import { ChevronDown, Library, Plus } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import StaleNotice from '@/components/cards/StaleNotice.vue'
import UpdatingCue from '@/components/cards/UpdatingCue.vue'
import { useDeckSuggestionsQuery } from '@/composables/useDeckAnalysis'
import { useDeckCardAdder } from '@/composables/useDeckCardAdder'
import { useDetailModalLink } from '@/composables/useDetailModalLink'
import type { DeckCardEntry, DeckSection, DeckSuggestionCard } from '@/lib/api'
import { printingLabel } from '@/lib/deckPricing'
import { rankLabel, suggestionsSummary, TOP_SUGGESTED } from '@/lib/deckSuggestions'

// **From your collection** (issue #684): the cards you already own that this deck could
// play — legal in its format, inside its colour identity, not already in it — most popular
// first and grouped by the role each fills, with the deck's own count per role beside them
// ("Card draw · in deck 6 · you own 14 more"). Every claim is the server's
// (`GET /api/decks/{game}/{deck_id}/suggestions`): which cards, which filters applied, and the
// role each card fills, read through the same grammar the roles panel counts the deck with.
//
// It is honest about what it is. The rank is EDHREC's **global** popularity, not synergy
// with this commander, and the response's first caveat says so — it is printed under the
// list, always, never behind "Details". The header names both filters the server applied,
// and when the server applied none (an untracked format, a deck with no card to read a
// colour off) says that instead of implying a fit it didn't check.
//
// Rests **collapsed** like the pricing panel beside it: the most popular few as rows, each
// with an "Add" that files the card through the same engine the add-cards box uses
// (`useDeckCardAdder` — the same automatic-by-type filing, the same optimistic counts), so a
// suggestion added here lands exactly where a searched card would. Behind "Details" sit the
// role groups. Owner-only: the read is the caller's collection, so there is no public mode.
const props = defineProps<{
  game: string
  deckId: number
  sections: DeckSection[]
  cards: DeckCardEntry[]
}>()

const game = toRef(props, 'game')
const deckId = computed(() => props.deckId)
const query = useDeckSuggestionsQuery(game, deckId)
const suggestions = computed(() => query.data.value)
const pending = computed(() => query.isPending.value)
const updating = computed(() => query.isFetching.value && !pending.value)

const top = computed(() => suggestions.value?.top.slice(0, TOP_SUGGESTED) ?? [])
const labelByRole = computed(
  () => new Map((suggestions.value?.roles ?? []).map((group) => [group.role, group.label])),
)
function roleLabels(card: DeckSuggestionCard): string[] {
  return card.roles.map((role) => labelByRole.value.get(role) ?? role)
}
/** The rank caveat — the first the server sends — stays in view; the rest sit in Details. */
const rankCaveat = computed(() => suggestions.value?.caveats[0] ?? '')
const otherCaveats = computed(() => suggestions.value?.caveats.slice(1) ?? [])

const expanded = ref(false)
const detailsId = useId()
const { hrefFor, onActivate, warm } = useDetailModalLink()

const adder = useDeckCardAdder({
  game,
  deckId,
  sections: computed(() => props.sections),
  cards: computed(() => props.cards),
})
const addError = ref('')
async function add(card: DeckSuggestionCard) {
  addError.value = ''
  try {
    await adder.add(card.card)
  } catch {
    addError.value = `Couldn't add ${card.card.name}. Please retry.`
  }
}
function addLabel(card: DeckSuggestionCard): string {
  return adder.needsExplicitSection(card.card)
    ? `Choose a section before adding ${card.card.name}`
    : `Add ${card.card.name} to the deck`
}
</script>

<template>
  <Card class="mb-6 gap-3 py-4" :aria-busy="updating || undefined">
    <CardHeader class="pb-0">
      <div class="flex flex-wrap items-start justify-between gap-2">
        <div class="min-w-0">
          <CardTitle class="flex items-center gap-2 text-base">
            <Library class="size-4" aria-hidden="true" /> From your collection
          </CardTitle>
          <p class="text-muted-foreground mt-1 text-xs" aria-live="polite">
            <template v-if="updating"><UpdatingCue label="Checking your collection…" /></template>
            <template v-else-if="pending">Checking your collection…</template>
            <template v-else-if="suggestions">{{ suggestionsSummary(suggestions) }}</template>
          </p>
        </div>
        <div class="flex shrink-0 items-center gap-3">
          <label class="text-muted-foreground flex items-center gap-1.5 text-xs">
            Add to
            <select
              v-model="adder.target.value"
              class="border-input bg-background focus-visible:ring-ring rounded-md border px-2 py-1 text-xs outline-none focus-visible:ring-2"
              aria-label="Section suggested cards are added to"
            >
              <option value="auto">Automatic (by type)</option>
              <option v-for="s in sections" :key="s.id" :value="String(s.id)">{{ s.name }}</option>
            </select>
          </label>
          <!-- `aria-label` because this page carries several buttons reading "Details". -->
          <button
            type="button"
            class="text-muted-foreground hover:text-foreground flex items-center gap-1 text-xs font-medium"
            aria-label="Details for suggestions from your collection"
            :aria-expanded="expanded"
            :aria-controls="expanded ? detailsId : undefined"
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
      </div>
    </CardHeader>

    <CardContent class="space-y-3">
      <div v-if="pending" class="space-y-2">
        <Skeleton v-for="n in 3" :key="n" class="h-5 w-full rounded" />
      </div>

      <!-- `isLoadingError`, not `isError`: a hiccuped background refetch keeps the list and
        says so above it rather than replacing a good answer with this. -->
      <p v-else-if="query.isLoadingError.value" class="text-muted-foreground text-sm">
        Couldn't read your collection for this deck.
      </p>

      <template v-else-if="suggestions">
        <StaleNotice
          v-if="query.isRefetchError.value"
          label="Couldn't refresh — showing the suggestions as they last loaded."
        />
        <p v-if="addError" class="text-destructive text-sm" aria-live="polite">{{ addError }}</p>

        <!-- The resting rows: the most popular few, each with its add. -->
        <ul v-if="top.length" class="divide-y" data-testid="top-suggestions">
          <li
            v-for="card in top"
            :key="card.card.id"
            class="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-3 gap-y-0.5 py-1.5 text-sm"
            data-testid="suggestion-row"
          >
            <div class="min-w-0">
              <a
                :href="hrefFor('card', game, card.card.id)"
                class="font-medium hover:underline"
                :title="card.card.name"
                @click="onActivate($event, 'card', game, card.card.id)"
                @pointerenter="warm('card')"
                @focusin="warm('card')"
                >{{ card.card.name }}</a
              >
              <span class="text-muted-foreground ml-2 text-xs tabular-nums" data-testid="rank">{{
                rankLabel(card)
              }}</span>
              <p class="text-muted-foreground flex flex-wrap items-center gap-x-2 text-xs">
                <span class="truncate">{{ printingLabel(card.card) }}</span>
                <span class="tabular-nums" data-testid="owned"
                  >×{{ card.owned }} owned<template v-if="adder.inTargetCount(card.card) > 0">
                    · {{ adder.inTargetCount(card.card) }} in section</template
                  ></span
                >
                <span
                  v-for="label in roleLabels(card)"
                  :key="label"
                  class="bg-muted rounded px-1"
                  >{{ label }}</span
                >
              </p>
            </div>
            <Button
              variant="outline"
              size="sm"
              class="h-7 px-2 text-xs"
              :aria-label="addLabel(card)"
              :disabled="adder.needsExplicitSection(card.card) || adder.isPending(card.card)"
              @click="add(card)"
            >
              <Plus class="size-3.5" aria-hidden="true" />
              {{ adder.isPending(card.card) ? 'Adding…' : 'Add' }}
            </Button>
          </li>
        </ul>
        <p v-else class="text-muted-foreground text-sm">
          Nothing you own fits this deck yet — every ranked card in your collection that would is
          either already in it, off-colour, or not legal in its format.
        </p>

        <p class="text-muted-foreground text-xs" data-testid="rank-caveat">{{ rankCaveat }}</p>

        <!-- Every role: what the deck holds, what you own that would fill it, and the cards. -->
        <div v-if="expanded" :id="detailsId" class="space-y-3 border-t pt-3">
          <div class="grid gap-3 sm:grid-cols-2">
            <section
              v-for="group in suggestions.roles"
              :key="group.role"
              class="rounded-md border p-3"
              data-testid="role-group"
            >
              <h3 class="flex flex-wrap items-baseline justify-between gap-2 text-sm font-medium">
                {{ group.label }}
                <span class="text-muted-foreground text-xs tabular-nums">
                  in deck {{ group.in_deck }} · you own {{ group.count }} more
                </span>
              </h3>
              <p class="text-muted-foreground mt-1 text-xs">{{ group.description }}</p>
              <ul v-if="group.cards.length" class="mt-2 divide-y">
                <li
                  v-for="card in group.cards"
                  :key="card.card.id"
                  class="flex items-center justify-between gap-2 py-1 text-sm"
                >
                  <span class="min-w-0">
                    <a
                      :href="hrefFor('card', game, card.card.id)"
                      class="hover:underline"
                      @click="onActivate($event, 'card', game, card.card.id)"
                      @pointerenter="warm('card')"
                      @focusin="warm('card')"
                      >{{ card.card.name }}</a
                    >
                    <span class="text-muted-foreground ml-2 text-xs tabular-nums">{{
                      rankLabel(card)
                    }}</span>
                  </span>
                  <Button
                    variant="ghost"
                    size="sm"
                    class="h-7 px-2 text-xs"
                    :aria-label="addLabel(card)"
                    :disabled="adder.needsExplicitSection(card.card) || adder.isPending(card.card)"
                    @click="add(card)"
                  >
                    <Plus class="size-3.5" aria-hidden="true" />
                    {{ adder.isPending(card.card) ? 'Adding…' : 'Add' }}
                  </Button>
                </li>
              </ul>
              <p v-if="group.count > group.cards.length" class="text-muted-foreground mt-1 text-xs">
                …and {{ group.count - group.cards.length }} more
              </p>
            </section>
          </div>
          <p class="text-muted-foreground text-xs">
            <span class="tabular-nums">{{ suggestions.unclassified_count }}</span> of the
            <span class="tabular-nums">{{ suggestions.scanned_count }}</span> cards classified fill
            no role — most creatures and every land — so the groups aren't a partition of what fits.
          </p>
          <ul v-if="otherCaveats.length" class="text-muted-foreground space-y-1 text-xs">
            <li v-for="caveat in otherCaveats" :key="caveat">{{ caveat }}</li>
          </ul>
        </div>
      </template>
    </CardContent>
  </Card>
</template>
