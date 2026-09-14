<script setup lang="ts">
import { computed, ref } from 'vue'
import { MoreHorizontal } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { usePlayTableContext } from '@/composables/usePlayTable'
import { seatColor } from '@/lib/playTable'

// The three verbs that end or re-point a game, kept out of the turn bar's main run.
//
// They share an overflow menu rather than sitting beside "Draw" because each of them is
// something you do **once**, and two of them are irreversible. Putting them next to the
// buttons you press a hundred times a game is how a mis-tap ends someone's evening.
//
// Two rules decide what the menu even offers, and they are computed rather than marked up so
// they can be asserted directly:
//
// - **Concede is yours.** Any seated player may scoop; a spectator has nothing to concede.
// - **Ending the game and moving the active seat are the host's.** The engine answers
//   `HostOnly` to anyone else, so offering them would be offering a button that can only fail.
//
// Concede is the only one that confirms, because it is the only one you can reach in one
// press from a live game — the other two already sit behind a dialog that makes you choose
// something before anything is sent.
const table = usePlayTableContext()
const store = table.store

/** Which menu item ids are on offer, in order. Rendered from, and asserted against. */
interface PlayTableMenuItem {
  id: 'concede' | 'end_game' | 'set_active'
  label: string
  destructive?: boolean
}

const items = computed<PlayTableMenuItem[]>(() => {
  const out: PlayTableMenuItem[] = []
  if (store.mySeatId !== null) out.push({ id: 'concede', label: 'Concede', destructive: true })
  if (store.isHost) {
    out.push({ id: 'end_game', label: 'End game…' })
    out.push({ id: 'set_active', label: 'Set active player…' })
  }
  return out
})

/** The host's items are a separate run in the menu, so the separator knows where to go. */
const hostFrom = computed(() => items.value.findIndex((item) => item.id !== 'concede'))

const confirmingConcede = ref(false)
const endingGame = ref(false)
const settingActive = ref(false)
/** The seat picked in the end-game dialog; `null` is the explicit "no winner". */
const winner = ref<number | null>(null)

/** Open whatever the chosen menu entry leads to. Nothing is sent from here. */
function select(id: PlayTableMenuItem['id']) {
  if (id === 'concede') confirmingConcede.value = true
  if (id === 'end_game') {
    winner.value = null
    endingGame.value = true
  }
  if (id === 'set_active') settingActive.value = true
}

function concede() {
  confirmingConcede.value = false
  table.send({ type: 'concede' })
}

function endGame() {
  endingGame.value = false
  table.send({ type: 'end_game', winner: winner.value })
}

function setActive(seatId: number) {
  settingActive.value = false
  table.send({ type: 'set_active', seat: seatId })
}

defineExpose({ items, select })
</script>

<template>
  <DropdownMenu v-if="items.length > 0">
    <DropdownMenuTrigger as-child>
      <Button variant="ghost" size="icon-sm" aria-label="More table actions">
        <MoreHorizontal class="size-4" />
      </Button>
    </DropdownMenuTrigger>
    <DropdownMenuContent align="end">
      <template v-for="(item, index) in items" :key="item.id">
        <DropdownMenuSeparator v-if="hostFrom > 0 && index === hostFrom" />
        <DropdownMenuItem
          :variant="item.destructive ? 'destructive' : 'default'"
          :disabled="item.id === 'concede' && !table.canAct.value"
          @select="select(item.id)"
        >
          {{ item.label }}
        </DropdownMenuItem>
      </template>
    </DropdownMenuContent>
  </DropdownMenu>

  <!-- Conceding. One extra press, because there is no undo and the button that reaches it is
    two presses from the one you use to draw. -->
  <Dialog :open="confirmingConcede" @update:open="(value: boolean) => (confirmingConcede = value)">
    <DialogContent
      class="bg-background w-[min(92vw,24rem)] rounded-xl border p-6 shadow-xl"
      data-play-dialog="concede"
    >
      <DialogTitle>Really concede?</DialogTitle>
      <DialogDescription>
        You'll be out of this game. Nothing on the table is taken away — everyone can still see it,
        and the log stays.
      </DialogDescription>
      <div class="mt-5 flex justify-end gap-2">
        <Button variant="outline" @click="confirmingConcede = false">Keep playing</Button>
        <Button variant="destructive" data-play-confirm="concede" @click="concede">Concede</Button>
      </div>
    </DialogContent>
  </Dialog>

  <!-- Ending the game: the host records who won, which is what makes the banner honest. -->
  <Dialog :open="endingGame" @update:open="(value: boolean) => (endingGame = value)">
    <DialogContent
      class="bg-background w-[min(92vw,26rem)] rounded-xl border p-6 shadow-xl"
      data-play-dialog="end-game"
    >
      <DialogTitle>End the game</DialogTitle>
      <DialogDescription>Who won? A draw or an abandoned game has no winner.</DialogDescription>
      <div class="mt-4 space-y-1" role="radiogroup" aria-label="Winner">
        <button
          v-for="seat in store.seats"
          :key="seat.id"
          type="button"
          role="radio"
          :aria-checked="winner === seat.id"
          class="hover:bg-accent/50 flex w-full items-center gap-2 rounded-md border px-3 py-2 text-left text-sm transition-colors"
          :class="winner === seat.id ? 'border-primary bg-accent/40' : ''"
          @click="winner = seat.id"
        >
          <span
            class="size-2 shrink-0 rounded-full"
            :style="{ backgroundColor: seatColor(seat.seat_index) }"
            aria-hidden="true"
          />
          <span class="min-w-0 flex-1 truncate">{{ seat.name }}</span>
          <span class="text-muted-foreground tabular-nums">{{ seat.life }}</span>
        </button>
        <button
          type="button"
          role="radio"
          :aria-checked="winner === null"
          class="hover:bg-accent/50 w-full rounded-md border px-3 py-2 text-left text-sm transition-colors"
          :class="winner === null ? 'border-primary bg-accent/40' : ''"
          @click="winner = null"
        >
          No winner
        </button>
      </div>
      <div class="mt-5 flex justify-end gap-2">
        <Button variant="outline" @click="endingGame = false">Cancel</Button>
        <Button data-play-confirm="end-game" @click="endGame">End game</Button>
      </div>
    </DialogContent>
  </Dialog>

  <!-- Moving the active seat: for when somebody's socket dropped on their turn, or the pod
    agreed to back one up. It is the host's call because it moves everyone's turn. -->
  <Dialog :open="settingActive" @update:open="(value: boolean) => (settingActive = value)">
    <DialogContent
      class="bg-background w-[min(92vw,24rem)] rounded-xl border p-6 shadow-xl"
      data-play-dialog="set-active"
    >
      <DialogTitle>Whose turn is it?</DialogTitle>
      <DialogDescription
        >Hand the turn to a seat without passing through the others.</DialogDescription
      >
      <div class="mt-4 space-y-1">
        <button
          v-for="seat in store.seats"
          :key="seat.id"
          type="button"
          class="hover:bg-accent/50 flex w-full items-center gap-2 rounded-md border px-3 py-2 text-left text-sm transition-colors"
          :class="store.turn?.active_seat === seat.id ? 'border-primary bg-accent/40' : ''"
          @click="setActive(seat.id)"
        >
          <span
            class="size-2 shrink-0 rounded-full"
            :style="{ backgroundColor: seatColor(seat.seat_index) }"
            aria-hidden="true"
          />
          <span class="min-w-0 flex-1 truncate">{{ seat.name }}</span>
          <span v-if="store.turn?.active_seat === seat.id" class="text-muted-foreground text-xs">
            active
          </span>
        </button>
      </div>
      <div class="mt-5 flex justify-end">
        <Button variant="outline" @click="settingActive = false">Cancel</Button>
      </div>
    </DialogContent>
  </Dialog>
</template>
