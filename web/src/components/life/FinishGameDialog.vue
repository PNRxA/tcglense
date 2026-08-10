<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Handshake, Trophy } from '@lucide/vue'
import { Button, buttonVariants } from '@/components/ui/button'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from '@/components/ui/dialog'
import type { LifeSeat } from '@/lib/api'
import { seatColor } from '@/lib/lifeSeries'

// Record who won.
//
// The dialog states the consequence in words before it locks anything, because finishing a game
// is the one irreversible step here: it feeds the linked decks' win records and the game stops
// accepting edits. Picking is one tap on the winner — the fast path — with a draw as an equal
// first-class option rather than something buried.
//
// A seat that is already out — no life, 21 commander damage from someone, ten poison — is
// **labelled**, never preselected or excluded. That's the deliberate half of issue #595's "does
// reaching 21 auto-suggest a result?": the counters know who can't win, but the last player
// standing still isn't reliably the winner (a pod can end in a concession), and a recorded
// result is immutable and counts towards a deck's record. So the answer is shown and the choice
// stays a person's.
const props = defineProps<{
  open: boolean
  seats: LifeSeat[]
  /** The rendered seats, for the "out — 21 commander damage" note. */
  seatViews: { seat: LifeSeat; lethal: string | null }[]
  busy?: boolean
}>()
const emit = defineEmits<{
  'update:open': [value: boolean]
  finish: [winnerPlayerId: number | null]
}>()

const choice = ref<number | 'draw' | null>(null)

watch(
  () => props.open,
  (open) => {
    if (!open) return
    // Preselect nothing: the last player standing is usually the winner, but "usually" is not
    // good enough for a record you'll read back weeks later.
    choice.value = null
  },
)

/** Highest life first — the likely winner is the easiest to reach. */
const ranked = computed(() => [...props.seats].sort((a, b) => b.life - a.life))

/** Why a seat is out, by seat id — so a 21-damage kill reads as one rather than as full life. */
const lethalById = computed(
  () => new Map(props.seatViews.map((view) => [view.seat.id, view.lethal])),
)

const linkedDecks = computed(() => props.seats.filter((seat) => seat.deck_id !== null).length)

const consequence = computed(() => {
  if (linkedDecks.value === 0) {
    return 'No players are linked to a deck, so this only records the game itself.'
  }
  const plural = linkedDecks.value === 1 ? 'deck' : 'decks'
  return `This adds a result to ${linkedDecks.value} linked ${plural} and locks the game — life totals can't change afterwards.`
})

function submit() {
  if (choice.value === null) return
  emit('finish', choice.value === 'draw' ? null : choice.value)
}
</script>

<template>
  <Dialog :open="open" @update:open="(value: boolean) => emit('update:open', value)">
    <DialogContent
      class="bg-background max-h-[85dvh] w-[min(92vw,26rem)] overflow-y-auto rounded-xl border p-6 shadow-xl"
    >
      <DialogTitle>Finish game</DialogTitle>
      <DialogDescription>Who won?</DialogDescription>

      <div class="mt-4 space-y-2">
        <button
          v-for="seat in ranked"
          :key="seat.id"
          type="button"
          class="hover:bg-accent/50 flex w-full items-center gap-3 rounded-lg border p-3 text-left transition-colors"
          :class="choice === seat.id ? 'border-success/60 bg-success/10' : ''"
          :aria-pressed="choice === seat.id"
          @click="choice = seat.id"
        >
          <span
            class="size-2.5 shrink-0 rounded-full"
            :style="{ backgroundColor: seatColor(seat.position) }"
            aria-hidden="true"
          />
          <span class="min-w-0 flex-1">
            <span class="block truncate font-medium">{{ seat.name }}</span>
            <span
              v-if="seat.deck_name ?? seat.commander_name"
              class="text-muted-foreground block truncate text-xs"
            >
              {{ seat.deck_name ?? seat.commander_name }}
            </span>
            <!-- A seat on full life can still be dead — 21 commander damage doesn't touch the
                 number beside it, so the reason is spelled out rather than inferred. -->
            <span v-if="lethalById.get(seat.id)" class="text-destructive block truncate text-xs">
              Out — {{ lethalById.get(seat.id) }}
            </span>
          </span>
          <span class="shrink-0 text-lg font-semibold tabular-nums">{{ seat.life }}</span>
          <Trophy
            v-if="choice === seat.id"
            class="size-4 shrink-0 text-success"
            aria-hidden="true"
          />
        </button>

        <button
          type="button"
          class="hover:bg-accent/50 flex w-full items-center gap-3 rounded-lg border p-3 text-left transition-colors"
          :class="choice === 'draw' ? 'border-ring bg-accent/60' : ''"
          :aria-pressed="choice === 'draw'"
          @click="choice = 'draw'"
        >
          <Handshake class="text-muted-foreground size-4 shrink-0" aria-hidden="true" />
          <span class="flex-1 font-medium">It was a draw</span>
        </button>
      </div>

      <p class="text-muted-foreground mt-4 text-sm">{{ consequence }}</p>

      <div class="mt-4 flex justify-end gap-2">
        <DialogClose :class="buttonVariants({ variant: 'ghost' })">Keep playing</DialogClose>
        <Button :disabled="choice === null || busy" @click="submit">Record result</Button>
      </div>
    </DialogContent>
  </Dialog>
</template>
