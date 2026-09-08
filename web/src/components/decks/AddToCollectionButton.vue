<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { RouterLink } from 'vue-router'
import { Library } from '@lucide/vue'
import { Button, buttonVariants } from '@/components/ui/button'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import { ApiError } from '@/lib/api'
import type { CollectionAddSummary } from '@/lib/api'

// "Add to collection" for a whole deck — the owner's deck page and the precon page share it,
// so "I bought this" reads and behaves the same on both. The button opens a confirmation
// (the write is additive and not idempotent: a second click records a second copy of every
// card, so the dialog says so before anything is sent), runs the caller's `submit`, and then
// shows what landed in place of the confirmation, with a link into the collection.
//
// The write itself belongs to the caller (`submit`): the deck page fires the deck mutation,
// the precon page the precon one, and this component never knows which — only how many
// copies the confirmation names and how the caller words what those copies are (a deck
// leaves its maybeboards out; a precon has none).
const props = defineProps<{
  /** Copies the confirmation names — the deck proper plus its sideboard, or every board of
   * a precon. The caller hides the button when this is zero. */
  copies: number
  /** The game slug, for the "View collection" link after a success. */
  game: string
  /** One sentence saying which cards these are, worded per surface. */
  note: string
  /** Performs the write; resolves to what was added, or throws an `ApiError`. */
  submit: () => Promise<CollectionAddSummary>
}>()

const open = ref(false)
const pending = ref(false)
const error = ref('')
const result = ref<CollectionAddSummary | null>(null)

// A reopened dialog is a fresh question: clear last time's answer or failure.
watch(open, (isOpen) => {
  if (!isOpen) return
  error.value = ''
  result.value = null
})

const plural = (n: number, noun: string) => `${n} ${noun}${n === 1 ? '' : 's'}`

const title = computed(() => `Add ${plural(props.copies, 'card')} to your collection?`)

/** "Added 100 cards (99 regular, 1 foil) across 87 printings." — the numbers the response
 * carries, worded so a foil count of zero doesn't clutter the common case. */
const resultText = computed(() => {
  const added = result.value
  if (!added) return ''
  const copies = added.regular_copies + added.foil_copies
  const finishes =
    added.foil_copies > 0 ? ` (${added.regular_copies} regular, ${added.foil_copies} foil)` : ''
  return `Added ${plural(copies, 'card')}${finishes} across ${plural(added.cards, 'printing')}.`
})

const skippedText = computed(() => {
  const skipped = result.value?.skipped_cards ?? 0
  if (skipped === 0) return ''
  return `${plural(skipped, 'card')} couldn't be added because ${
    skipped === 1 ? 'it is' : 'they are'
  } no longer in the catalog.`
})

async function confirm() {
  if (pending.value) return
  pending.value = true
  error.value = ''
  try {
    result.value = await props.submit()
  } catch (caught) {
    error.value =
      caught instanceof ApiError
        ? caught.message
        : 'The cards could not be added. Please try again.'
  } finally {
    pending.value = false
  }
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogTrigger as-child>
      <Button variant="outline" size="sm">
        <Library class="size-4" aria-hidden="true" /> Add to collection
      </Button>
    </DialogTrigger>
    <DialogContent class="bg-background w-[min(92vw,26rem)] rounded-xl border p-6 shadow-xl">
      <template v-if="result">
        <DialogTitle>Added to your collection</DialogTitle>
        <DialogDescription class="text-muted-foreground mt-1 text-sm">{{
          resultText
        }}</DialogDescription>
        <p v-if="skippedText" class="text-warning mt-2 text-sm" aria-live="polite">
          {{ skippedText }}
        </p>
        <div class="mt-4 flex justify-end gap-2">
          <RouterLink
            :class="buttonVariants({ variant: 'outline' })"
            :to="`/collection/${game}`"
            data-testid="view-collection"
          >
            View collection
          </RouterLink>
          <DialogClose :class="buttonVariants()">Done</DialogClose>
        </div>
      </template>
      <template v-else>
        <DialogTitle>{{ title }}</DialogTitle>
        <DialogDescription class="text-muted-foreground mt-1 text-sm">
          {{ note }} They are added on top of what you already own, so adding the same deck twice
          records two copies of everything.
        </DialogDescription>
        <p v-if="error" class="text-destructive mt-2 text-sm" aria-live="polite">{{ error }}</p>
        <div class="mt-4 flex justify-end gap-2">
          <DialogClose :class="buttonVariants({ variant: 'ghost' })" :disabled="pending">
            Cancel
          </DialogClose>
          <Button :disabled="pending" data-testid="confirm-add" @click="confirm">
            {{ pending ? 'Adding…' : 'Add to collection' }}
          </Button>
        </div>
      </template>
    </DialogContent>
  </Dialog>
</template>
