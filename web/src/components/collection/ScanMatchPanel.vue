<script setup lang="ts">
import { computed } from 'vue'
import { Check, Loader2, Minus, Plus, Sparkles, TriangleAlert, X } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import CardImage from '@/components/cards/CardImage.vue'
import { displayUsdPrice } from '@/lib/cardPrice'
import { printingMetadataLabel } from '@/lib/printings'
import type { Card, CollectionQuantities } from '@/lib/api'
import type { ScanMatch } from '@/composables/useScanSession'
import { useCurrency } from '@/composables/useCurrency'

// The editable match panel: the card the scan resolved to, shown large, with a name
// corrector (when the OCR is ambiguous), a printing picker, and regular/foil steppers.
// It's tentative — nothing is written until the next card is scanned (or the session ends)
// — so this is the window to fix a wrong match before it commits.
const props = defineProps<{
  game: string
  match: ScanMatch
  prints: Card[]
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

const money = useCurrency()
const price = computed(() => {
  const picked = props.selectedCard ? displayUsdPrice(props.selectedCard.prices) : null
  return picked ? { ...picked, text: money.formatUsd(picked.amount) } : null
})

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
  <div class="grid gap-5 sm:grid-cols-[minmax(0,10rem)_1fr]">
    <!-- The matched printing's art, big enough to eyeball against the physical card. -->
    <CardImage
      v-if="selectedCard"
      :game="game"
      :id="selectedCard.id"
      :name="selectedCard.name"
      :has-image="selectedCard.has_image"
      size="normal"
      class="w-full max-w-40"
    />
    <div
      v-else
      class="bg-muted text-muted-foreground flex aspect-[61/85] w-full max-w-40 items-center justify-center rounded-lg text-sm"
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

        <!-- Name: a heading when unambiguous, a corrector when the OCR had alternatives. -->
        <Select
          v-if="match.candidates.length > 1"
          :model-value="match.name"
          :disabled="disabled"
          @update:model-value="(v) => emit('name', String(v))"
        >
          <SelectTrigger class="mt-1 w-full" aria-label="Matched card name">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem v-for="candidate in match.candidates" :key="candidate" :value="candidate">
              {{ candidate }}
            </SelectItem>
          </SelectContent>
        </Select>
        <h2 v-else class="text-xl font-semibold tracking-tight">{{ match.name }}</h2>
      </div>

      <!-- Printing picker: pre-selected from the set/collector hint (or newest), overridable. -->
      <div>
        <label class="text-muted-foreground mb-1 block text-xs font-medium">Printing</label>
        <div v-if="printsLoading" class="text-muted-foreground flex items-center gap-2 text-sm">
          <Loader2 class="size-4 animate-spin" aria-hidden="true" />
          Loading printings…
        </div>
        <Select
          v-else-if="prints.length"
          :model-value="selectedId"
          :disabled="disabled"
          @update:model-value="(v) => emit('select', String(v))"
        >
          <SelectTrigger class="w-full" aria-label="Printing">
            <SelectValue placeholder="Pick a printing" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem v-for="card in prints" :key="card.id" :value="card.id">
              <span class="truncate">{{ card.set_name }}</span>
              <span class="text-muted-foreground ml-1">— {{ printingMetadataLabel(card) }}</span>
            </SelectItem>
          </SelectContent>
        </Select>
        <p v-else-if="!printsError" class="text-muted-foreground text-sm">No printings found.</p>
        <div
          v-if="printsError"
          class="border-destructive/40 text-destructive mt-2 flex flex-wrap items-center gap-2 rounded-md border px-2.5 py-2 text-xs"
          role="alert"
        >
          <TriangleAlert class="size-3.5 shrink-0" aria-hidden="true" />
          <span>
            {{
              prints.length
                ? "Couldn't load every printing. Retry or choose a loaded printing manually."
                : "Couldn't load printings. Please retry."
            }}
          </span>
          <Button
            variant="outline"
            size="sm"
            class="ml-auto h-7 px-2"
            :disabled="disabled"
            @click="emit('retryPrintings')"
          >
            Retry
          </Button>
        </div>
        <div
          v-if="prints.length"
          class="text-muted-foreground mt-1.5 flex flex-wrap items-center justify-between gap-2 text-xs"
        >
          <span>{{ prints.length }} of {{ printsTotal }} printings loaded</span>
          <Button
            v-if="printsHasMore && !printsError"
            variant="ghost"
            size="sm"
            class="h-7 px-2"
            :disabled="printsLoadingMore || disabled"
            @click="emit('loadMore')"
          >
            <Loader2 v-if="printsLoadingMore" class="size-3.5 animate-spin" aria-hidden="true" />
            {{ printsLoadingMore ? 'Loading…' : 'Load more' }}
          </Button>
        </div>
        <p v-if="price" class="text-muted-foreground mt-1 text-xs tabular-nums">
          {{ price.text }}<span v-if="price.foil" class="ml-0.5 uppercase opacity-70">foil</span>
        </p>
      </div>

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
              class="size-8"
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
              class="size-8"
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
        <div class="flex items-center gap-2">
          <Button size="sm" :disabled="!ready || disabled" @click="emit('confirm')">
            <Check class="size-4" aria-hidden="true" />
            Confirm
          </Button>
          <Button
            variant="ghost"
            size="sm"
            class="text-muted-foreground"
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
</template>
