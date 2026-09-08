<script setup lang="ts">
import { computed, ref, toRef, useId } from 'vue'
import { ChevronDown } from '@lucide/vue'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import ManaSymbols from '@/components/cards/ManaSymbols.vue'
import StaleNotice from '@/components/cards/StaleNotice.vue'
import UpdatingCue from '@/components/cards/UpdatingCue.vue'
import {
  useDeckManaQuery,
  usePreconManaQuery,
  usePublicDeckManaQuery,
} from '@/composables/useDeckAnalysis'
import { useDetailModalLink } from '@/composables/useDetailModalLink'
import type { DeckManaColor, DeckManaStatus } from '@/lib/api'

// The deck page's **mana base**: per colour, the pips the deck's spells ask for against the
// sources its library produces, and whether that's enough — Frank Karsten's source counts,
// applied to this list (issue #670). Every number is the server's
// (`GET /api/decks/{game}/{deck_id}/mana`, or its public and precon mirrors), so a CLI asking
// "do I have enough blue for UU on turn two" gets the same answer this panel draws.
//
// Like `DeckStats` and `DeckBracket` beside it, the panel **rests collapsed**: one row per
// colour — the pip, sources against the number needed, and the verdict as a chip — which is
// the whole question answered at a glance. Behind "Details" sit the evidence and the model:
// which spells set each colour's requirement (the number can be audited against the cards
// it was made of, as the bracket's can), which cards were counted as sources, and the caveats
// that say what Karsten's table assumes. The chip and the detail heading read the same
// `status` off the same response, so the two can never make different claims.
//
// Two silences are the response's, kept here: a hybrid pip is listed but never counted
// against a colour, and "not checked yet" (a catalog row that predates the produced-mana
// convention) is worded as syncing rather than letting a source count read as complete.
//
// `handle` / `preconSlug` pick the surface, chosen ONCE at mount exactly as the siblings do.
const props = defineProps<{
  game: string
  /** Addressing is fixed at mount and mutually exclusive: a deck id (the owner's own deck),
   *  a `handle` + deck id (a shared deck), or a `preconSlug` (a published catalog decklist,
   *  which has no deck row and no numeric id). Exactly one applies. */
  deckId?: number
  handle?: string
  preconSlug?: string
}>()

const game = toRef(props, 'game')
const deckId = computed(() => props.deckId ?? 0)
const handle = computed(() => props.handle ?? '')
const preconSlug = computed(() => props.preconSlug ?? '')

// Three addressing modes, selected ONCE at mount — see the props.
const manaQuery = props.preconSlug
  ? usePreconManaQuery(game, preconSlug)
  : props.handle
    ? usePublicDeckManaQuery(handle, deckId)
    : useDeckManaQuery(game, deckId)

const base = computed(() => manaQuery.data.value)
const colors = computed(() => base.value?.colors ?? [])
const pending = computed(() => manaQuery.isPending.value)
const failed = computed(() => manaQuery.isLoadingError.value)
const updating = computed(() => manaQuery.isFetching.value && !pending.value)

const expanded = ref(false)
const detailsId = useId()

/** The verdict chip's tone: the design system's status tokens, never a palette literal. A
 * colour nothing demands is muted — it's on the list for its sources, not for a verdict. */
const STATUS_TONE: Record<DeckManaStatus, string> = {
  enough: 'bg-success/15 text-success',
  short: 'bg-warning/15 text-warning',
  no_demand: 'bg-muted text-muted-foreground',
}

/** The chip's word. "Short 2" carries the number because that is the thing to fix. */
function statusLabel(color: DeckManaColor): string {
  if (color.status === 'enough') return 'Enough'
  if (color.status === 'short') return `Short ${color.shortfall}`
  return 'No demand'
}

/** "21 / 36" — sources against the number needed, or the sources alone when nothing sets a
 * requirement (a colour the deck produces but never pays). */
function sourcesLabel(color: DeckManaColor): string {
  return color.sources_needed === null
    ? `${color.sources}`
    : `${color.sources} / ${color.sources_needed}`
}

/** What a demand chip says on hover: the row it was judged as and what it needs. */
function demandTitle(color: DeckManaColor, needed: number, key: string, turn: number): string {
  return `${key} on turn ${turn}: ${needed} ${color.label.toLowerCase()} sources needed`
}

const { hrefFor, onActivate, warm } = useDetailModalLink()
</script>

<template>
  <!-- One card, three bodies, the header shared across them (see `DeckStats`): the rows land
    in space the panel already occupies rather than pushing the deck down the page. Hidden
    outright for a deck with nothing coloured in it — no costs, no sources — since a table of
    nothing is not an answer. -->
  <Card
    v-if="pending || failed || colors.length > 0"
    class="mb-6"
    :aria-busy="pending || updating || undefined"
  >
    <CardHeader class="flex flex-row items-center justify-between gap-3 space-y-0">
      <CardTitle class="text-base">Mana base</CardTitle>
      <div class="flex shrink-0 items-center gap-3">
        <span class="text-muted-foreground text-xs" aria-live="polite">
          <UpdatingCue v-if="pending" label="Counting sources…" />
          <UpdatingCue v-else-if="updating" label="Recounting…" />
        </span>
        <!-- `aria-label` because `DeckStats` and `DeckBracket` each put a button reading
          "Details" on this page; the visible word stays inside the accessible name. -->
        <button
          v-if="!failed"
          type="button"
          class="text-muted-foreground hover:text-foreground focus-visible:ring-ring/50 flex shrink-0 items-center gap-1 rounded-sm text-xs font-medium outline-none focus-visible:ring-3 disabled:opacity-50"
          aria-label="Details for mana base"
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
      <p class="text-destructive text-sm">The mana base couldn't be worked out. Please retry.</p>
    </CardContent>

    <!-- The resting shape: a few colour rows' worth of skeleton. -->
    <CardContent v-else-if="pending" class="space-y-2">
      <Skeleton v-for="n in 3" :key="n" class="h-5 w-64 max-w-full" />
    </CardContent>

    <CardContent v-else-if="base" class="space-y-4">
      <StaleNotice
        v-if="manaQuery.isRefetchError.value"
        label="Couldn't refresh — showing the mana base as it last loaded."
      />

      <!-- The resting rows: one per colour, the whole question at a glance. -->
      <ul class="space-y-1.5">
        <li
          v-for="color in colors"
          :key="color.color"
          class="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm"
          :title="color.verdict"
        >
          <span class="inline-flex w-24 items-center gap-1.5">
            <ManaSymbols :text="`{${color.color}}`" class="leading-none" aria-hidden="true" />
            <span>{{ color.label }}</span>
          </span>
          <span class="tabular-nums" :title="`${color.sources} sources in the library`">
            <span class="text-muted-foreground">Sources</span>
            <span class="ml-1.5 font-semibold">{{ sourcesLabel(color) }}</span>
          </span>
          <span
            class="inline-flex items-center rounded-md px-1.5 py-0.5 text-xs font-semibold tabular-nums"
            :class="STATUS_TONE[color.status]"
            >{{ statusLabel(color) }}</span
          >
          <span
            v-if="color.pips > 0"
            class="text-muted-foreground text-xs tabular-nums"
            :title="`${color.pips} ${color.label.toLowerCase()} pips across the deck's costs`"
          >
            {{ color.pips }} {{ color.pips === 1 ? 'pip' : 'pips' }}
          </span>
        </li>
      </ul>

      <!-- The model in one line, so a number above is never read against the wrong deck. -->
      <p class="text-muted-foreground text-xs">
        Judged as a {{ base.table_size }}-card deck ·
        <span class="tabular-nums">{{ base.land_count }}</span>
        {{ base.land_count === 1 ? 'land' : 'lands' }} in a
        <span class="tabular-nums">{{ base.library_size }}</span
        >-card library
        <template v-if="base.unchecked_count > 0">
          · {{ base.unchecked_count }} {{ base.unchecked_count === 1 ? 'card' : 'cards' }}
          {{ base.unchecked_count === 1 ? "hasn't" : "haven't" }} been checked for what
          {{ base.unchecked_count === 1 ? 'it produces' : 'they produce' }} yet — card data is still
          syncing, so source counts may be low
        </template>
      </p>

      <!-- The evidence: what set each number, and what the numbers assume. -->
      <div v-if="expanded" :id="detailsId" class="space-y-5 border-t pt-4">
        <section
          v-for="color in colors"
          :key="color.color"
          class="rounded-md border p-3"
          :aria-label="`${color.label} mana`"
        >
          <div class="flex flex-wrap items-center justify-between gap-2">
            <h3 class="inline-flex items-center gap-1.5 text-sm font-medium">
              <ManaSymbols :text="`{${color.color}}`" class="leading-none" aria-hidden="true" />
              {{ color.label }}
            </h3>
            <span
              class="inline-flex items-center rounded-md px-1.5 py-0.5 text-xs font-semibold tabular-nums"
              :class="STATUS_TONE[color.status]"
              >{{ statusLabel(color) }}</span
            >
          </div>
          <p class="text-muted-foreground mt-1 text-xs">{{ color.verdict }}</p>

          <!-- Which spells set the requirement, hungriest first. -->
          <template v-if="color.demand.length">
            <h4 class="text-muted-foreground mt-3 text-xs font-medium tracking-wide uppercase">
              Needs it · {{ color.demand_count }}
              {{ color.demand_count === 1 ? 'card' : 'cards' }}
            </h4>
            <ul class="mt-1.5 flex flex-wrap gap-1.5">
              <li v-for="card in color.demand" :key="card.card_id">
                <a
                  :href="hrefFor('card', game, card.card_id)"
                  class="inline-flex items-center gap-1.5 rounded-md border px-1.5 py-0.5 text-xs hover:underline"
                  :title="demandTitle(color, card.sources_needed, card.cost_key, card.turn)"
                  @click="onActivate($event, 'card', game, card.card_id)"
                  @pointerenter="warm('card')"
                  @focusin="warm('card')"
                >
                  {{ card.name }}
                  <ManaSymbols :text="card.mana_cost" class="leading-none" aria-hidden="true" />
                  <span v-if="card.quantity > 1" class="text-muted-foreground tabular-nums"
                    >×{{ card.quantity }}</span
                  >
                  <span class="text-muted-foreground tabular-nums"
                    >→ {{ card.sources_needed }}</span
                  >
                </a>
              </li>
            </ul>
            <p
              v-if="color.demand_count > color.demand.length"
              class="text-muted-foreground mt-1.5 text-xs"
            >
              …and {{ color.demand_count - color.demand.length }} more
            </p>
          </template>
          <!-- Only beside a hard requirement: with none, the verdict above already says it. -->
          <p
            v-if="color.hybrid_pips > 0 && color.pips > 0"
            class="text-muted-foreground mt-2 text-xs"
          >
            Plus {{ color.hybrid_pips }} hybrid {{ color.hybrid_pips === 1 ? 'pip' : 'pips' }} this
            colour could pay — not counted against it.
          </p>

          <!-- Which cards were counted as sources: lands first, the split stated. -->
          <h4 class="text-muted-foreground mt-3 text-xs font-medium tracking-wide uppercase">
            Sources · {{ color.land_sources }} {{ color.land_sources === 1 ? 'land' : 'lands' }}
            <template v-if="color.nonland_sources > 0">
              + {{ color.nonland_sources }} nonland</template
            >
          </h4>
          <ul v-if="color.source_cards.length" class="mt-1.5 flex flex-wrap gap-1.5">
            <li v-for="card in color.source_cards" :key="card.card_id">
              <a
                :href="hrefFor('card', game, card.card_id)"
                class="inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-xs hover:underline"
                :class="card.land ? '' : 'border-dashed'"
                :title="card.land ? 'Land' : 'Nonland source'"
                @click="onActivate($event, 'card', game, card.card_id)"
                @pointerenter="warm('card')"
                @focusin="warm('card')"
              >
                {{ card.name }}
                <span v-if="card.quantity > 1" class="text-muted-foreground tabular-nums"
                  >×{{ card.quantity }}</span
                >
              </a>
            </li>
          </ul>
          <p v-else class="text-muted-foreground mt-1.5 text-xs">
            Nothing in the library produces {{ color.label.toLowerCase() }}.
          </p>
          <p
            v-if="color.source_count > color.source_cards.length"
            class="text-muted-foreground mt-1.5 text-xs"
          >
            …and {{ color.source_count - color.source_cards.length }} more
          </p>
        </section>

        <section class="border-t pt-4">
          <h3 class="text-sm font-semibold">What this assumes</h3>
          <ul class="text-muted-foreground mt-1.5 space-y-1 text-xs">
            <li v-for="(caveat, index) in base.caveats" :key="index">{{ caveat }}</li>
          </ul>
          <p class="text-muted-foreground mt-2 text-xs">Source: {{ base.source }}</p>
        </section>
      </div>
    </CardContent>
  </Card>
</template>
