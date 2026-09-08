<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { Tag } from '@lucide/vue'
import { buttonVariants } from '@/components/ui/button'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from '@/components/ui/dialog'
import PrintingPickerGrid from '@/components/printings/PrintingPickerGrid.vue'
import PrintingTile from '@/components/printings/PrintingTile.vue'
import { useChangeDeckCardPrintingMutation } from '@/composables/useDecks'
import { usePrintingPicker } from '@/composables/usePrintings'
import { useCurrency } from '@/composables/useCurrency'
import { ApiError, type Card } from '@/lib/api'

// The suggested printing the pricing panel (issue #672) opens this dialog with: the cheapest
// priced printing of the row's card at the row's own finish split, and what the row would
// cost held as it. Shown as its own tile above the grid, so the swap the panel promised is
// one click here rather than a hunt through a hundred printings — and marked in the grid too,
// so the two readings can't disagree about which printing is meant.
export interface SuggestedPrinting {
  card: Card
  priceUsd: string
}

const props = defineProps<{
  game: string
  deckId: number
  sectionId: number
  card: Card
  quantity: number
  foilQuantity: number
  suggested?: SuggestedPrinting | null
}>()
const open = defineModel<boolean>('open', { default: false })
const game = toRef(props, 'game')
const cardName = computed(() => props.card.name)
const enabled = computed(() => open.value)
const picker = usePrintingPicker(game, cardName, { enabled, collectionFilter: true })
const changePrinting = useChangeDeckCardPrintingMutation()
const changingTo = ref('')
const errorMessage = ref('')
const money = useCurrency()

// A suggestion that IS the current printing has nothing to offer — the row already holds it.
const suggestion = computed(() =>
  props.suggested && props.suggested.card.id !== props.card.id ? props.suggested : null,
)
const suggestedPrice = computed(() =>
  suggestion.value ? money.formatUsd(suggestion.value.priceUsd) : null,
)

async function choose(printing: Card) {
  if (printing.id === props.card.id || changePrinting.isPending.value) return
  changingTo.value = printing.id
  errorMessage.value = ''
  try {
    await changePrinting.mutateAsync({
      game: props.game,
      deckId: props.deckId,
      sectionId: props.sectionId,
      id: props.card.id,
      newCardId: printing.id,
    })
    open.value = false
  } catch (error) {
    errorMessage.value =
      error instanceof ApiError ? error.message : 'Could not change printing. Please retry.'
  } finally {
    changingTo.value = ''
  }
}
</script>

<template>
  <Dialog v-model:open="open">
    <!-- Top-anchored (see DialogContent's `anchor`): its printing list paginates, and a
      centred panel shifts every tile up by half of each page's height as it arrives —
      landing a tap on the printing next to the one that was aimed at. -->
    <DialogContent
      anchor="top"
      class="bg-background max-h-[90svh] w-[min(94vw,64rem)] max-w-5xl overflow-y-auto rounded-xl border p-6 shadow-xl"
    >
      <DialogTitle>Change printing</DialogTitle>
      <DialogDescription>
        Choose another printing of {{ card.name }}. Its {{ quantity + foilQuantity }}
        {{ quantity + foilQuantity === 1 ? 'copy' : 'copies' }} and finish counts stay in this
        section.
      </DialogDescription>

      <p v-if="errorMessage" class="text-destructive mt-3 text-sm" aria-live="polite">
        {{ errorMessage }}
      </p>

      <!-- The cheapest printing, when the caller worked one out: the swap the pricing panel
        offered, one click away. The grid below still lists every printing for a different
        choice. -->
      <section v-if="suggestion" class="bg-muted/40 mt-4 rounded-lg border p-3">
        <p class="flex items-center gap-1.5 text-sm font-medium">
          <Tag class="size-4" aria-hidden="true" /> Cheapest printing
        </p>
        <p class="text-muted-foreground mt-0.5 text-xs">
          The lowest-priced printing for how this row is held
          <template v-if="suggestedPrice"> — {{ suggestedPrice }} for its copies</template>.
        </p>
        <div class="mt-3 grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4">
          <PrintingTile
            :game="game"
            :card="suggestion.card"
            selectable
            :loading="changingTo === suggestion.card.id"
            :disabled="changePrinting.isPending.value"
            :aria-label="`Change to the cheapest printing, ${suggestion.card.set_name} ${suggestion.card.collector_number}`"
            data-testid="suggested-printing"
            @select="choose(suggestion.card)"
          />
        </div>
      </section>

      <PrintingPickerGrid
        v-model:filter="picker.filter.value"
        v-model:collection-only="picker.collectionOnly.value"
        class="mt-4"
        :printings="picker.printings.value"
        :filtered-printings="picker.filteredPrintings.value"
        :total="picker.total.value"
        :pending="picker.isPending.value"
        :error="picker.failed.value"
        :has-more="picker.hasNextPage.value"
        :loading-more="picker.isFetchingNextPage.value"
        collection-filter
        :collection-loading="picker.collectionFilterLoading.value"
        error-message="Could not load this card's printings."
        empty-message="No printings found for this card."
        @load-more="picker.loadMore"
      >
        <template #tile="{ printing }">
          <PrintingTile
            :game="game"
            :card="printing"
            selectable
            :current="printing.id === card.id"
            :loading="changingTo === printing.id"
            :disabled="printing.id === card.id || changePrinting.isPending.value"
            :aria-label="
              printing.id === card.id
                ? `${printing.set_name} ${printing.collector_number}, current printing`
                : `Change to ${printing.set_name} ${printing.collector_number}`
            "
            @select="choose(printing)"
          >
            <template v-if="suggestion && printing.id === suggestion.card.id" #overlay>
              <span
                class="bg-background/90 text-foreground absolute top-1 left-1 z-10 inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-xs shadow"
              >
                <Tag class="size-3" aria-hidden="true" /> Cheapest
              </span>
            </template>
          </PrintingTile>
        </template>
      </PrintingPickerGrid>

      <div class="mt-5 flex justify-end">
        <DialogClose :class="buttonVariants({ variant: 'outline' })">Close</DialogClose>
      </div>
    </DialogContent>
  </Dialog>
</template>
