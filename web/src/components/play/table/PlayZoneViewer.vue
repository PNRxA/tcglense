<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ChevronDown, ChevronUp } from '@lucide/vue'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import PlayCard from '@/components/play/table/PlayCard.vue'
import PlayCardMenu from '@/components/play/table/PlayCardMenu.vue'
import { usePlayTableContext } from '@/composables/usePlayTable'
import { cardName, zoneLabel } from '@/lib/playTable'
import type { PlayCardView, PlayZone } from '@/lib/api/play'

// Looking through a pile — three jobs that are the same dialog.
//
// 1. **A public zone** (anyone's graveyard, exile or command zone; my own hand). A list, with
//    the owner's own cards carrying their normal context menu.
// 2. **The top of my library** (`look_top`). Order is the whole point — a scry or a Brainstorm
//    is "these three, in *this* order" — so the list is reorderable and ends in one explicit
//    "put them back in this order" (`reorder_top`). Cards can also be pulled straight out to
//    hand / battlefield / graveyard / bottom.
// 3. **Searching my library** (`search_library`). The whole library by name with a filter, each
//    card movable. It ends with a shuffle reminder rather than an automatic shuffle: the
//    engine enforces nothing, and silently reordering someone's library would be worse than
//    trusting them to press the button.
//
// A peek is private and one-shot; closing the dialog is what consumes it (`clearPeek`), which
// is why every exit funnels through `table.closeViewer()`.
const table = usePlayTableContext()

const filter = ref('')
/** The peek's cards in the order the player has arranged them (case 2). */
const ordered = ref<PlayCardView[]>([])
/**
 * Cards pulled out of this peek while it was open.
 *
 * A peek is a photograph of the library taken when the server answered; pulling a card to
 * hand moves it *now*, and the photograph is instantly wrong. That matters most for
 * `reorder_top`, which the engine refuses outright when the list names a card no longer in
 * the library — so a Brainstorm that put one card in hand and then pressed "put back in this
 * order" used to be rejected wholesale, losing the ordering the player had just done.
 */
const pulled = ref(new Set<number>())

const state = computed(() => table.viewer.value)
const peek = computed(() => table.store.peek)
const open = computed(() => state.value !== null)

const isPeek = computed(() => state.value?.kind === 'peek' && peek.value !== null)
const isLookTop = computed(() => isPeek.value && peek.value?.kind === 'look_top')

watch(
  peek,
  (next) => {
    ordered.value = next ? [...next.cards] : []
    pulled.value = new Set()
    filter.value = ''
  },
  { immediate: true },
)

const zoneCards = computed<PlayCardView[]>(() => {
  const current = state.value
  if (!current || current.kind !== 'zone') return []
  return table.store.cardsIn(current.seatId, current.zone) as PlayCardView[]
})

const searchCards = computed<PlayCardView[]>(() => {
  const needle = filter.value.trim().toLowerCase()
  const cards = [...(peek.value?.cards ?? [])].sort((a, b) =>
    cardName(a).localeCompare(cardName(b)),
  )
  const remaining = cards.filter((card) => !pulled.value.has(card.id))
  if (!needle) return remaining
  return remaining.filter((card) => cardName(card).toLowerCase().includes(needle))
})

const cards = computed<PlayCardView[]>(() => {
  if (!isPeek.value) return zoneCards.value
  return isLookTop.value ? ordered.value : searchCards.value
})

const title = computed(() => {
  const current = state.value
  if (!current) return ''
  if (current.kind === 'peek') {
    return isLookTop.value ? 'Top of your library' : 'Search your library'
  }
  const seat = table.store.seatById(current.seatId)
  const owner = seat && seat.id !== table.store.mySeatId ? `${seat.name}'s ` : 'Your '
  return `${owner}${zoneLabel(current.zone).toLowerCase()}`
})

const ownZone = computed(() => {
  const current = state.value
  return current?.kind === 'zone' && current.seatId === table.store.mySeatId
})

const viewerZone = computed<PlayZone>(() =>
  state.value?.kind === 'zone' ? state.value.zone : 'library',
)

function move(index: number, delta: number) {
  const next = [...ordered.value]
  const target = index + delta
  const card = next[index]
  const swap = next[target]
  if (!card || !swap) return
  next[index] = swap
  next[target] = card
  ordered.value = next
}

function putBack() {
  // Only the cards still on top: `ordered` has already dropped whatever was pulled out, and
  // an empty list is nothing to say at all.
  const ids = ordered.value.map((card) => card.id)
  if (ids.length > 0) table.send({ type: 'reorder_top', cards: ids })
  table.closeViewer()
}

function pull(card: PlayCardView, zone: PlayZone, placement: 'top' | 'bottom' | null = null) {
  table.moveLibraryCard(card.id, zone, {
    placement,
    x: zone === 'battlefield' ? 0.5 : null,
    y: zone === 'battlefield' ? 0.5 : null,
  })
  // It has left the library (or the top of it): drop it from both this dialog's lists, so
  // what is on screen is what a `reorder_top` may still name.
  ordered.value = ordered.value.filter((entry) => entry.id !== card.id)
  pulled.value = new Set(pulled.value).add(card.id)
}

function onOpenChange(value: boolean) {
  if (!value) table.closeViewer()
}
</script>

<template>
  <Dialog :open="open" @update:open="onOpenChange">
    <DialogContent
      anchor="top"
      class="bg-background flex max-h-[85dvh] w-[min(94vw,60rem)] flex-col rounded-xl border p-5 shadow-xl"
    >
      <DialogTitle>{{ title }}</DialogTitle>
      <DialogDescription>
        <template v-if="isLookTop">
          Put them back in the order you leave them in, or pull one out.
        </template>
        <template v-else-if="isPeek">
          Everything in your library. Take what you need, then shuffle.
        </template>
        <template v-else>
          {{ cards.length }} {{ cards.length === 1 ? 'card' : 'cards' }}.
        </template>
      </DialogDescription>

      <Input
        v-if="isPeek && !isLookTop"
        v-model="filter"
        class="mt-3"
        placeholder="Filter by name"
        aria-label="Filter by name"
      />

      <div class="mt-4 min-h-0 flex-1 overflow-y-auto">
        <p v-if="cards.length === 0" class="text-muted-foreground text-sm">Nothing here.</p>
        <ul
          v-else
          class="grid grid-cols-[repeat(auto-fill,minmax(6rem,1fr))] gap-3"
          :class="isLookTop ? 'sm:grid-cols-[repeat(auto-fill,minmax(8rem,1fr))]' : ''"
        >
          <li v-for="(card, index) in cards" :key="card.id" class="space-y-1">
            <PlayCardMenu v-if="ownZone" :card="card" :zone="viewerZone">
              <PlayCard :card="card" :game="table.store.game" size="small" />
            </PlayCardMenu>
            <PlayCard v-else :card="card" :game="table.store.game" size="small" />

            <template v-if="isPeek">
              <div v-if="isLookTop" class="flex justify-center gap-1">
                <Button
                  variant="outline"
                  size="icon-sm"
                  :disabled="index === 0"
                  :aria-label="`Move ${cardName(card)} up`"
                  @click="move(index, -1)"
                >
                  <ChevronUp class="size-4" />
                </Button>
                <Button
                  variant="outline"
                  size="icon-sm"
                  :disabled="index === cards.length - 1"
                  :aria-label="`Move ${cardName(card)} down`"
                  @click="move(index, 1)"
                >
                  <ChevronDown class="size-4" />
                </Button>
              </div>
              <div class="flex flex-wrap justify-center gap-1">
                <Button variant="ghost" size="sm" class="px-1.5" @click="pull(card, 'hand')">
                  Hand
                </Button>
                <Button variant="ghost" size="sm" class="px-1.5" @click="pull(card, 'battlefield')">
                  Play
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  class="px-1.5"
                  @click="pull(card, 'graveyard', 'top')"
                >
                  Bin
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  class="px-1.5"
                  @click="pull(card, 'library', 'bottom')"
                >
                  Bottom
                </Button>
              </div>
            </template>
          </li>
        </ul>
      </div>

      <div class="mt-4 flex flex-wrap items-center justify-end gap-2">
        <p v-if="isPeek && !isLookTop" class="text-warning mr-auto text-xs">
          Remember to shuffle when you're done.
        </p>
        <Button v-if="isPeek && !isLookTop" variant="outline" @click="table.shuffle()">
          Shuffle
        </Button>
        <Button v-if="isLookTop" @click="putBack">Put back in this order</Button>
        <Button variant="outline" @click="table.closeViewer()">Done</Button>
      </div>
    </DialogContent>
  </Dialog>
</template>
