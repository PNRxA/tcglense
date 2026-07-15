<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { Check, Loader2 } from '@lucide/vue'
import CardImage from '@/components/cards/CardImage.vue'
import { buttonVariants } from '@/components/ui/button'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from '@/components/ui/dialog'
import { useChangeDeckCardPrintingMutation } from '@/composables/useDecks'
import { useCardPrintingsByName } from '@/composables/useQuickAdd'
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
const printingsQuery = useCardPrintingsByName(game, cardName, { enabled })
const printings = computed(() => printingsQuery.data.value?.data ?? [])
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
      <div v-if="printingsQuery.isPending.value" class="text-muted-foreground flex py-12">
        <Loader2 class="mr-2 size-4 animate-spin" /> Loading printings…
      </div>
      <p v-else-if="printingsQuery.isError.value" class="text-muted-foreground py-12 text-sm">
        Could not load this card's printings.
      </p>
      <div v-else class="mt-4 grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5">
        <button
          v-for="printing in printings"
          :key="printing.id"
          type="button"
          class="focus-visible:ring-ring relative rounded-lg border p-1.5 text-left outline-none transition hover:border-primary/50 focus-visible:ring-2 disabled:cursor-default"
          :class="printing.id === card.id ? 'border-primary bg-primary/5' : ''"
          :disabled="printing.id === card.id || changePrinting.isPending.value"
          :aria-label="
            printing.id === card.id
              ? `${printing.set_name} ${printing.collector_number}, current printing`
              : `Change to ${printing.set_name} ${printing.collector_number}`
          "
          @click="choose(printing)"
        >
          <div class="relative">
            <CardImage
              :game="game"
              :id="printing.id"
              :name="printing.name"
              :has-image="printing.has_image"
              size="normal"
              class="w-full rounded-md"
            />
            <span
              v-if="printing.id === card.id"
              class="bg-primary text-primary-foreground absolute right-1 bottom-1 flex items-center gap-1 rounded-md px-1.5 py-0.5 text-xs shadow"
            >
              <Check class="size-3" /> Current
            </span>
            <span
              v-else-if="changingTo === printing.id"
              class="bg-background/90 absolute right-1 bottom-1 rounded-full p-1.5 shadow"
            >
              <Loader2 class="size-4 animate-spin" />
            </span>
          </div>
          <p class="mt-1.5 truncate text-xs font-medium" :title="printing.set_name">
            {{ printing.set_name }}
          </p>
          <p class="text-muted-foreground text-xs">
            {{ printing.set_code.toUpperCase() }} · #{{ printing.collector_number }}
          </p>
        </button>
      </div>

      <div class="mt-5 flex justify-end">
        <DialogClose :class="buttonVariants({ variant: 'outline' })">Close</DialogClose>
      </div>
    </DialogContent>
  </Dialog>
</template>
