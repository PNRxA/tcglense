<script setup lang="ts">
import { computed, ref, toRef, useId } from 'vue'
import { ArrowRightLeft, ChevronDown, Coins, Sparkles } from '@lucide/vue'
import { Button, buttonVariants } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from '@/components/ui/dialog'
import { Skeleton } from '@/components/ui/skeleton'
import StaleNotice from '@/components/cards/StaleNotice.vue'
import UpdatingCue from '@/components/cards/UpdatingCue.vue'
import DeckPrintingDialog from '@/components/decks/DeckPrintingDialog.vue'
import { useCurrency } from '@/composables/useCurrency'
import { useDeckPricingQuery, usePublicDeckPricingQuery } from '@/composables/useDeckAnalysis'
import { useChangeDeckCardPrintingsMutation } from '@/composables/useDecks'
import { useDetailModalLink } from '@/composables/useDetailModalLink'
import { ApiError, type DeckPricingLine } from '@/lib/api'
import { hasSaving, printingLabel, TOP_EXPENSIVE } from '@/lib/deckPricing'

// **Where the money is** (issue #672): the deck's value broken down per row, most expensive
// first, with the cheapest printing of each card and what swapping to it would save. Every
// number is the server's (`GET /api/decks/{game}/{deck_id}/pricing`, or its public mirror):
// the total is the very fold the header prints, so the panel can never say the deck is
// worth something the header doesn't.
//
// Like `DeckStats` and `DeckBracket` beside it, the panel **rests collapsed**: the whole
// table is a screen on a page whose subject is the card list. The resting rows are the
// three totals — held, at cheapest printings, and the saving between them — and the top few
// most expensive cards, read straight off the sorted response (the top-N is a prefix, so
// the summary is the summary *of* the detail). Behind "Details" sits every row.
//
// Two honesties come from the response and are kept here: `null` is "unpriced" and is
// rendered as an em dash, never as $0.00; and a row's saving is only ever stated when the
// server stated it (a held printing unpriced in a finish the row holds gets a cheapest but
// no saving, so no swap is offered for it either).
//
// The swaps are the deck's existing printing write, never a new one. A row's "Swap" opens
// the same `DeckPrintingDialog` the card control opens, with the cheapest printing suggested
// at the top of it; "Swap all" batches that same write across every row with a saving,
// after confirming, because a hundred printings changing is a big edit to undo by hand.
// `handle` puts the panel in public mode (a deck someone shared): the breakdown, no buttons.
const props = defineProps<{
  game: string
  deckId: number
  handle?: string
}>()

const game = toRef(props, 'game')
const deckId = computed(() => props.deckId)
const handle = computed(() => props.handle ?? '')
const readonly = computed(() => !!props.handle)
// Two addressing modes, selected ONCE at mount — see the props.
const pricingQuery = props.handle
  ? usePublicDeckPricingQuery(handle, deckId)
  : useDeckPricingQuery(game, deckId)

const pricing = computed(() => pricingQuery.data.value)
const lines = computed(() => pricing.value?.lines ?? [])
const top = computed(() => lines.value.filter((line) => line.price_usd).slice(0, TOP_EXPENSIVE))
const swappable = computed(() => lines.value.filter(hasSaving))
const pending = computed(() => pricingQuery.isPending.value)
const updating = computed(() => pricingQuery.isFetching.value && !pending.value)

const money = useCurrency()
/** A stored USD string in the viewer's currency, or an em dash for an unpriced value. */
function usd(raw: string | null | undefined): string {
  return money.formatUsd(raw) ?? '—'
}

const expanded = ref(false)
const detailsId = useId()
const { hrefFor, onActivate, warm } = useDetailModalLink()

// One dialog for the whole table, pointed at whichever row asked: a hundred rows each
// mounting their own printing picker would be a hundred queries waiting to fire.
const swapTarget = ref<DeckPricingLine | null>(null)
const swapOpen = ref(false)
function openSwap(line: DeckPricingLine) {
  swapTarget.value = line
  swapOpen.value = true
}

const swapAll = useChangeDeckCardPrintingsMutation()
const confirmOpen = ref(false)
const swapAllError = ref('')
async function confirmSwapAll() {
  swapAllError.value = ''
  const swaps = swappable.value.flatMap((line) =>
    line.cheapest
      ? [{ id: line.card.id, sectionId: line.section_id, newCardId: line.cheapest.card.id }]
      : [],
  )
  try {
    await swapAll.mutateAsync({ game: props.game, deckId: props.deckId, swaps })
    confirmOpen.value = false
  } catch (error) {
    // The rows before the failure are already swapped and stay so; the refetch the
    // mutation triggers on settle shows the table as it now stands.
    swapAllError.value =
      error instanceof ApiError
        ? `${error.message} Rows swapped before that are kept.`
        : 'Could not swap every printing. Rows swapped so far are kept.'
  }
}
</script>

<template>
  <Card class="mb-6 gap-3 py-4" :aria-busy="updating || undefined">
    <CardHeader class="pb-0">
      <div class="flex flex-wrap items-start justify-between gap-2">
        <div class="min-w-0">
          <CardTitle class="flex items-center gap-2 text-base">
            <Coins class="size-4" aria-hidden="true" /> Where the money is
          </CardTitle>
          <p class="text-muted-foreground mt-1 text-xs" aria-live="polite">
            <template v-if="updating"><UpdatingCue label="Repricing…" /></template>
            <template v-else-if="pending">Pricing every card…</template>
            <template v-else-if="pricing?.total_usd">
              {{ usd(pricing.total_usd) }} as held · {{ usd(pricing.cheapest_total_usd) }} at
              cheapest printings
              <template v-if="pricing.swappable_count > 0">
                · save {{ usd(pricing.saving_usd) }} across {{ pricing.swappable_count }} card{{
                  pricing.swappable_count === 1 ? '' : 's'
                }}
              </template>
              <template v-else>· already the cheapest printings</template>
              <template v-if="pricing.unpriced_count > 0">
                · {{ pricing.unpriced_count }} unpriced
              </template>
            </template>
            <template v-else-if="pricing">Nothing in this deck is priced yet.</template>
          </p>
        </div>
        <div class="flex shrink-0 items-center gap-2">
          <Button
            v-if="!readonly && swappable.length > 0"
            variant="outline"
            size="sm"
            :disabled="swapAll.isPending.value"
            @click="confirmOpen = true"
          >
            <ArrowRightLeft class="size-4" aria-hidden="true" />
            {{ swapAll.isPending.value ? 'Swapping…' : `Swap all (${swappable.length})` }}
          </Button>
          <button
            type="button"
            class="text-muted-foreground hover:text-foreground flex items-center gap-1 text-xs font-medium"
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

      <!-- `isLoadingError`, not `isError` (issue #622): a hiccuped background refetch keeps
        the numbers and says so above them instead of replacing a good table with this. -->
      <p v-else-if="pricingQuery.isLoadingError.value" class="text-muted-foreground text-sm">
        Couldn't price this deck.
      </p>

      <template v-else-if="pricing">
        <StaleNotice
          v-if="pricingQuery.isRefetchError.value"
          label="Couldn't refresh — showing the prices as they last loaded."
        />

        <!-- The resting row: the most expensive cards, each with what it costs the deck. -->
        <ul v-if="top.length" class="flex flex-wrap gap-1.5" data-testid="top-expensive">
          <li v-for="line in top" :key="`${line.card.id}-${line.section_id}`">
            <a
              :href="hrefFor('card', game, line.card.id)"
              class="bg-muted inline-flex items-center gap-1.5 rounded-md px-2 py-0.5 text-xs hover:underline"
              @click="onActivate($event, 'card', game, line.card.id)"
              @pointerenter="warm('card')"
              @focusin="warm('card')"
            >
              <span class="max-w-40 truncate">{{ line.card.name }}</span>
              <span class="text-muted-foreground tabular-nums">{{ usd(line.price_usd) }}</span>
            </a>
          </li>
        </ul>
        <p v-else class="text-muted-foreground text-sm">No priced cards in the deck proper.</p>

        <!-- Every row, most expensive first, with its cheapest printing and the saving. -->
        <div v-if="expanded" :id="detailsId" class="border-t pt-3">
          <div
            class="text-muted-foreground hidden grid-cols-[minmax(0,1fr)_4rem_5.5rem_minmax(0,1fr)_5.5rem_5rem] gap-2 px-1.5 pb-1 text-xs sm:grid"
            aria-hidden="true"
          >
            <span>Card</span>
            <span class="text-right">Copies</span>
            <span class="text-right">Price</span>
            <span>Cheapest printing</span>
            <span class="text-right">Saving</span>
            <span />
          </div>
          <ul class="divide-y">
            <li
              v-for="line in lines"
              :key="`${line.card.id}-${line.section_id}`"
              class="grid grid-cols-[minmax(0,1fr)_auto] gap-x-2 gap-y-0.5 px-1.5 py-1.5 text-sm sm:grid-cols-[minmax(0,1fr)_4rem_5.5rem_minmax(0,1fr)_5.5rem_5rem] sm:items-center"
              data-testid="pricing-line"
            >
              <div class="min-w-0">
                <a
                  :href="hrefFor('card', game, line.card.id)"
                  class="block truncate font-medium hover:underline"
                  :title="line.card.name"
                  @click="onActivate($event, 'card', game, line.card.id)"
                  @pointerenter="warm('card')"
                  @focusin="warm('card')"
                  >{{ line.card.name }}</a
                >
                <p class="text-muted-foreground truncate text-xs">{{ printingLabel(line.card) }}</p>
              </div>
              <span class="text-muted-foreground text-right text-xs tabular-nums sm:text-sm">
                ×{{ line.quantity + line.foil_quantity
                }}<span
                  v-if="line.foil_quantity > 0"
                  class="ml-1 inline-flex items-center gap-0.5"
                  :title="`${line.foil_quantity} foil`"
                  ><Sparkles class="size-3" aria-hidden="true" />{{ line.foil_quantity }}</span
                >
              </span>
              <span class="text-right tabular-nums" data-testid="line-price">{{
                usd(line.price_usd)
              }}</span>
              <div class="text-muted-foreground min-w-0 col-span-2 text-xs sm:col-span-1">
                <template v-if="line.cheapest && line.cheapest.card.id !== line.card.id">
                  <span class="truncate">{{ printingLabel(line.cheapest.card) }}</span>
                  <span class="tabular-nums"> · {{ usd(line.cheapest.price_usd) }}</span>
                </template>
                <template v-else-if="line.cheapest">Already the cheapest</template>
                <template v-else>No priced printing</template>
              </div>
              <span
                class="text-right text-xs tabular-nums sm:text-sm"
                :class="hasSaving(line) ? 'text-success' : 'text-muted-foreground'"
                data-testid="line-saving"
                >{{ hasSaving(line) ? usd(line.saving_usd) : '—' }}</span
              >
              <span class="col-span-2 flex justify-end sm:col-span-1">
                <Button
                  v-if="!readonly && hasSaving(line)"
                  variant="outline"
                  size="sm"
                  class="h-7 px-2 text-xs"
                  :aria-label="`Swap ${line.card.name} to its cheapest printing`"
                  @click="openSwap(line)"
                >
                  <ArrowRightLeft class="size-3.5" aria-hidden="true" /> Swap
                </Button>
              </span>
            </li>
          </ul>
        </div>
      </template>
    </CardContent>
  </Card>

  <DeckPrintingDialog
    v-if="!readonly && swapTarget"
    v-model:open="swapOpen"
    :game="game"
    :deck-id="deckId"
    :section-id="swapTarget.section_id"
    :card="swapTarget.card"
    :quantity="swapTarget.quantity"
    :foil-quantity="swapTarget.foil_quantity"
    :suggested="
      swapTarget.cheapest
        ? { card: swapTarget.cheapest.card, priceUsd: swapTarget.cheapest.price_usd }
        : null
    "
  />

  <Dialog v-if="!readonly" v-model:open="confirmOpen">
    <DialogContent class="bg-background w-[min(92vw,26rem)] rounded-xl border p-6 shadow-xl">
      <DialogTitle>Swap to cheapest printings?</DialogTitle>
      <DialogDescription>
        {{ swappable.length }} card{{ swappable.length === 1 ? '' : 's' }} will change to
        {{ swappable.length === 1 ? 'its' : 'their' }} cheapest printing, saving about
        {{ usd(pricing?.saving_usd) }}. Copies and finishes stay as they are; you can change any
        printing back afterwards.
      </DialogDescription>
      <p v-if="swapAllError" class="text-destructive mt-2 text-sm" aria-live="polite">
        {{ swapAllError }}
      </p>
      <div class="mt-4 flex justify-end gap-2">
        <DialogClose
          :class="buttonVariants({ variant: 'ghost' })"
          :disabled="swapAll.isPending.value"
          >Cancel</DialogClose
        >
        <Button :disabled="swapAll.isPending.value" @click="confirmSwapAll">
          {{ swapAll.isPending.value ? 'Swapping…' : 'Swap all' }}
        </Button>
      </div>
    </DialogContent>
  </Dialog>
</template>
