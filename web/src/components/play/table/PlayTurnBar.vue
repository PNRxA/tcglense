<script setup lang="ts">
import { computed } from 'vue'
import { LogOut, PlusSquare, RotateCcw, Shuffle, SkipForward, Sparkles } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import PlayDiceMenu from '@/components/play/table/PlayDiceMenu.vue'
import PlayTableMenu from '@/components/play/table/PlayTableMenu.vue'
import { usePlayTableContext } from '@/composables/usePlayTable'
import { PLAY_PHASES, seatColor } from '@/lib/playTable'
import type { PlayPhase } from '@/lib/api/play'

// The strip across the top: whose turn it is, and the handful of verbs that belong to the
// turn rather than to a card.
//
// **Phases are a shared pointer, not a rule.** Nothing is enforced by phase — this is paper
// Magic — so the chips exist to tell four people who are not in the same room where the turn
// is. Only the active seat can move the pointer, because otherwise it is just noise.
//
// "Pass turn" is the one primary button on the whole table, and only while it is actually my
// turn; the rest are outline, because pressing them at the wrong moment is a nuisance, not a
// mistake.
//
// Once the game is **finished** every verb here goes flat (`table.canAct`) and the banner
// takes over. The log and chat beside it stay live on purpose: the minute after a game ends is
// when a pod actually talks about it.
const emit = defineEmits<{ leave: [] }>()

const table = usePlayTableContext()

const turn = computed(() => table.store.turn)
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
      return { label: 'Live', tone: 'bg-success/15 text-success' }
    case 'connecting':
    case 'reconnecting':
      return { label: 'Reconnecting', tone: 'bg-warning/15 text-warning' }
    default:
      return { label: 'Offline', tone: 'bg-destructive/15 text-destructive' }
  }
})

function setPhase(phase: PlayPhase) {
  if (!table.store.isMyTurn || !table.canAct.value) return
  table.send({ type: 'set_phase', phase })
}
</script>

<template>
  <header class="bg-card flex flex-col gap-1.5 border-b px-2 py-1.5">
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

      <!-- Phase chips: a segmented pointer, live only for whoever's turn it is. -->
      <div
        class="order-last flex w-full gap-0.5 overflow-x-auto sm:order-none sm:w-auto"
        role="group"
        aria-label="Turn phase"
      >
        <button
          v-for="item in PLAY_PHASES"
          :key="item.phase"
          type="button"
          class="rounded px-1.5 py-0.5 text-[0.7rem] whitespace-nowrap transition-colors"
          :class="
            turn?.phase === item.phase
              ? 'bg-primary text-primary-foreground'
              : 'text-muted-foreground hover:bg-accent'
          "
          :aria-pressed="turn?.phase === item.phase"
          :disabled="!table.store.isMyTurn || !table.canAct.value"
          @click="setPhase(item.phase)"
        >
          {{ item.label }}
        </button>
      </div>

      <div class="ml-auto flex flex-wrap items-center gap-1.5">
        <Button variant="outline" size="sm" :disabled="!table.canAct.value" @click="table.draw(1)">
          <Sparkles class="size-4" /><span class="hidden sm:inline">Draw</span>
        </Button>
        <Button
          variant="outline"
          size="sm"
          :disabled="!table.canAct.value"
          @click="table.untapAll()"
        >
          <RotateCcw class="size-4" /><span class="hidden sm:inline">Untap all</span>
        </Button>
        <Button
          variant="outline"
          size="sm"
          :disabled="!table.canAct.value"
          @click="table.shuffle()"
        >
          <Shuffle class="size-4" /><span class="hidden sm:inline">Shuffle</span>
        </Button>
        <Button
          variant="outline"
          size="sm"
          :disabled="!table.canAct.value"
          @click="table.tokenOpen.value = true"
        >
          <PlusSquare class="size-4" /><span class="hidden sm:inline">Token</span>
        </Button>
        <PlayDiceMenu />
        <Button
          :variant="table.store.isMyTurn ? 'default' : 'outline'"
          size="sm"
          :disabled="!table.store.isMyTurn || !table.canAct.value"
          @click="table.passTurn()"
        >
          <SkipForward class="size-4" />Pass turn
        </Button>
        <span
          class="rounded-full px-2 py-0.5 text-[0.7rem] font-medium"
          :class="connection.tone"
          role="status"
          >{{ connection.label }}</span
        >
        <PlayTableMenu />
        <Button variant="ghost" size="icon-sm" aria-label="Leave the table" @click="emit('leave')">
          <LogOut class="size-4" />
        </Button>
      </div>
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
