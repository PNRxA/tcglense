<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { ArrowRightLeft, Check, Loader2, Minus, Plus, RefreshCw, Sparkles } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import OwnedCountBadge from '@/components/cards/OwnedCountBadge.vue'
import DeckPrintingDialog from '@/components/decks/DeckPrintingDialog.vue'
import { useOwnedCountEditor, type OwnedCountSeed } from '@/composables/useOwnedCountEditor'
import { useMoveDeckCardMutation, useSetDeckCardMutation } from '@/composables/useDecks'
import type { Card, DeckSection } from '@/lib/api'

// Quick-add / edit control overlaid on a card tile inside the deck builder (issue #363),
// the deck's analogue of the collection's OwnedCountControl. A corner chip shows how many
// copies are in the deck (or a "+" for a card not yet added); its popover has regular/foil
// steppers and a "move to another section" picker.
//
// The counts are seeded from the deck detail (which is authoritative and reloads on every
// edit), so — unlike the collection control — there's no per-card fetch: the editor reuses
// the shared debounce/serialize/flush machinery via an injected `saveFn` that writes a
// (deck, section, card) row. Rendered only for the deck's owner (the public deck view is
// read-only).
const props = defineProps<{
  game: string
  deckId: number
  sectionId: number
  card: Card
  quantity: number
  foilQuantity: number
  sections: DeckSection[]
}>()

const open = ref(false)
const printingOpen = ref(false)
const preparingPrinting = ref(false)
const game = toRef(props, 'game')
const cardId = computed(() => props.card.id)

const setCard = useSetDeckCardMutation()
const moveCard = useMoveDeckCardMutation()

// Seed straight from the deck detail's counts — always available, so the steppers are never
// disabled waiting on a fetch.
const seed = computed<OwnedCountSeed>(() => ({
  quantity: props.quantity,
  foil_quantity: props.foilQuantity,
}))
const { regular, foil, adjust, flush, saving, saveError } = useOwnedCountEditor(
  game,
  cardId,
  seed,
  {
    saveFn: (id, quantity, foilQuantity) =>
      setCard.mutateAsync({
        game: props.game,
        deckId: props.deckId,
        sectionId: props.sectionId,
        id,
        quantity,
        foil_quantity: foilQuantity,
      }),
  },
)

const total = computed(() => props.quantity + props.foilQuantity)
const inDeck = computed(() => total.value > 0)
const editorTotal = computed(() => regular.value + foil.value)
const otherSections = computed(() => props.sections.filter((s) => s.id !== props.sectionId))

const rows = computed(() => [
  { key: 'quantity' as const, label: 'Regular', value: regular.value, icon: null },
  { key: 'foil' as const, label: 'Foil', value: foil.value, icon: Sparkles },
])

// The "move to section" picker holds no persistent selection — its `get` stays '' so reka
// shows the placeholder — and choosing a section moves the card, then closes the popover.
const moveTarget = computed({
  get: () => '',
  set: (value: string) => {
    if (!value) return
    void moveToSection(Number(value))
  },
})

async function moveToSection(toSectionId: number) {
  if (!(await flush())) return
  await moveCard.mutateAsync({
    game: props.game,
    deckId: props.deckId,
    id: props.card.id,
    fromSectionId: props.sectionId,
    toSectionId,
  })
  open.value = false
}

async function openPrintingPicker() {
  if (preparingPrinting.value) return
  preparingPrinting.value = true
  try {
    if (!(await flush())) return
    open.value = false
    printingOpen.value = true
  } finally {
    preparingPrinting.value = false
  }
}
</script>

<template>
  <Popover v-model:open="open">
    <PopoverTrigger as-child>
      <button
        type="button"
        class="group/add absolute bottom-1.5 left-1.5 z-20 inline-flex items-center rounded-md outline-none transition focus-visible:ring-2 focus-visible:ring-ring"
        :aria-label="
          inDeck ? `Edit copies of ${card.name} in this deck` : `Add ${card.name} to this deck`
        "
        @click.stop
      >
        <OwnedCountBadge
          v-if="inDeck"
          :quantity="quantity"
          :foil-quantity="foilQuantity"
          kind="owned"
          :tooltip="false"
          hover-as-add
        />
        <span
          v-else
          class="bg-primary/90 text-primary-foreground inline-flex items-center justify-center rounded-md p-1.5 shadow"
        >
          <Plus class="size-4" aria-hidden="true" />
        </span>
      </button>
    </PopoverTrigger>

    <PopoverContent side="top" align="start" :side-offset="6" class="w-60 p-3">
      <div class="mb-3 flex items-center justify-between gap-2">
        <p class="truncate text-sm font-medium" :title="card.name">{{ card.name }}</p>
        <span
          class="text-muted-foreground flex shrink-0 items-center gap-1 text-xs"
          aria-live="polite"
        >
          <template v-if="saveError"><span class="text-destructive">Retry</span></template>
          <template v-else-if="saving">
            <Loader2 class="size-3.5 animate-spin" aria-hidden="true" /> Saving…
          </template>
          <template v-else-if="editorTotal > 0">
            <Check class="size-3.5" aria-hidden="true" /> Saved
          </template>
        </span>
      </div>

      <div class="space-y-2">
        <div v-for="row in rows" :key="row.key" class="flex items-center justify-between gap-3">
          <span class="flex items-center gap-1.5 text-sm">
            <component :is="row.icon" v-if="row.icon" class="size-3.5" aria-hidden="true" />
            {{ row.label }}
          </span>
          <div class="flex items-center gap-2">
            <Button
              variant="outline"
              size="icon"
              :aria-disabled="row.value <= 0"
              :class="{ 'pointer-events-none opacity-50': row.value <= 0 }"
              :aria-label="`Remove one ${row.label.toLowerCase()} copy of ${card.name}`"
              @click="adjust(row.key, -1)"
            >
              <Minus />
            </Button>
            <span
              class="w-8 text-center text-sm font-medium tabular-nums"
              aria-live="polite"
              aria-atomic="true"
              :aria-label="`${row.label}: ${row.value}`"
              >{{ row.value }}</span
            >
            <Button
              variant="outline"
              size="icon"
              :aria-label="`Add one ${row.label.toLowerCase()} copy of ${card.name}`"
              @click="adjust(row.key, 1)"
            >
              <Plus />
            </Button>
          </div>
        </div>
      </div>

      <!-- Move this card to another section of the deck. -->
      <div v-if="otherSections.length && inDeck" class="mt-3 border-t pt-2">
        <label class="text-muted-foreground flex items-center gap-1.5 text-xs">
          <ArrowRightLeft class="size-3.5" aria-hidden="true" /> Move to section
        </label>
        <Select v-model="moveTarget">
          <SelectTrigger size="sm" class="mt-1 w-full" aria-label="Move to section">
            <SelectValue placeholder="Choose a section…" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem v-for="s in otherSections" :key="s.id" :value="String(s.id)">
              {{ s.name }}
            </SelectItem>
          </SelectContent>
        </Select>
      </div>

      <Button
        v-if="inDeck"
        variant="outline"
        size="sm"
        class="mt-3 w-full"
        :disabled="preparingPrinting"
        @click="openPrintingPicker"
      >
        <Loader2 v-if="preparingPrinting" class="size-3.5 animate-spin" />
        <RefreshCw v-else class="size-3.5" /> Change printing
      </Button>
    </PopoverContent>
  </Popover>

  <DeckPrintingDialog
    v-model:open="printingOpen"
    :game="game"
    :deck-id="deckId"
    :section-id="sectionId"
    :card="card"
    :quantity="quantity"
    :foil-quantity="foilQuantity"
  />
</template>
