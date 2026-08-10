<script setup lang="ts">
import { computed } from 'vue'
import { Check, Loader2, Minus, Plus, Sparkles, X } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger } from '@/components/ui/select'
import CardImage from '@/components/cards/CardImage.vue'
import PrintingPickerGrid from '@/components/printings/PrintingPickerGrid.vue'
import PrintingTile from '@/components/printings/PrintingTile.vue'
import { displayUsdPrice } from '@/lib/cardPrice'
import { holdingDelta, holdingDeltaIsFoil, holdingDeltaSummary } from '@/lib/holdingDelta'
import { printingMetadataLabel } from '@/lib/printings'
import type { Card, CollectionQuantities, ScanMatch as ScanCandidate } from '@/lib/api'
import type { ScanMatch } from '@/composables/useScanSession'
import { useCurrency } from '@/composables/useCurrency'

// The editable match panel: the card the scan resolved to, shown large, with a name
// corrector (when the OCR is ambiguous), the printing picker, and regular/foil steppers.
// It's tentative — nothing is written until the next card is scanned (or the session ends)
// — so this is the window to fix a wrong match before it commits. The printing picker is the
// same visual PrintingPickerGrid used across the app (deck/quick-add), so a correction shows
// card art rather than a text-only line, and the name corrector carries each candidate's art.
const props = defineProps<{
  game: string
  match: ScanMatch
  prints: Card[]
  printsFiltered: Card[]
  printsLoading: boolean
  printsLoadingMore: boolean
  printsError: boolean
  printsTotal: number
  printsHasMore: boolean
  selectedCard: Card | null
  selectedId: string
  owned: CollectionQuantities
  target: CollectionQuantities
  ready: boolean
  resolving: boolean
  /** The current printing's holding failed to load — a terminal state, not a slow one. */
  ownedError: boolean
  disabled: boolean
  /** Ranked visual matches from the last capture — their art backs the name corrector. */
  candidates: ScanCandidate[]
}>()

const emit = defineEmits<{
  name: [string]
  select: [string]
  adjust: ['quantity' | 'foil_quantity', number]
  confirm: []
  discard: []
  loadMore: []
  retryPrintings: []
}>()

// The loaded-page filter for the shared picker grid (forwarded to the scan session's picker).
const filter = defineModel<string>('filter', { required: true })

const money = useCurrency()
const price = computed(() => {
  const picked = props.selectedCard ? displayUsdPrice(props.selectedCard.prices) : null
  return picked ? { ...picked, text: money.formatUsd(picked.amount) } : null
})

// Representative art for a candidate name: the highest-ranked visual match that carries it
// (the names are derived from these matches, so one always exists while the corrector shows).
function nameCard(name: string): Card | null {
  return props.candidates.find((candidate) => candidate.card.name === name)?.card ?? null
}

// `target` still carries the previous card's counts until the newly matched printing's
// holding settles and re-seeds them (`ready`), so the steppers must not show a number
// that is about to change under the user. But "not seeded" is not the same as "still
// working": a failed holding read, a failed printings page with nothing picked, and a
// name that resolves to no printings at all are all terminal — the panel already shows
// its own error text and a Retry for each. Spinning there would assert progress that
// will never come, so only a genuinely in-flight resolution gets the spinner; the rest
// get the same neutral placeholder the art slot falls back to.
const countsPending = computed(
  () =>
    !props.ready &&
    !props.ownedError &&
    (props.resolving || props.printsLoading || props.selectedCard !== null),
)

// What the stepper's count reads as: the settled number, or why there isn't one — the
// spinner and the placeholder are both silent to a screen reader on their own, and
// "reading count" on a state that has stopped reading would contradict the panel's
// own error text.
function countLabel(value: number): string {
  if (countsPending.value) return 'reading count'
  if (!props.ready) return 'count unavailable'
  return String(value)
}

// What committing right now would change. The scanner routes its copy to foil off a printed
// ★ it detected visually, and that call can be wrong — so which of the two numbers moved has
// to be obvious *before* the card commits, not discoverable by comparing "3" against
// "(had 2)". Only meaningful once the holding has seeded: until then `target` still carries
// the previous card's counts, the same reason `(had N)` is gated on `ready`.
const delta = computed(() => holdingDelta(props.owned, props.target))
const deltaSummary = computed(() => (props.ready ? holdingDeltaSummary(delta.value) : null))
const deltaIsFoil = computed(() => holdingDeltaIsFoil(delta.value))
// The glyph carries the finish where there is one to carry; a change that only takes copies
// away has no foil story to tell and must not lead with a plus sign.
const deltaIcon = computed(() => {
  if (deltaIsFoil.value) return Sparkles
  const { quantity, foil_quantity } = delta.value
  return quantity <= 0 && foil_quantity <= 0 ? Minus : Plus
})

// Foil accents amber and regular the primary hue, matching the session log's chips so the
// tentative panel and the history describe one card the same way.
const FOIL_ACCENT = 'bg-amber-500/10 text-amber-700 ring-amber-500/30 dark:text-amber-400'
const REGULAR_ACCENT = 'bg-primary/10 text-primary ring-primary/20'

const rows = computed(() => [
  {
    key: 'quantity' as const,
    label: 'Regular',
    value: props.target.quantity,
    was: props.owned.quantity,
    delta: props.ready ? delta.value.quantity : 0,
    accent: REGULAR_ACCENT,
    icon: null,
  },
  {
    key: 'foil_quantity' as const,
    label: 'Foil',
    value: props.target.foil_quantity,
    was: props.owned.foil_quantity,
    delta: props.ready ? delta.value.foil_quantity : 0,
    accent: FOIL_ACCENT,
    icon: Sparkles,
  },
])
</script>

<template>
  <div class="space-y-4">
    <div
      class="grid grid-cols-[5.5rem_minmax(0,1fr)] gap-3 sm:grid-cols-[minmax(0,10rem)_1fr] sm:gap-5"
    >
      <!-- The matched printing's art, big enough to eyeball against the physical card. -->
      <CardImage
        v-if="selectedCard"
        :game="game"
        :id="selectedCard.id"
        :name="selectedCard.name"
        :has-image="selectedCard.has_image"
        size="normal"
        class="w-full max-w-40 justify-self-center"
      />
      <div
        v-else
        class="bg-muted text-muted-foreground flex aspect-[61/85] w-full max-w-40 items-center justify-center justify-self-center rounded-lg text-sm"
      >
        <Loader2 v-if="resolving || printsLoading" class="size-5 animate-spin" aria-hidden="true" />
        <span v-else>No art</span>
      </div>

      <div class="min-w-0 space-y-3">
        <div>
          <p class="text-muted-foreground text-xs">
            Read as “<span class="font-medium">{{ match.ocrName }}</span
            >”
          </p>

          <!-- Name: a heading when unambiguous, a corrector when the OCR had alternatives.
             The trigger stays a compact text line (the select has a fixed control height);
             the open list carries each candidate's card art so the pick isn't text-only. -->
          <Select
            v-if="match.candidates.length > 1"
            :model-value="match.name"
            :disabled="disabled"
            @update:model-value="(v) => emit('name', String(v))"
          >
            <SelectTrigger class="mt-1 min-h-11 w-full lg:min-h-9" aria-label="Matched card name">
              <span class="truncate">{{ match.name }}</span>
            </SelectTrigger>
            <SelectContent>
              <SelectItem
                v-for="candidate in match.candidates"
                :key="candidate"
                :value="candidate"
                class="py-2"
              >
                <span class="flex min-w-0 items-center gap-3">
                  <span v-if="nameCard(candidate)" aria-hidden="true" class="shrink-0">
                    <CardImage
                      :game="game"
                      :id="nameCard(candidate)!.id"
                      :name="candidate"
                      :has-image="nameCard(candidate)!.has_image"
                      size="small"
                      class="w-12"
                    />
                  </span>
                  <span class="truncate">{{ candidate }}</span>
                </span>
              </SelectItem>
            </SelectContent>
          </Select>
          <h2 v-else class="text-xl font-semibold tracking-tight [overflow-wrap:anywhere]">
            {{ match.name }}
          </h2>
        </div>

        <p v-if="price" class="text-muted-foreground text-xs tabular-nums">
          {{ price.text }}<span v-if="price.foil" class="ml-0.5 uppercase opacity-70">foil</span>
        </p>

        <!-- The headline answer to "which number did that scan go into?", stated before the
           steppers so a wrong foil call is caught at a glance rather than after committing.
           The matching row below is tinted the same colour. -->
        <p
          v-if="deltaSummary"
          data-testid="scan-delta-summary"
          class="inline-flex items-center gap-1.5 rounded-md px-2 py-1 text-xs font-medium ring-1"
          :class="deltaIsFoil ? FOIL_ACCENT : REGULAR_ACCENT"
        >
          <component :is="deltaIcon" class="size-3.5 shrink-0" aria-hidden="true" />
          {{ deltaSummary }}
        </p>

        <!-- Copies to keep in the collection (absolute; defaults to what you owned + 1). -->
        <div class="space-y-1.5">
          <div
            v-for="row in rows"
            :key="row.key"
            class="-mx-2 flex flex-wrap items-center justify-between gap-x-3 gap-y-1 rounded-lg px-2 py-1 ring-1"
            :class="row.delta ? row.accent : 'ring-transparent'"
          >
            <span class="flex items-center gap-1.5 text-sm">
              <component :is="row.icon" v-if="row.icon" class="size-3.5" aria-hidden="true" />
              {{ row.label }}
              <span v-if="ready && row.was > 0" class="text-muted-foreground text-xs"
                >(had {{ row.was }})</span
              >
              <!-- The summary above already states the change in words, so this is the
                 visual echo on the row it belongs to, not a second announcement. -->
              <span
                v-if="row.delta"
                class="text-xs font-semibold tabular-nums"
                aria-hidden="true"
                >{{ row.delta > 0 ? `+${row.delta}` : row.delta }}</span
              >
            </span>
            <div class="flex items-center gap-1.5">
              <Button
                variant="outline"
                size="icon"
                class="size-11 lg:size-8"
                :disabled="!ready || disabled"
                :aria-disabled="row.value <= 0"
                :class="{ 'pointer-events-none opacity-50': row.value <= 0 }"
                :aria-label="`Remove one ${row.label.toLowerCase()} copy`"
                @click="emit('adjust', row.key, -1)"
              >
                <Minus />
              </Button>
              <!-- One aria-live element across all three states, so a settled count is
                 announced as an update rather than a freshly inserted region. -->
              <span
                class="flex w-8 items-center justify-center text-center text-sm font-medium tabular-nums"
                aria-live="polite"
                :aria-label="`${row.label}: ${countLabel(row.value)}`"
              >
                <Loader2
                  v-if="countsPending"
                  class="text-muted-foreground size-4 animate-spin"
                  aria-hidden="true"
                />
                <span v-else-if="!ready" class="text-muted-foreground" aria-hidden="true">—</span>
                <template v-else>{{ row.value }}</template>
              </span>
              <Button
                variant="outline"
                size="icon"
                class="size-11 lg:size-8"
                :disabled="!ready || disabled"
                :aria-label="`Add one ${row.label.toLowerCase()} copy`"
                @click="emit('adjust', row.key, 1)"
              >
                <Plus />
              </Button>
            </div>
          </div>
        </div>

        <div class="flex flex-wrap items-center justify-between gap-2 pt-1">
          <p class="text-muted-foreground text-xs">Or capture the next card to add this one.</p>
          <div class="flex max-w-full flex-wrap items-center justify-end gap-2">
            <Button
              size="sm"
              class="min-h-11 lg:min-h-8"
              :disabled="!ready || disabled"
              @click="emit('confirm')"
            >
              <Check class="size-4" aria-hidden="true" />
              Add card
            </Button>
            <Button
              variant="ghost"
              size="sm"
              class="text-muted-foreground min-h-11 lg:min-h-8"
              :disabled="disabled"
              @click="emit('discard')"
            >
              <X class="size-4" aria-hidden="true" />
              Discard
            </Button>
          </div>
        </div>
      </div>
    </div>

    <!-- Printing picker: pre-selected from the set/collector hint (or newest), overridable.
       Reuses the shared visual picker so a correction shows card art, not just a text line. -->
    <div>
      <label class="text-muted-foreground mb-1.5 block text-xs font-medium">Printing</label>
      <PrintingPickerGrid
        v-model:filter="filter"
        :printings="prints"
        :filtered-printings="printsFiltered"
        :total="printsTotal"
        :pending="printsLoading"
        :error="printsError"
        :has-more="printsHasMore"
        :loading-more="printsLoadingMore"
        error-message="Couldn't load printings. Retry below or choose a loaded printing."
        empty-message="No printings found."
        @load-more="emit('loadMore')"
      >
        <template #tile="{ printing }">
          <PrintingTile
            :game="game"
            :card="printing"
            selectable
            :current="printing.id === selectedId"
            :disabled="disabled"
            :aria-label="
              printing.id === selectedId
                ? `${printing.set_name} ${printingMetadataLabel(printing)}, selected printing`
                : `Use ${printing.set_name} ${printingMetadataLabel(printing)}`
            "
            @select="emit('select', printing.id)"
          />
        </template>
      </PrintingPickerGrid>
      <!-- Only when the grid has no "Load more" of its own to retry through (a failed first
         page, or a failed refetch of an already-complete list) — otherwise it would stack a
         third recovery control beside the grid's own error text and load-more button. -->
      <div v-if="printsError && !printsHasMore" class="mt-2 flex justify-center">
        <Button
          variant="outline"
          size="sm"
          class="min-h-11 lg:min-h-8"
          :disabled="disabled"
          @click="emit('retryPrintings')"
        >
          Retry loading printings
        </Button>
      </div>
    </div>
  </div>
</template>
