<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Loader2, Plus, Search } from '@lucide/vue'
import { Input } from '@/components/ui/input'
import { useCardNameSuggestions, useCardPrintingsByName } from '@/composables/useQuickAdd'
import { useSetDeckCardMutation } from '@/composables/useDecks'
import type { Card, DeckCardEntry, DeckSection } from '@/lib/api'
import { displayUsdPrice } from '@/lib/cardPrice'

// The deck builder's "add cards" box (issue #363): search a card name, pick a printing, and
// add it to a chosen section — reusing the public card-name/printings reads that power the
// collection quick-add. Adds are additive (current count + 1) so re-adding a card bumps it.
const props = defineProps<{
  game: string
  deckId: number
  sections: DeckSection[]
  cards: DeckCardEntry[]
}>()

const term = ref('')
const debounced = ref('')
let timer: ReturnType<typeof setTimeout> | null = null
watch(term, (value) => {
  if (timer) clearTimeout(timer)
  timer = setTimeout(() => {
    debounced.value = value
  }, 250)
})

const gameRef = computed(() => props.game)
const namesQuery = useCardNameSuggestions(gameRef, debounced)
const names = computed(() => namesQuery.data.value?.data ?? [])

// The chosen name -> its printings (fetched only once a name is picked).
const pickedName = ref('')
const pickedEnabled = computed(() => pickedName.value.length > 0)
const printingsQuery = useCardPrintingsByName(gameRef, pickedName, { enabled: pickedEnabled })
const printings = computed(() => printingsQuery.data.value?.data ?? [])

// The section a picked printing is added to (defaults to the first section).
const targetSectionId = ref<number | null>(null)
watch(
  () => props.sections,
  (sections) => {
    const first = sections[0]
    if (targetSectionId.value == null && first) targetSectionId.value = first.id
  },
  { immediate: true },
)

const setCard = useSetDeckCardMutation()

function pickName(name: string) {
  pickedName.value = name
}

function currentCounts(cardId: string, sectionId: number): { quantity: number; foil: number } {
  const entry = props.cards.find((c) => c.card.id === cardId && c.section_id === sectionId)
  return { quantity: entry?.quantity ?? 0, foil: entry?.foil_quantity ?? 0 }
}

async function addPrinting(card: Card) {
  const sectionId = targetSectionId.value
  if (sectionId == null) return
  const current = currentCounts(card.id, sectionId)
  await setCard.mutateAsync({
    game: props.game,
    deckId: props.deckId,
    sectionId,
    id: card.id,
    quantity: current.quantity + 1,
    foil_quantity: current.foil,
  })
}

function reset() {
  term.value = ''
  debounced.value = ''
  pickedName.value = ''
}
</script>

<template>
  <div class="bg-card rounded-lg border p-3">
    <div class="flex flex-wrap items-center gap-2">
      <div class="relative min-w-[12rem] flex-1">
        <Search
          class="text-muted-foreground pointer-events-none absolute top-1/2 left-2 size-4 -translate-y-1/2"
          aria-hidden="true"
        />
        <Input v-model="term" placeholder="Add a card by name…" class="pl-8" />
      </div>
      <label class="text-muted-foreground flex items-center gap-1.5 text-sm">
        to
        <select
          v-model.number="targetSectionId"
          class="border-input bg-background rounded-md border px-2 py-1.5 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <option v-for="s in sections" :key="s.id" :value="s.id">{{ s.name }}</option>
        </select>
      </label>
    </div>

    <!-- Name suggestions (until a name is picked). -->
    <div v-if="!pickedName && names.length" class="mt-2 flex flex-wrap gap-1.5">
      <button
        v-for="name in names"
        :key="name"
        class="bg-muted hover:bg-accent rounded-md px-2 py-1 text-sm"
        @click="pickName(name)"
      >
        {{ name }}
      </button>
    </div>

    <!-- Printings of the chosen name. -->
    <div v-if="pickedName" class="mt-3">
      <div class="mb-2 flex items-center justify-between">
        <p class="text-sm">
          Printings of <strong>{{ pickedName }}</strong>
        </p>
        <button class="text-muted-foreground hover:text-foreground text-xs" @click="reset">
          Clear
        </button>
      </div>
      <Loader2 v-if="printingsQuery.isPending.value" class="text-muted-foreground size-4 animate-spin" />
      <div v-else class="grid max-h-64 gap-1.5 overflow-y-auto sm:grid-cols-2">
        <button
          v-for="card in printings"
          :key="card.id"
          class="hover:bg-accent flex items-center justify-between gap-2 rounded-md border px-2 py-1.5 text-left"
          @click="addPrinting(card)"
        >
          <span class="min-w-0">
            <span class="text-muted-foreground text-xs"
              >{{ card.set_code.toUpperCase() }} · #{{ card.collector_number }}</span
            >
          </span>
          <span class="flex items-center gap-1.5">
            <span v-if="displayUsdPrice(card.prices)" class="text-xs tabular-nums"
              >${{ displayUsdPrice(card.prices)?.amount }}</span
            >
            <Plus class="size-4" aria-hidden="true" />
          </span>
        </button>
      </div>
    </div>
  </div>
</template>
