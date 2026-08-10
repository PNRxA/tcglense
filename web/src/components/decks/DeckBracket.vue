<script setup lang="ts">
import { computed, ref, toRef, useId } from 'vue'
import { ChevronDown } from '@lucide/vue'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import UpdatingCue from '@/components/cards/UpdatingCue.vue'
import { useDeckBracketQuery, usePublicDeckBracketQuery } from '@/composables/useDeckAnalysis'
import { useDetailModalLink } from '@/composables/useDetailModalLink'
import { bracketBar, bracketTone, ESTIMATABLE_BRACKETS } from '@/lib/bracket'

// The deck page's **estimated Commander bracket**: where the deck sits on Wizards' 1–5
// ladder, so two players can compare decks before a game instead of finding out on turn
// four. Everything shown is the server's (`GET /api/decks/{game}/{deck_id}/bracket`, or its
// public mirror), including the ladder's own labels — a CLI asking the same question gets
// the same panel's worth of answer.
//
// The estimate is a **floor**, and the panel is built to say so rather than hide it: the
// reasons say what moved it, every counted card is listed so the number can be audited, and
// the caveats name what a decklist simply cannot show (combos, whether extra turns get
// chained, what the deck was built for). A power rating nobody can check is a power rating
// nobody believes.
//
// That honesty is a lot of text for something most visits only glance at, so it is
// **collapsed by default**: the rung, the ladder, and one chip per category — including the
// zeroes, which are half the point — always show, and the evidence behind them is one click
// away. The chips are the summary *of* the detail rather than a second set of numbers, so a
// collapsed panel can't disagree with an expanded one.
//
// The disclosure follows `components/shared/CollapsibleSection.vue`'s idiom (rotating
// chevron, `aria-expanded`, body mounted only while open) rather than reusing it: that
// component hides its whole card behind its header, and here the headline has to stay
// visible for the panel to be worth glancing at at all.
//
// `handle` puts the panel in public mode, exactly as `DeckStats` does: the surface is fixed
// per mount, so the query hook is selected once below.
const props = defineProps<{
  game: string
  deckId: number
  handle?: string
}>()

const game = toRef(props, 'game')
const deckId = toRef(props, 'deckId')
const handle = computed(() => props.handle ?? '')
const bracketQuery = props.handle
  ? usePublicDeckBracketQuery(handle, deckId)
  : useDeckBracketQuery(game, deckId)

// `null` is the answer for every deck that isn't a Commander deck — the ladder is defined
// for that format alone — so the panel renders nothing at all rather than an empty card.
const estimate = computed(() => bracketQuery.data.value?.data ?? null)
const pending = computed(() => bracketQuery.isPending.value)
const updating = computed(() => bracketQuery.isFetching.value && !pending.value)

const tone = computed(() => bracketTone(estimate.value?.bracket ?? 0))

const expanded = ref(false)
const detailsId = useId()

/** A category's count badge: toned when it decided the bracket, muted when the deck holds
 * none — the same three states in the summary chips and in the detail cards, so the two
 * readings of one number can never look like different claims. */
function countTone(count: number, decisive: boolean): string {
  if (count === 0) return 'text-muted-foreground'
  return decisive ? tone.value : 'bg-muted'
}

const { hrefFor, onActivate, warm } = useDetailModalLink()
</script>

<template>
  <!-- A one-line cue rather than a skeleton card: the answer is `null` for most decks, and
    reserving a panel's worth of space for something that usually never appears would shove
    the deck down the page on every load. -->
  <p v-if="pending" class="text-muted-foreground mb-4 text-sm">
    <UpdatingCue label="Estimating bracket…" />
  </p>

  <Card v-else-if="estimate" class="mb-6" :aria-busy="updating || undefined">
    <CardHeader class="flex flex-row items-start justify-between gap-3 space-y-0">
      <div class="min-w-0">
        <CardTitle class="text-base">Estimated bracket</CardTitle>
        <p class="text-muted-foreground mt-1 text-xs">
          The lowest bracket this {{ estimate.format_label }} deck's cards don't rule out.
        </p>
      </div>
      <span v-if="updating" class="text-muted-foreground shrink-0 text-xs" aria-live="polite">
        <UpdatingCue label="Re-estimating…" />
      </span>
    </CardHeader>

    <CardContent class="space-y-4">
      <div class="flex items-center gap-3">
        <span
          class="flex size-12 shrink-0 items-center justify-center rounded-lg text-2xl font-semibold tabular-nums"
          :class="tone"
          >{{ estimate.bracket }}</span
        >
        <div class="min-w-0">
          <p class="font-semibold">{{ estimate.label }}</p>
          <p class="text-muted-foreground text-sm">{{ estimate.description }}</p>
        </div>
      </div>

      <!-- The whole ladder, so the estimate is read in context. Brackets 1 and 5 are
        greyed: they weren't ruled out, they're just not something a list can establish. -->
      <ol class="grid grid-cols-5 gap-1.5">
        <li v-for="rung in estimate.ladder" :key="rung.bracket" :title="rung.description">
          <div
            class="h-1.5 rounded-full"
            :class="rung.bracket === estimate.bracket ? bracketBar(rung.bracket) : 'bg-muted'"
          />
          <p
            class="mt-1 truncate text-[0.7rem]"
            :class="
              rung.bracket === estimate.bracket
                ? 'font-semibold'
                : ESTIMATABLE_BRACKETS.includes(rung.bracket)
                  ? 'text-muted-foreground'
                  : 'text-muted-foreground/60'
            "
          >
            {{ rung.bracket }} · {{ rung.label }}
          </p>
        </li>
      </ol>

      <!-- The condensed summary: what was counted, without the evidence. Every category
        appears, so a zero is something the deck says rather than something the reader has
        to infer from an absence. -->
      <ul class="flex flex-wrap items-center gap-x-2 gap-y-1.5">
        <li
          v-for="category in estimate.categories"
          :key="category.signal"
          class="inline-flex items-center gap-1.5 rounded-md border px-2 py-1 text-xs"
        >
          <span :class="category.count === 0 ? 'text-muted-foreground' : ''">{{
            category.label
          }}</span>
          <span
            class="inline-flex items-center rounded px-1 font-semibold tabular-nums"
            :class="countTone(category.count, category.decisive)"
            >{{ category.count }}</span
          >
        </li>
      </ul>

      <button
        type="button"
        class="text-muted-foreground hover:text-foreground group flex items-center gap-1.5 text-xs font-medium"
        :aria-expanded="expanded"
        :aria-controls="detailsId"
        @click="expanded = !expanded"
      >
        {{ expanded ? 'Hide details' : 'Show details' }}
        <ChevronDown
          class="size-3.5 transition-transform"
          :class="expanded ? 'rotate-180' : ''"
          aria-hidden="true"
        />
      </button>

      <div v-if="expanded" :id="detailsId" class="space-y-5 border-t pt-4">
        <ul class="space-y-1 text-sm">
          <li v-for="(reason, index) in estimate.reasons" :key="index" class="flex gap-2">
            <span class="text-muted-foreground" aria-hidden="true">•</span>
            <span class="min-w-0">{{ reason }}</span>
          </li>
        </ul>

        <!-- Exactly which cards were counted — the panel's whole claim to being checkable. -->
        <div class="grid gap-3 sm:grid-cols-2">
          <section
            v-for="category in estimate.categories"
            :key="category.signal"
            class="rounded-md border p-3"
          >
            <div class="flex items-center justify-between gap-2">
              <h3 class="text-sm font-medium">{{ category.label }}</h3>
              <span
                class="inline-flex shrink-0 items-center rounded-md px-1.5 py-0.5 text-xs font-semibold tabular-nums"
                :class="countTone(category.count, category.decisive)"
                >{{ category.count }}</span
              >
            </div>
            <p class="text-muted-foreground mt-1 text-xs">{{ category.description }}</p>
            <ul v-if="category.cards.length" class="mt-2 flex flex-wrap gap-1.5">
              <li v-for="card in category.cards" :key="card.card_id">
                <a
                  :href="hrefFor('card', game, card.card_id)"
                  class="inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-xs hover:underline"
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
            <p
              v-if="category.count > category.cards.length"
              class="text-muted-foreground mt-1.5 text-xs"
            >
              …and {{ category.count - category.cards.length }} more
            </p>
          </section>
        </div>

        <section class="border-t pt-4">
          <h3 class="text-sm font-semibold">What this can't see</h3>
          <ul class="text-muted-foreground mt-1.5 space-y-1 text-xs">
            <li v-for="(caveat, index) in estimate.caveats" :key="index">{{ caveat }}</li>
          </ul>
        </section>
      </div>
    </CardContent>
  </Card>
</template>
