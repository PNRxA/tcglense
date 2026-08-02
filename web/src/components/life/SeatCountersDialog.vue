<script setup lang="ts">
import { computed } from 'vue'
import { Minus, Plus, Skull } from '@lucide/vue'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button, buttonVariants } from '@/components/ui/button'
import {
  COMMANDER_DAMAGE,
  COUNTER_META,
  isLethalValue,
  type LifeCounterKind,
  type SeatCounters,
} from '@/lib/lifeCounters'
import { seatColor } from '@/lib/lifeSeries'
import type { LifeSeat } from '@/lib/api'
import type { TapTarget } from '@/composables/useLifeSession'

// One seat's counters, as steppers.
//
// **Commander damage is one row per opponent**, which is the standard matrix read from the
// receiving seat's side: at a table you ask "how much has *their* commander hit me for", never
// "what's my total commander damage" — 7 from each of three opponents is 21 and lethal from
// none of them. Rendering the whole source × target grid at once would be a table of empty cells
// on a phone; per-seat, it is the row you actually need with the seat you opened it from.
//
// A source that has **left the table** still gets a row when it dealt damage: the seat is gone,
// its damage isn't, and hiding the row would turn a scoop into missing state.
const props = defineProps<{
  open: boolean
  seat: LifeSeat | null
  /** That seat's counter state, as the server folded it. */
  counters: SeatCounters
  /** Every seat at the table, for the commander-damage rows. */
  seats: LifeSeat[]
  /** Which counters to show: the tracked ones plus any that hold a value anyway. */
  shown: LifeCounterKind[]
  /**
   * Which the game actually **tracks**. Narrower than `shown` on purpose: a counter switched
   * off keeps its recorded value and so keeps its row, but the server refuses a write to it
   * (`require_enabled`, a 422), so its steppers have to go — the same guard a damage source
   * that has left the table already gets.
   */
  tracked: readonly string[]
  /** The value to show for a chain, including anything still uncommitted. */
  value: (target: TapTarget) => number
  /** False on a finished game: the dialog becomes a read-only summary. */
  editable: boolean
}>()

const emit = defineEmits<{
  'update:open': [value: boolean]
  bump: [target: TapTarget, delta: number]
}>()

/** The sourceless counters this game shows, in vocabulary order. */
const simple = computed(() => props.shown.filter((kind) => kind !== COMMANDER_DAMAGE))

const showsCommanderDamage = computed(() => props.shown.includes(COMMANDER_DAMAGE))

/** One commander-damage row: an opponent, or an id that no longer sits at the table. */
interface DamageRow {
  sourceId: number
  name: string
  /** The seat's own position, for its colour — null for a source that has left. */
  position: number | null
}

const damageRows = computed<DamageRow[]>(() => {
  const seat = props.seat
  if (!seat || !showsCommanderDamage.value) return []
  const rows: DamageRow[] = props.seats
    .filter((other) => other.id !== seat.id)
    .map((other) => ({ sourceId: other.id, name: other.name, position: other.position }))
  // Damage from someone who has since been taken off the table: the seat is gone, its damage
  // isn't. Shown (it still counts towards 21) but with no stepper — there's no commander there
  // to deal more, and the server would 404 the source anyway.
  const seated = new Set(props.seats.map((other) => other.id))
  for (const sourceId of props.counters.commanderDamage.keys()) {
    if (!seated.has(sourceId)) {
      rows.push({ sourceId, name: 'A player who left', position: null })
    }
  }
  return rows
})

/** Whether a counter's steppers should be live: the game must still be on, and still tracking it. */
function canEdit(kind: LifeCounterKind): boolean {
  return props.editable && props.tracked.includes(kind)
}

/** The chain one damage row edits. */
function damageTarget(sourceId: number): TapTarget {
  return {
    playerId: props.seat?.id ?? 0,
    counter: COMMANDER_DAMAGE,
    sourcePlayerId: sourceId,
  }
}

function bump(target: TapTarget, delta: number) {
  // Belt and braces with the `v-if`s: the server would answer 422/404 anyway, and `useLifeTaps`
  // deliberately never retries, so the number would just snap back with an error banner.
  if (!target.counter || !canEdit(target.counter)) return
  emit('bump', target, delta)
}
</script>

<template>
  <Dialog :open="open" @update:open="(value: boolean) => emit('update:open', value)">
    <DialogContent
      class="bg-background max-h-[85dvh] w-[min(92vw,26rem)] overflow-y-auto rounded-xl border p-6 shadow-xl"
    >
      <DialogTitle>{{ seat?.name ?? 'Counters' }}</DialogTitle>
      <DialogDescription> Everything this seat is carrying besides life. </DialogDescription>

      <div v-if="seat" class="mt-4 space-y-5">
        <section v-if="showsCommanderDamage" class="space-y-2">
          <h3 class="text-sm font-medium">{{ COUNTER_META.commander_damage.label }}</h3>
          <p class="text-muted-foreground text-xs">
            Counted per commander — 21 from any single one ends the game for this seat.
          </p>
          <div
            v-for="row in damageRows"
            :key="row.sourceId"
            class="flex items-center gap-2 rounded-lg border p-2"
          >
            <span
              v-if="row.position !== null"
              class="size-2.5 shrink-0 rounded-full"
              :style="{ backgroundColor: seatColor(row.position) }"
              aria-hidden="true"
            />
            <span class="min-w-0 flex-1 truncate text-sm">{{ row.name }}</span>
            <!-- A source that has left the table is reported, not edited: there is no commander
                 there to deal more, and the server would 404 the seat id. -->
            <Button
              v-if="canEdit('commander_damage') && row.position !== null"
              variant="outline"
              size="icon-sm"
              :aria-label="`Less commander damage from ${row.name}`"
              @click="bump(damageTarget(row.sourceId), -1)"
            >
              <Minus class="size-4" />
            </Button>
            <span
              class="w-9 shrink-0 text-center text-lg font-semibold tabular-nums"
              :class="
                isLethalValue('commander_damage', value(damageTarget(row.sourceId)))
                  ? 'text-destructive'
                  : ''
              "
            >
              {{ value(damageTarget(row.sourceId)) }}
            </span>
            <Button
              v-if="canEdit('commander_damage') && row.position !== null"
              variant="outline"
              size="icon-sm"
              :aria-label="`More commander damage from ${row.name}`"
              @click="bump(damageTarget(row.sourceId), 1)"
            >
              <Plus class="size-4" />
            </Button>
            <Skull
              v-if="isLethalValue('commander_damage', value(damageTarget(row.sourceId)))"
              class="text-destructive size-4 shrink-0"
              aria-label="Lethal"
            />
          </div>
          <p v-if="damageRows.length === 0" class="text-muted-foreground text-sm">
            Nobody else is at the table yet.
          </p>
        </section>

        <section v-for="kind in simple" :key="kind" class="flex items-center gap-2">
          <span class="min-w-0 flex-1">
            <span class="block text-sm font-medium">{{ COUNTER_META[kind].label }}</span>
            <span class="text-muted-foreground block text-xs">{{ COUNTER_META[kind].hint }}</span>
          </span>
          <Button
            v-if="canEdit(kind)"
            variant="outline"
            size="icon-sm"
            :aria-label="`Less ${COUNTER_META[kind].label.toLowerCase()}`"
            @click="bump({ playerId: seat.id, counter: kind }, -1)"
          >
            <Minus class="size-4" />
          </Button>
          <span
            class="w-9 shrink-0 text-center text-lg font-semibold tabular-nums"
            :class="
              isLethalValue(kind, value({ playerId: seat.id, counter: kind }))
                ? 'text-destructive'
                : ''
            "
          >
            {{ value({ playerId: seat.id, counter: kind }) }}
          </span>
          <Button
            v-if="canEdit(kind)"
            variant="outline"
            size="icon-sm"
            :aria-label="`More ${COUNTER_META[kind].label.toLowerCase()}`"
            @click="bump({ playerId: seat.id, counter: kind }, 1)"
          >
            <Plus class="size-4" />
          </Button>
        </section>

        <p v-if="shown.length === 0" class="text-muted-foreground text-sm">
          This game isn't tracking any counters. Turn some on from the game menu.
        </p>
      </div>

      <div class="mt-4 flex justify-end">
        <DialogClose :class="buttonVariants({ variant: 'outline' })">Done</DialogClose>
      </div>
    </DialogContent>
  </Dialog>
</template>
