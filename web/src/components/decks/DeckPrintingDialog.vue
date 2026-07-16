<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
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
import { ApiError, type Card } from '@/lib/api'

const props = defineProps<{
  game: string
  deckId: number
  sectionId: number
  card: Card
  quantity: number
  foilQuantity: number
}>()
const open = defineModel<boolean>('open', { default: false })
const game = toRef(props, 'game')
const cardName = computed(() => props.card.name)
const enabled = computed(() => open.value)
const picker = usePrintingPicker(game, cardName, { enabled })
const changePrinting = useChangeDeckCardPrintingMutation()
const changingTo = ref('')
const errorMessage = ref('')

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
    <DialogContent
      class="bg-background max-h-[90vh] w-[min(94vw,64rem)] max-w-5xl overflow-y-auto rounded-xl border p-6 shadow-xl"
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
      <PrintingPickerGrid
        v-model:filter="picker.filter.value"
        class="mt-4"
        :printings="picker.printings.value"
        :filtered-printings="picker.filteredPrintings.value"
        :total="picker.total.value"
        :pending="picker.isPending.value"
        :error="picker.failed.value"
        :has-more="picker.hasNextPage.value"
        :loading-more="picker.isFetchingNextPage.value"
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
          />
        </template>
      </PrintingPickerGrid>

      <div class="mt-5 flex justify-end">
        <DialogClose :class="buttonVariants({ variant: 'outline' })">Close</DialogClose>
      </div>
    </DialogContent>
  </Dialog>
</template>
