<script setup lang="ts">
import { computed, ref, toRef, useId, type Ref } from 'vue'
import { ChevronDown } from '@lucide/vue'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import ManaSymbols from '@/components/cards/ManaSymbols.vue'
import UpdatingCue from '@/components/cards/UpdatingCue.vue'
import {
  useDeckBracketQuery,
  useDeckManaQuery,
  useDeckPricingQuery,
  useDeckStatsQuery,
  useDeckSuggestionsQuery,
  usePreconBracketQuery,
  usePreconManaQuery,
  usePreconStatsQuery,
  usePublicDeckBracketQuery,
  usePublicDeckManaQuery,
  usePublicDeckPricingQuery,
  usePublicDeckStatsQuery,
} from '@/composables/useDeckAnalysis'
import { useCurrency } from '@/composables/useCurrency'
import type { DeckLegality, DeckStatsParams } from '@/lib/api'
import {
  bracketGlance,
  landsGlance,
  legalityGlance,
  manaGlance,
  manaValueGlance,
  savingGlance,
  suggestionsGlance,
  type Glance,
  type GlanceTone,
} from '@/lib/deckOverview'
import { normalizeFormatKey } from '@/lib/legality'
import { useDeckViewStore } from '@/stores/deckView'

// The **overview** of a deck page: the one collapsible that stands between the header and
// the card list. Collapsed, it is a single card carrying one chip per analysis — the
// legality verdict, the bracket, the land count and average mana value, the mana base's
// verdict, the saving at cheapest printings, and (for the owner) how many cards they
// already own that fit — each read straight off the response the corresponding panel
// renders (`lib/deckOverview.ts`), so the strip is the summary *of* the panels, exactly as
// each panel's own resting rows are the summary of its detail. Expanded, the slot mounts
// the whole stack the page used to show unconditionally: the legality banner, the bracket,
// analytics, roles, mana base, pricing, suggestions, the test hand and the comparison.
//
// Every read it fires is one the panels fire on mount anyway, under the same query key, so
// opening the stack costs no second request and the collapsed page costs no more than the
// old one. `DeckRoles` is deliberately not summarised here: its resting row *is* the card
// list's role filter, and a count of roles is not a verdict.
//
// The disclosure is `DeckBracket`'s (rotating chevron, `aria-expanded`, body mounted only
// while open) but its state is **remembered** (`stores/deckView`), unlike the panels' own
// per-mount toggles: this is the layout of the page, not a question about one deck, and a
// reader who wants the whole picture shouldn't re-open it on every deck they visit. The
// strip stays on screen while open — the chips double as the table of contents for the
// stack below, and hiding them on expand would jump the layout.
//
// Addressing is fixed at mount and mutually exclusive, like every panel: a deck id (the
// owner's), a `handle` + deck id (a shared deck), or a `preconSlug` (a published list).
// Exactly one applies, and the hook for that mode is selected once.
const props = defineProps<{
  game: string
  deckId?: number
  handle?: string
  preconSlug?: string
  /** The deck's format label, for the bracket gate. `null` is a deck with no format. */
  format?: string | null
  /** The view's own legality read: the verdict, or `null` for an untracked format. */
  legality: DeckLegality | null
  legalityPending?: boolean
  /** Cards in the deck proper — an empty deck asks the server nothing. */
  totalCards: number
  /** What the expanded stack holds, named beside it by the view that owns the slot. */
  description: string
}>()

const deckView = useDeckViewStore()
const expanded = computed(() => deckView.overviewExpanded)
const bodyId = useId()
function toggle() {
  deckView.setOverviewExpanded(!expanded.value)
}

const game = toRef(props, 'game')
const deckId = computed(() => props.deckId ?? 0)
const handle = computed(() => props.handle ?? '')
const preconSlug = computed(() => props.preconSlug ?? '')
const hasCards = computed(() => props.totalCards > 0)
// The same gate `DeckBracket` applies: the ladder is Commander's alone, and every other
// format's answer is a guaranteed null that would still spend the per-user Analytics quota.
const bracketEnabled = computed(
  () =>
    hasCards.value &&
    (props.format === undefined || normalizeFormatKey(props.format) === 'commander'),
)
// `{}` hashes to the same key as the panel's untouched `{ sections: undefined, card:
// undefined }`, so the two reads share one cache entry.
const statsParams = ref<DeckStatsParams>({})

const statsQuery = props.preconSlug
  ? usePreconStatsQuery(game, preconSlug, statsParams, hasCards)
  : props.handle
    ? usePublicDeckStatsQuery(handle, deckId, statsParams, hasCards)
    : useDeckStatsQuery(game, deckId, statsParams, hasCards)
const bracketQuery = props.preconSlug
  ? usePreconBracketQuery(game, preconSlug, bracketEnabled)
  : props.handle
    ? usePublicDeckBracketQuery(handle, deckId, bracketEnabled)
    : useDeckBracketQuery(game, deckId, bracketEnabled)
const manaQuery = props.preconSlug
  ? usePreconManaQuery(game, preconSlug, hasCards)
  : props.handle
    ? usePublicDeckManaQuery(handle, deckId, hasCards)
    : useDeckManaQuery(game, deckId, hasCards)
// Pricing has no precon read, and suggestions read the caller's collection, so only the
// owner's page asks; a mode that can't ask never calls the hook at all.
const pricingQuery = props.preconSlug
  ? null
  : props.handle
    ? usePublicDeckPricingQuery(handle, deckId, hasCards)
    : useDeckPricingQuery(game, deckId, hasCards)
const suggestionsQuery =
  props.preconSlug || props.handle ? null : useDeckSuggestionsQuery(game, deckId, hasCards)

const money = useCurrency()

const chips = computed<Glance[]>(() => {
  const out: Glance[] = []
  if (props.legality) out.push(legalityGlance(props.legality))
  const estimate = bracketQuery.data.value?.data
  if (estimate) out.push(bracketGlance(estimate))
  const deck = statsQuery.data.value?.deck
  if (deck) {
    const lands = landsGlance(deck)
    if (lands) out.push(lands)
    const mv = manaValueGlance(deck)
    if (mv) out.push(mv)
  }
  const base = manaQuery.data.value
  if (base) {
    const mana = manaGlance(base)
    if (mana) out.push(mana)
  }
  const pricing = pricingQuery?.data.value
  if (pricing) {
    const saving = savingGlance(pricing, money.formatUsd)
    if (saving) out.push(saving)
  }
  const suggestions = suggestionsQuery?.data.value
  if (suggestions) {
    const fit = suggestionsGlance(suggestions)
    if (fit) out.push(fit)
  }
  return out
})

// One placeholder per read still on its first trip, so the strip draws its shape before
// the answers land instead of growing chip by chip and shoving the card list down.
const pendingCount = computed(() => {
  const queries: Array<{ isPending: Ref<boolean> }> = [statsQuery, manaQuery]
  if (bracketEnabled.value) queries.push(bracketQuery)
  if (pricingQuery) queries.push(pricingQuery)
  if (suggestionsQuery) queries.push(suggestionsQuery)
  return hasCards.value ? queries.filter((q) => q.isPending.value).length : 0
})
const anyFailed = computed(() =>
  [statsQuery, bracketQuery, manaQuery, pricingQuery, suggestionsQuery].some(
    (q) => q?.isLoadingError.value,
  ),
)

const TONE_CLASS: Record<GlanceTone, string> = {
  neutral: 'border',
  success: 'bg-success/15 text-success',
  warning: 'bg-warning/15 text-warning',
  destructive: 'bg-destructive/15 text-destructive',
}
</script>

<template>
  <div class="mb-6">
    <Card class="gap-3 py-4" :aria-busy="pendingCount > 0 || undefined">
      <CardHeader class="flex flex-row items-center justify-between gap-3 space-y-0 pb-0">
        <CardTitle class="text-base">Overview</CardTitle>
        <button
          type="button"
          class="text-muted-foreground hover:text-foreground focus-visible:ring-ring/50 flex shrink-0 items-center gap-1 rounded-sm text-xs font-medium outline-none focus-visible:ring-3"
          :aria-expanded="expanded"
          :aria-controls="expanded ? bodyId : undefined"
          @click="toggle"
        >
          {{ expanded ? 'Hide details' : 'Show details' }}
          <ChevronDown
            class="size-3.5 transition-transform"
            :class="expanded ? 'rotate-180' : ''"
            aria-hidden="true"
          />
        </button>
      </CardHeader>
      <CardContent class="space-y-2">
        <!-- The strip: one chip per answer, in the order the panels stack below. -->
        <ul class="flex flex-wrap items-center gap-1.5" aria-label="Deck at a glance">
          <li
            v-if="legalityPending"
            class="text-muted-foreground inline-flex items-center rounded-md border px-1.5 py-0.5 text-xs"
          >
            <UpdatingCue label="Checking legality…" />
          </li>
          <li
            v-for="chip in chips"
            :key="chip.key"
            class="inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-xs font-medium"
            :class="TONE_CLASS[chip.tone]"
            :title="chip.title"
          >
            {{ chip.label }}
            <ManaSymbols
              v-if="chip.pips?.length"
              :text="chip.pips.map((p) => `{${p}}`).join('')"
              class="leading-none"
            />
          </li>
          <li v-for="n in pendingCount" :key="`pending-${n}`" aria-hidden="true">
            <Skeleton class="h-5 w-32" />
          </li>
          <li
            v-if="anyFailed"
            class="text-muted-foreground inline-flex items-center rounded-md border px-1.5 py-0.5 text-xs"
          >
            Some numbers couldn't be worked out
          </li>
          <li
            v-if="!hasCards && !legalityPending && chips.length === 0"
            class="text-muted-foreground text-xs"
          >
            Add cards to see the deck’s numbers here.
          </li>
        </ul>
        <p v-if="!expanded" class="text-muted-foreground text-xs">{{ description }}</p>
      </CardContent>
    </Card>

    <!-- The stack, mounted only while open: every panel below fires its read on mount, and
      each shares its key with the read above, so nothing is asked twice. -->
    <div v-if="expanded" :id="bodyId" class="mt-6">
      <slot />
    </div>
  </div>
</template>
