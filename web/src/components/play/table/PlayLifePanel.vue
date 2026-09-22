<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Minus, Plus } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { usePlayTableContext } from '@/composables/usePlayTable'
import { seatColor } from '@/lib/playTable'

// My own totals, beside my board.
//
// Life gets the two full-height tap zones the life counter established — the thing you do a
// hundred times a game needs no aiming — plus a **quick set**, because a table where nobody
// enforces anything is a table where someone occasionally needs to type 43. The set box sends
// a *delta* (the wire has no "set life"), which is the right shape anyway: two people
// adjusting at once then compose instead of clobbering.
//
// Commander damage is read from the receiving side, one row per opponent: at a table you ask
// "how much has *their* commander hit me for", never "what's my total" — 7 from each of three
// opponents is lethal from none of them.
const props = withDefaults(
  defineProps<{
    /** Fill the container instead of taking the desktop's fixed column (the phone life sheet). */
    full?: boolean
  }>(),
  { full: false },
)

const table = usePlayTableContext()

const seat = computed(() => table.store.mySeat)
const counters = [
  { name: 'poison', label: 'Poison' },
  { name: 'energy', label: 'Energy' },
  { name: 'experience', label: 'Experience' },
] as const

const draft = ref('')
watch(
  () => seat.value?.life,
  (life) => {
    draft.value = life === undefined ? '' : String(life)
  },
  { immediate: true },
)

// Every stepper here goes through one gate: a finished game's totals are a record of how it
// ended, and a stray tap on a phone left on the table shouldn't rewrite it.
function bump(delta: number) {
  if (!table.canAct.value) return
  table.send({ type: 'life', delta })
}

function applyDraft() {
  if (!table.canAct.value) return
  const current = seat.value?.life
  const next = Number.parseInt(draft.value, 10)
  if (current === undefined || !Number.isFinite(next) || next === current) return
  bump(next - current)
}

function counterValue(name: string): number {
  return seat.value?.counters[name] ?? 0
}

function bumpCounter(name: string, delta: number) {
  if (!table.canAct.value) return
  table.send({ type: 'player_counter', name, delta })
}

function commanderDamage(fromSeat: number): number {
  return seat.value?.commander_damage[fromSeat] ?? 0
}

function bumpCommanderDamage(fromSeat: number, delta: number) {
  if (!table.canAct.value) return
  table.send({ type: 'commander_damage', from_seat: fromSeat, delta })
}

const showsCommanderDamage = computed(() => table.store.format === 'commander')
</script>

<template>
  <section
    v-if="seat"
    class="bg-card flex flex-col gap-3 overflow-y-auto rounded-lg border p-2"
    :class="props.full ? 'w-full' : 'w-40 shrink-0 sm:w-44'"
    aria-label="Your totals"
  >
    <div class="flex items-center gap-1.5">
      <span
        class="size-2 shrink-0 rounded-full"
        :style="{ backgroundColor: seatColor(seat.seat_index) }"
        aria-hidden="true"
      />
      <p class="truncate text-sm font-medium">{{ seat.name }}</p>
    </div>

    <div class="flex items-stretch gap-1">
      <Button
        variant="outline"
        size="icon"
        :disabled="!table.canAct.value"
        aria-label="Lose a life"
        @click="bump(-1)"
      >
        <Minus class="size-4" />
      </Button>
      <span
        class="flex-1 text-center text-3xl leading-9 font-semibold tabular-nums"
        :class="seat.life <= 0 ? 'text-destructive' : seat.life <= 5 ? 'text-warning' : ''"
        >{{ seat.life }}</span
      >
      <Button
        variant="outline"
        size="icon"
        :disabled="!table.canAct.value"
        aria-label="Gain a life"
        @click="bump(1)"
      >
        <Plus class="size-4" />
      </Button>
    </div>
    <span class="sr-only" aria-live="polite">{{ seat.name }}: {{ seat.life }} life</span>

    <form class="flex gap-1" @submit.prevent="applyDraft">
      <Input
        v-model="draft"
        class="h-8 text-center"
        inputmode="numeric"
        aria-label="Set life total"
      />
      <Button type="submit" variant="outline" size="sm" :disabled="!table.canAct.value">Set</Button>
    </form>

    <div class="space-y-1">
      <div v-for="counter in counters" :key="counter.name" class="flex items-center gap-1">
        <span class="min-w-0 flex-1 truncate text-xs">{{ counter.label }}</span>
        <Button
          variant="ghost"
          size="icon-sm"
          :disabled="!table.canAct.value"
          :aria-label="`Less ${counter.label.toLowerCase()}`"
          @click="bumpCounter(counter.name, -1)"
        >
          <Minus class="size-3.5" />
        </Button>
        <span class="w-5 text-center text-sm tabular-nums">{{ counterValue(counter.name) }}</span>
        <Button
          variant="ghost"
          size="icon-sm"
          :disabled="!table.canAct.value"
          :aria-label="`More ${counter.label.toLowerCase()}`"
          @click="bumpCounter(counter.name, 1)"
        >
          <Plus class="size-3.5" />
        </Button>
      </div>
    </div>

    <div v-if="showsCommanderDamage && table.store.opponents.length > 0" class="space-y-1">
      <h3 class="text-muted-foreground text-[0.65rem] tracking-wide uppercase">Commander damage</h3>
      <div
        v-for="opponent in table.store.opponents"
        :key="opponent.id"
        class="flex items-center gap-1"
      >
        <span class="min-w-0 flex-1 truncate text-xs">{{ opponent.name }}</span>
        <Button
          variant="ghost"
          size="icon-sm"
          :disabled="!table.canAct.value"
          :aria-label="`Less commander damage from ${opponent.name}`"
          @click="bumpCommanderDamage(opponent.id, -1)"
        >
          <Minus class="size-3.5" />
        </Button>
        <span
          class="w-5 text-center text-sm tabular-nums"
          :class="commanderDamage(opponent.id) >= 21 ? 'text-destructive' : ''"
          >{{ commanderDamage(opponent.id) }}</span
        >
        <Button
          variant="ghost"
          size="icon-sm"
          :disabled="!table.canAct.value"
          :aria-label="`More commander damage from ${opponent.name}`"
          @click="bumpCommanderDamage(opponent.id, 1)"
        >
          <Plus class="size-3.5" />
        </Button>
      </div>
    </div>
  </section>
</template>
