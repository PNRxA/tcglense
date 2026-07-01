<script setup lang="ts">
import { computed, toRef } from 'vue'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from '@/components/ui/dialog'
import { buttonVariants } from '@/components/ui/button'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import QuickAddPrintRow from '@/components/collection/QuickAddPrintRow.vue'
import { useCardPrintingsByName } from '@/composables/useQuickAdd'
import { useOwnedCounts } from '@/composables/useCollection'
import type { Card } from '@/lib/api'
import type { OwnedCountSeed } from '@/composables/useOwnedCountEditor'

// Step two of quick-add: having chosen a name, pick which printing and add regular
// and/or foil copies. Opened by QuickAddBox once a name is selected. The reka dialog
// gives a focus trap, Escape-to-close, and click-outside dismissal for free.
const props = defineProps<{ game: string; name: string | null }>()
const open = defineModel<boolean>('open', { required: true })
// Forwarded to the parent so it can return focus to the quick-add box on close (this
// dialog is opened programmatically, without a trigger, so reka has no element to
// restore focus to and would otherwise drop it to <body>).
const emit = defineEmits<{ closeAutoFocus: [Event] }>()

const game = toRef(props, 'game')
const name = computed(() => props.name ?? '')

// Fetch printings only while the dialog is open (so picking a name is what triggers
// it), newest printing first.
const printsQuery = useCardPrintingsByName(game, name, { enabled: open })
const prints = computed<Card[]>(() => printsQuery.data.value?.data ?? [])

// Authoritative owned counts for every printing, refetched on each open (staleTime 0)
// so the absolute-count editors seed off the true current holding, never a stale one.
// Gate on `ready && !fetching`, not `ready` alone: reopening the SAME name reuses the
// query key, so `ready` stays true off the retained (possibly stale) cache while the
// staleTime-0 refetch runs — seeding an editor then, and saving before it settles,
// would clobber the true count (mirrors OwnedCountControl's `isSuccess && !isFetching`).
const { ownership, ready, fetching } = useOwnedCounts(game, prints, {
  enabled: open,
  staleTime: 0,
})
const seedReady = computed(() => ready.value && !fetching.value)
function seedFor(card: Card): OwnedCountSeed | undefined {
  return seedReady.value
    ? (ownership.value[card.id] ?? { quantity: 0, foil_quantity: 0 })
    : undefined
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent
      class="bg-background max-h-[85vh] w-[min(94vw,44rem)] overflow-y-auto rounded-xl border p-6 shadow-xl"
      @close-auto-focus="emit('closeAutoFocus', $event)"
    >
      <DialogTitle class="text-lg font-semibold">
        Add <span class="text-primary">{{ name }}</span>
      </DialogTitle>
      <DialogDescription class="text-muted-foreground mt-1 text-sm">
        Pick a printing, then add regular or foil copies to your collection.
      </DialogDescription>

      <div class="mt-4">
        <LoadingRow v-if="printsQuery.isPending.value" label="Loading printings…" />
        <p v-else-if="printsQuery.isError.value" class="text-destructive py-8 text-center text-sm">
          Couldn't load printings. Please close and try again.
        </p>
        <p v-else-if="!prints.length" class="text-muted-foreground py-8 text-center text-sm">
          No printings found for this name.
        </p>
        <ul v-else class="divide-border divide-y">
          <li v-for="card in prints" :key="card.id" class="py-3 first:pt-0 last:pb-0">
            <QuickAddPrintRow :game="game" :card="card" :seed="seedFor(card)" :ready="seedReady" />
          </li>
        </ul>
      </div>

      <div class="mt-6 flex justify-end">
        <DialogClose :class="buttonVariants({ variant: 'outline' })">Done</DialogClose>
      </div>
    </DialogContent>
  </Dialog>
</template>
