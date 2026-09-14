<script setup lang="ts">
import { computed } from 'vue'
import {
  LogOut,
  MessageSquare,
  Minus,
  Plus,
  PlusSquare,
  RotateCcw,
  Shuffle,
  SkipForward,
  Sparkles,
} from '@lucide/vue'
import { Button } from '@/components/ui/button'
import PlayDiceMenu from '@/components/play/table/PlayDiceMenu.vue'
import PlayPhaseChips from '@/components/play/table/PlayPhaseChips.vue'
import PlayTableMenu from '@/components/play/table/PlayTableMenu.vue'
import { usePlayTableContext } from '@/composables/usePlayTable'
import { seatColor } from '@/lib/playTable'

// The strip across the top: whose turn it is, and the handful of verbs that belong to the turn
// rather than to a card.
//
// **One row on a phone.** The desktop bar is five labelled buttons, a dice menu, "Pass turn", a
// status pill and two icons — which on a 390px screen wrapped to three rows and pushed the
// board halfway down the screen. So below `sm` the labels go (the icons keep their
// `aria-label`s), "Pass turn" becomes "Pass", the connection pill becomes a dot, and the phase
// chips collapse to the current phase (`PlayPhaseChips`). The life total rides the bar as a
// chip with ±1 on it, because life is the number you look at constantly and the full panel is
// a sheet away.
//
// "Pass turn" is the one primary button on the whole table, and only while it is actually my
// turn; the rest are outline, because pressing them at the wrong moment is a nuisance, not a
// mistake.
//
// Once the game is **finished** every verb here goes flat (`table.canAct`) and the banner takes
// over. The log and chat beside it stay live on purpose: the minute after a game ends is when a
// pod actually talks about it.
const emit = defineEmits<{ leave: [] }>()

const table = usePlayTableContext()

const turn = computed(() => table.store.turn)
/** The one-row arrangement: a narrow screen, or a short one with no rows to spare. */
const phone = computed(() => table.compact.value)
/**
 * Whether the compact bar needs a *second* row for the phases and the way out.
 *
 * Only a narrow screen does. A phone held sideways is 844px wide — there is width for the
 * collapsed phase chip inline, and at 390px of total height a second bar row is 20px the
 * battlefield cannot spare.
 */
const twoRows = computed(() => table.isPhone.value)
const seat = computed(() => table.store.mySeat)
const activeSeat = computed(() => {
  const id = turn.value?.active_seat
  return id === null || id === undefined ? null : (table.store.seatById(id) ?? null)
})
const winner = computed(() => {
  const id = table.store.winner
  return id === null ? null : (table.store.seatById(id) ?? null)
})
const finished = computed(() => table.store.status === 'finished')

const connection = computed(() => {
  switch (table.store.connection) {
    case 'open':
      return { label: 'Live', tone: 'bg-success/15 text-success', dot: 'bg-success' }
    case 'connecting':
    case 'reconnecting':
      return { label: 'Reconnecting', tone: 'bg-warning/15 text-warning', dot: 'bg-warning' }
    default:
      return { label: 'Offline', tone: 'bg-destructive/15 text-destructive', dot: 'bg-destructive' }
  }
})

/** The four table verbs that are icon-only on a phone and labelled everywhere else. */
const verbs = computed(() => [
  { id: 'draw', label: 'Draw', icon: Sparkles, run: () => table.draw(1) },
  { id: 'untap', label: 'Untap all', icon: RotateCcw, run: () => table.untapAll() },
  { id: 'shuffle', label: 'Shuffle', icon: Shuffle, run: () => table.shuffle() },
  {
    id: 'token',
    label: 'Token',
    icon: PlusSquare,
    run: () => {
      table.tokenOpen.value = true
    },
  },
])

function bumpLife(delta: number) {
  if (!table.canAct.value) return
  table.send({ type: 'life', delta })
}
</script>

<template>
  <header class="bg-card flex flex-col gap-1 border-b px-2 py-1.5">
    <div class="flex flex-wrap items-center gap-1.5">
      <div class="mr-1 min-w-0">
        <p class="truncate text-sm font-medium">
          {{ table.store.room?.label || 'Table' }}
          <span class="text-muted-foreground font-mono text-xs">{{ table.store.code }}</span>
        </p>
        <p class="text-muted-foreground truncate text-xs">
          <template v-if="turn && turn.number > 0">Turn {{ turn.number }}</template>
          <template v-if="activeSeat">
            ·
            <span
              class="mr-1 inline-block size-1.5 rounded-full align-middle"
              :style="{ backgroundColor: seatColor(activeSeat.seat_index) }"
              aria-hidden="true"
            />{{ activeSeat.name }}
          </template>
        </p>
      </div>

      <!-- On a phone the phases sit on the second short row with the piles; on a laptop they
        run inline here, where there is width to spare. -->
      <PlayPhaseChips v-if="!twoRows" class="order-last w-full sm:order-none sm:w-auto" />

      <div class="ml-auto flex items-center gap-1 sm:flex-wrap sm:gap-1.5">
        <!-- Life, on the bar itself: the number you look at every few seconds, with the ±1 you
          press most. Everything else about your totals is one tap away in the sheet. -->
        <div
          v-if="phone && seat"
          class="bg-muted/60 flex shrink-0 items-center rounded-full"
          role="group"
          aria-label="Your life"
        >
          <Button
            variant="ghost"
            size="icon-sm"
            class="rounded-full"
            :disabled="!table.canAct.value"
            aria-label="Lose a life"
            @click="bumpLife(-1)"
          >
            <Minus class="size-3.5" />
          </Button>
          <button
            type="button"
            class="min-w-7 text-center text-sm font-semibold tabular-nums"
            :class="seat.life <= 0 ? 'text-destructive' : seat.life <= 5 ? 'text-warning' : ''"
            :aria-label="`Your totals — ${seat.life} life`"
            @click="table.lifeOpen.value = true"
          >
            {{ seat.life }}
          </button>
          <Button
            variant="ghost"
            size="icon-sm"
            class="rounded-full"
            :disabled="!table.canAct.value"
            aria-label="Gain a life"
            @click="bumpLife(1)"
          >
            <Plus class="size-3.5" />
          </Button>
        </div>

        <Button
          v-for="verb in verbs"
          :key="verb.id"
          variant="outline"
          :size="phone ? 'icon-sm' : 'sm'"
          :disabled="!table.canAct.value"
          :aria-label="verb.label"
          @click="verb.run()"
        >
          <component :is="verb.icon" class="size-4" />
          <span v-if="!phone">{{ verb.label }}</span>
        </Button>
        <PlayDiceMenu />
        <Button
          :variant="table.store.isMyTurn ? 'default' : 'outline'"
          size="sm"
          :class="phone ? 'px-2' : ''"
          :disabled="!table.store.isMyTurn || !table.canAct.value"
          @click="table.passTurn()"
        >
          <SkipForward class="size-4" />{{ phone ? 'Pass' : 'Pass turn' }}
        </Button>

        <!-- The status, as a dot where there is no room for a word. -->
        <span
          v-if="phone"
          class="size-2 shrink-0 rounded-full"
          :class="connection.dot"
          role="status"
          :aria-label="connection.label"
        />
        <span
          v-else
          class="rounded-full px-2 py-0.5 text-[0.7rem] font-medium"
          :class="connection.tone"
          role="status"
          >{{ connection.label }}</span
        >

        <!-- The log's way in on a phone lives here rather than floating over the hand. On the
          narrowest bar it moves to the second row with the menu, so the first row's verbs never
          push past the edge. -->
        <Button
          v-if="phone && !twoRows"
          variant="ghost"
          size="icon-sm"
          aria-label="Open the log"
          @click="table.logSheetOpen.value = true"
        >
          <MessageSquare class="size-4" />
        </Button>
        <PlayTableMenu v-if="!twoRows" />
        <Button
          v-if="!twoRows"
          variant="ghost"
          size="icon-sm"
          aria-label="Leave the table"
          @click="emit('leave')"
        >
          <LogOut class="size-4" />
        </Button>
      </div>
    </div>

    <!-- The narrow phone's second short row: where the turn is, and the way out. -->
    <div v-if="twoRows" class="flex items-center gap-1.5">
      <PlayPhaseChips class="min-w-0 flex-1" />
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label="Open the log"
        @click="table.logSheetOpen.value = true"
      >
        <MessageSquare class="size-4" />
      </Button>
      <PlayTableMenu />
      <Button variant="ghost" size="icon-sm" aria-label="Leave the table" @click="emit('leave')">
        <LogOut class="size-4" />
      </Button>
    </div>

    <p
      v-if="finished || winner"
      class="bg-success/15 text-success rounded px-2 py-1 text-center text-sm font-medium"
      role="status"
    >
      Game over — {{ winner ? `${winner.name} won` : 'no winner recorded' }}
    </p>
  </header>
</template>
