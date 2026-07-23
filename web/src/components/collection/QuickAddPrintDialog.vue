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
import QuickAddPrintTile from '@/components/collection/QuickAddPrintTile.vue'
import PrintingPickerGrid from '@/components/printings/PrintingPickerGrid.vue'
import { usePrintingPicker } from '@/composables/usePrintings'
import { useOwnedCounts } from '@/composables/useCollection'
import { useWishlistCounts } from '@/composables/useWishlist'
import type { Card } from '@/lib/api'
import type { CardListTarget, OwnedCountSeed } from '@/composables/useOwnedCountEditor'

// Step two of quick-add: having chosen a name, pick which printing and add regular
// and/or foil copies — to the collection by default, or the wish list when `list`
// says so (#167). Opened by QuickAddBox once a name is selected. The reka dialog
// gives a focus trap, Escape-to-close, and click-outside dismissal for free.
const props = withDefaults(
  defineProps<{ game: string; name: string | null; list?: CardListTarget }>(),
  { list: 'collection' },
)
const open = defineModel<boolean>('open', { required: true })
// Forwarded to the parent so it can return focus to the quick-add box on close (this
// dialog is opened programmatically, without a trigger, so reka has no element to
// restore focus to and would otherwise drop it to <body>).
const emit = defineEmits<{ closeAutoFocus: [Event] }>()

const game = toRef(props, 'game')
const name = computed(() => props.name ?? '')

// Fetch printings only while the dialog is open. The shared picker accumulates 200-card
// pages and owns the loaded-page filter state, so even 800+ basic-land printings remain
// reachable without claiming the filter searched pages that have not been loaded.
const picker = usePrintingPicker(game, name, { enabled: open })

// Authoritative counts for every printing, refetched on each open (staleTime 0) so the
// absolute-count editors seed off the true current holding, never a stale one — from
// the collection or the wish list per the target (fixed per instance, so picking the
// hook once at setup is safe). Keyed on the full `prints` list (not the filtered view)
// so typing in the filter box never refetches. Gate on `ready && !fetching`, not
// `ready` alone: reopening the SAME name reuses the query key, so `ready` stays true
// off the retained (possibly stale) cache while the staleTime-0 refetch runs — seeding
// an editor then, and saving before it settles, would clobber the true count (mirrors
// OwnedCountControl's guard).
const { ownership, ready, fetching } =
  props.list === 'wishlist'
    ? useWishlistCounts(game, picker.printings, { enabled: open, staleTime: 0 })
    : useOwnedCounts(game, picker.printings, { enabled: open, staleTime: 0 })
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
      class="bg-background flex max-h-[85vh] w-[min(94vw,44rem)] flex-col overflow-hidden rounded-xl border p-6 shadow-xl"
      @close-auto-focus="emit('closeAutoFocus', $event)"
    >
      <DialogTitle class="text-lg font-semibold">
        Add <span class="text-primary">{{ name }}</span>
      </DialogTitle>
      <DialogDescription class="text-muted-foreground mt-1 text-sm">
        Pick a printing, then add regular or foil copies to your
        {{ list === 'wishlist' ? 'wish list' : 'collection' }}.
      </DialogDescription>

      <!-- The grid fills the dialog and scrolls on its own (scrollable), so the title above,
        the filter/sort bar, and the Done button below all stay pinned — a long printing list
        never buries them off-screen. -->
      <PrintingPickerGrid
        v-model:filter="picker.filter.value"
        scrollable
        class="mt-4 min-h-0 flex-1"
        :printings="picker.printings.value"
        :filtered-printings="picker.filteredPrintings.value"
        :total="picker.total.value"
        :pending="picker.isPending.value"
        :error="picker.failed.value"
        :has-more="picker.hasNextPage.value"
        :loading-more="picker.isFetchingNextPage.value"
        error-message="Couldn't load printings. Please close and try again."
        empty-message="No printings found for this name."
        @load-more="picker.loadMore"
      >
        <template #tile="{ printing }">
          <QuickAddPrintTile
            :game="game"
            :card="printing"
            :seed="seedFor(printing)"
            :ready="seedReady"
            :list="list"
          />
        </template>
      </PrintingPickerGrid>

      <div class="mt-6 flex justify-end">
        <DialogClose :class="buttonVariants({ variant: 'outline' })">Done</DialogClose>
      </div>
    </DialogContent>
  </Dialog>
</template>
