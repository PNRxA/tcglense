<script setup lang="ts">
import { computed } from 'vue'
import { Check, Loader2, Minus, Plus, Sparkles, X } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger } from '@/components/ui/select'
import CardImage from '@/components/cards/CardImage.vue'
import PrintingPickerGrid from '@/components/printings/PrintingPickerGrid.vue'
import PrintingTile from '@/components/printings/PrintingTile.vue'
import { displayUsdPrice } from '@/lib/cardPrice'
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

const rows = computed(() => [
  {
    key: 'quantity' as const,
    label: 'Regular',
    value: props.target.quantity,
    was: props.owned.quantity,
    icon: null,
  },
  {
    key: 'foil_quantity' as const,
    label: 'Foil',
    value: props.target.foil_quantity,
    was: props.owned.foil_quantity,
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
              <SelectItem v-for="candidate in match.candidates" :key="candidate" :value="candidate">
                <span class="flex min-w-0 items-center gap-2">
                  <span v-if="nameCard(candidate)" aria-hidden="true" class="shrink-0">
                    <CardImage
                      :game="game"
                      :id="nameCard(candidate)!.id"
                      :name="candidate"
                      :has-image="nameCard(candidate)!.has_image"
                      size="small"
                      class="w-6"
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

        <!-- Copies to keep in the collection (absolute; defaults to what you owned + 1). -->
        <div class="space-y-1.5">
          <div
            v-for="row in rows"
            :key="row.key"
            class="flex flex-wrap items-center justify-between gap-x-3 gap-y-1"
          >
            <span class="flex items-center gap-1.5 text-sm">
              <component :is="row.icon" v-if="row.icon" class="size-3.5" aria-hidden="true" />
              {{ row.label }}
              <span v-if="row.was > 0" class="text-muted-foreground text-xs"
                >(had {{ row.was }})</span
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
              <span
                class="w-8 text-center text-sm font-medium tabular-nums"
                aria-live="polite"
                :aria-label="`${row.label}: ${row.value}`"
                >{{ row.value }}</span
              >
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
