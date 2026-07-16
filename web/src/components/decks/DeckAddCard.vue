<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Search } from '@lucide/vue'
import { Input } from '@/components/ui/input'
import DeckAddPrintTile from '@/components/decks/DeckAddPrintTile.vue'
import PrintingPickerGrid from '@/components/printings/PrintingPickerGrid.vue'
import { useCardNameSuggestions } from '@/composables/useQuickAdd'
import { usePrintingPicker } from '@/composables/usePrintings'
import { useSetDeckCardMutation } from '@/composables/useDecks'
import type { Card, DeckCardEntry, DeckSection } from '@/lib/api'
import { automaticDeckSection } from '@/lib/deckCategories'

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
// Editing the search box after picking a name dismisses the (now-stale) printings menu on
// its own, so the user drops straight back to name search without hitting "Clear".
// `pickName` never touches `term`, so this only fires on genuine typing.
watch(term, () => {
  if (pickedName.value) pickedName.value = ''
})
const pickedEnabled = computed(() => pickedName.value.length > 0)
const picker = usePrintingPicker(gameRef, pickedName, { enabled: pickedEnabled })

// Automatic is the default: each printing files into its preset type bucket. A user can
// still pin the add box to any explicit section for functional/custom categorisation.
const target = ref('auto')
watch(
  () => props.sections,
  (sections) => {
    if (target.value === 'auto') return
    if (!sections.some((section) => String(section.id) === target.value)) target.value = 'auto'
  },
  { immediate: true },
)

const setCard = useSetDeckCardMutation()

// Optimistic per-(card, section) count so rapid re-adds (building a playset by clicking the
// same printing several times) stack instead of all reading `quantity=0` off the deck cache
// that only refreshes after each write's refetch lands. Cleared once the refetch catches up.
const optimistic = new Map<string, number>()
const keyOf = (cardId: string, sectionId: number) => `${cardId}:${sectionId}`

// In-flight adds, keyed the same way, so each tile can spin its own "+" while its write is
// outstanding (a single shared mutation's `isPending` can't tell the tiles apart). A ref'd
// Set is reactive for add/delete, so the `isPending(card.id)` prop below ticks on its own.
const pending = ref(new Set<string>())
watch(
  () => props.cards,
  (cards) => {
    for (const [k, v] of optimistic) {
      const [cardId, sec] = k.split(':')
      const entry = cards.find((c) => c.card.id === cardId && c.section_id === Number(sec))
      if ((entry?.quantity ?? 0) >= v) optimistic.delete(k)
    }
  },
)

function pickName(name: string) {
  pickedName.value = name
}

function currentCounts(cardId: string, sectionId: number): { quantity: number; foil: number } {
  const entry = props.cards.find((c) => c.card.id === cardId && c.section_id === sectionId)
  return { quantity: entry?.quantity ?? 0, foil: entry?.foil_quantity ?? 0 }
}

// Total copies (regular + foil) of a printing already in the current target section — the
// progress badge on its tile. Reactive off `props.cards` + `targetSectionId`, so it ticks
// up as the post-add refetch lands.
function targetSectionId(card: Card): number | null {
  if (target.value === 'auto') return automaticDeckSection(card, props.sections)?.id ?? null
  const sectionId = Number(target.value)
  return Number.isFinite(sectionId) ? sectionId : null
}

function needsExplicitSection(card: Card): boolean {
  return target.value === 'auto' && targetSectionId(card) == null
}

const hasUnclassifiedPrintings = computed(() =>
  picker.printings.value.some((card) => needsExplicitSection(card)),
)

function inTargetCount(card: Card): number {
  const sectionId = targetSectionId(card)
  if (sectionId == null) return 0
  const { quantity, foil } = currentCounts(card.id, sectionId)
  return quantity + foil
}

// Whether an add for this printing (in the current target section) is still in flight.
function isPending(card: Card): boolean {
  const sectionId = targetSectionId(card)
  if (sectionId == null) return false
  return pending.value.has(keyOf(card.id, sectionId))
}

async function addPrinting(card: Card) {
  const sectionId = targetSectionId(card)
  if (sectionId == null) return
  const k = keyOf(card.id, sectionId)
  const server = currentCounts(card.id, sectionId)
  const next = (optimistic.get(k) ?? server.quantity) + 1
  optimistic.set(k, next)
  pending.value.add(k)
  try {
    await setCard.mutateAsync({
      game: props.game,
      deckId: props.deckId,
      sectionId,
      id: card.id,
      quantity: next,
      foil_quantity: server.foil,
    })
  } finally {
    pending.value.delete(k)
  }
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
          v-model="target"
          class="border-input bg-background rounded-md border px-2 py-1.5 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <option value="auto">Automatic (by type)</option>
          <option v-for="s in sections" :key="s.id" :value="String(s.id)">{{ s.name }}</option>
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
      <p
        v-if="!picker.isPending.value && hasUnclassifiedPrintings"
        class="text-muted-foreground mb-2 text-xs"
        role="status"
      >
        Some card types have no safe automatic category. Choose a section above to add them.
      </p>
      <PrintingPickerGrid
        v-model:filter="picker.filter.value"
        class="max-h-[36rem] overflow-y-auto pr-1"
        :printings="picker.printings.value"
        :filtered-printings="picker.filteredPrintings.value"
        :total="picker.total.value"
        :pending="picker.isPending.value"
        :error="picker.failed.value"
        :has-more="picker.hasNextPage.value"
        :loading-more="picker.isFetchingNextPage.value"
        @load-more="picker.loadMore"
      >
        <template #tile="{ printing }">
          <DeckAddPrintTile
            :game="game"
            :card="printing"
            :count="inTargetCount(printing)"
            :loading="isPending(printing)"
            :disabled="needsExplicitSection(printing)"
            @add="addPrinting(printing)"
          />
        </template>
      </PrintingPickerGrid>
    </div>
  </div>
</template>
