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
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import QuickAddPrintTile from '@/components/collection/QuickAddPrintTile.vue'
import { useCardPrintingsByName } from '@/composables/useQuickAdd'
import { useOwnedCounts } from '@/composables/useCollection'
import { useWishlistCounts } from '@/composables/useWishlist'
import { filterPrintings } from '@/lib/quickAddFilter'
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

// Fetch printings only while the dialog is open (so picking a name is what triggers
// it), newest printing first.
const printsQuery = useCardPrintingsByName(game, name, { enabled: open })
const prints = computed<Card[]>(() => printsQuery.data.value?.data ?? [])

// Client-side filter over the loaded printings — a card can have many printings, so a
// box to narrow by set name/code (e.g. "TLA"), collector number (e.g. "#2672" or
// "2672"), rarity, or language makes the right one quick to find. Space-separated
// tokens are ANDed. Reset whenever the dialog (re)opens for a new name.
const filter = ref('')
watch(open, (isOpen) => {
  if (isOpen) filter.value = ''
})
const filteredPrints = computed<Card[]>(() => filterPrintings(prints.value, filter.value))

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
    ? useWishlistCounts(game, prints, { enabled: open, staleTime: 0 })
    : useOwnedCounts(game, prints, { enabled: open, staleTime: 0 })
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
        Pick a printing, then add regular or foil copies to your
        {{ list === 'wishlist' ? 'wish list' : 'collection' }}.
      </DialogDescription>

      <div class="mt-4">
        <LoadingRow v-if="printsQuery.isPending.value" label="Loading printings…" />
        <p v-else-if="printsQuery.isError.value" class="text-destructive py-8 text-center text-sm">
          Couldn't load printings. Please close and try again.
        </p>
        <p v-else-if="!prints.length" class="text-muted-foreground py-8 text-center text-sm">
          No printings found for this name.
        </p>
        <template v-else>
          <!-- Filter + count. Only worth showing once there's more than one printing. -->
          <div
            v-if="prints.length > 1"
            class="mb-4 flex flex-wrap items-center justify-between gap-2"
          >
            <CardSearchBox
              v-model="filter"
              class="w-full sm:w-72"
              placeholder="Filter by set, number, or rarity…"
              aria-label="Filter printings by set, number, or rarity"
            />
            <p class="text-muted-foreground shrink-0 text-xs">
              {{ filteredPrints.length }} of {{ prints.length }} printings
            </p>
          </div>

          <p v-if="!filteredPrints.length" class="text-muted-foreground py-8 text-center text-sm">
            No printings match “{{ filter.trim() }}”.
          </p>
          <div v-else class="grid grid-cols-1 gap-4 sm:grid-cols-2 sm:gap-5">
            <QuickAddPrintTile
              v-for="card in filteredPrints"
              :key="card.id"
              :game="game"
              :card="card"
              :seed="seedFor(card)"
              :ready="seedReady"
              :list="list"
            />
          </div>
        </template>
      </div>

      <div class="mt-6 flex justify-end">
        <DialogClose :class="buttonVariants({ variant: 'outline' })">Done</DialogClose>
      </div>
    </DialogContent>
  </Dialog>
</template>
