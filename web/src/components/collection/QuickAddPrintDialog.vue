<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
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

// Held-first ordering: a card you're quick-adding is usually one you already have a copy of,
// and its printings can run into the hundreds — so the ones on the target list lead the grid
// instead of being hunted for behind a set filter. It rides the counts fetched above, at no
// extra request.
//
// The grid is ordered off *this set*, never off those live counts, because the counts are the
// very thing the tiles edit. Each printing's held-ness is decided **once**, the first time its
// count is authoritative, and never revised: the set only grows, as later pages resolve. That
// is what keeps the grid still. Ordering off the live map instead would move it three ways —
// a `+` click would float the tile out from under the pointer ~350ms later (the editor's
// debounce), and every counts *refetch* (a save's invalidation, a "Load more" widening the
// batch key, a window refocus at `staleTime: 0`) would blank the map mid-session, snapping the
// whole grid to newest-first and back. So the opening order is the answer to "what did I
// already own when I opened this", which is the question being asked.
const heldPrintings = ref<ReadonlySet<string>>(new Set())
// Ids already decided, so a refetch can't re-judge one. Plain (non-reactive) — nothing renders
// off it; `heldPrintings` is reassigned when it actually changes, which is what re-sorts.
const decided = new Set<string>()
// A fresh picker (another name, another game, or simply reopening) re-snapshots, so copies
// added last time lead the next one.
let session = ''

watch(
  [game, name, open, seedReady, picker.printings],
  () => {
    const key = JSON.stringify([game.value, name.value, open.value])
    if (key !== session) {
      session = key
      decided.clear()
      heldPrintings.value = new Set()
    }
    if (!open.value || !seedReady.value) return
    let next: Set<string> | null = null
    for (const card of picker.printings.value) {
      if (decided.has(card.id)) continue
      decided.add(card.id)
      const counts = ownership.value[card.id]
      if (!counts || counts.quantity + counts.foil_quantity <= 0) continue
      next ??= new Set(heldPrintings.value)
      next.add(card.id)
    }
    if (next) heldPrintings.value = next
  },
  { immediate: true },
)

const heldFirstLabel = computed(() =>
  props.list === 'wishlist' ? 'On my wish list first' : 'Owned first',
)
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
        held-first
        :held-first-label="heldFirstLabel"
        :held="heldPrintings"
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
