<script setup lang="ts">
import { computed } from 'vue'
import { Skull } from '@lucide/vue'
import {
  COMMANDER_DAMAGE,
  COUNTER_META,
  isLethalValue,
  worstCommanderDamage,
  type LifeCounterKind,
  type SeatCounters,
} from '@/lib/lifeCounters'

// The counters on a seat tile, as a row of small chips under the total.
//
// It is deliberately a *summary*, not a control: the mat's whole point is that the thing you do
// a hundred times a game (tapping life) needs no aiming, so putting four more steppers on the
// tile would shrink the tap zones for the sake of the rarer action. The chips report, and the
// whole strip is one button that opens the seat's counter dialog.
//
// Commander damage collapses to the **worst single source**, because that is the number the game
// is measured against — 7 from each of three opponents is not lethal, and a chip reading "21"
// there would say the opposite of what is true.
const props = defineProps<{
  counters: SeatCounters
  /** Which counter rows this game shows — the strip never invents one the mat isn't tracking. */
  shown: LifeCounterKind[]
  seatName: string
}>()

defineEmits<{ open: [] }>()

interface Chip {
  kind: LifeCounterKind
  label: string
  value: number
  lethal: boolean
}

const chips = computed<Chip[]>(() =>
  props.shown.flatMap((kind) => {
    const value =
      kind === COMMANDER_DAMAGE
        ? worstCommanderDamage(props.counters)
        : (props.counters.values[kind] ?? 0)
    // A counter at zero is not state worth spending tile space on; the dialog still offers it.
    if (value === 0) return []
    return [
      {
        kind,
        label: COUNTER_META[kind].short,
        value,
        lethal: isLethalValue(kind, value),
      },
    ]
  }),
)
</script>

<template>
  <button
    type="button"
    class="hover:bg-accent/60 flex w-full flex-wrap items-center justify-center gap-1 rounded-md px-2 py-0.5 transition-colors"
    :aria-label="`Counters for ${seatName}`"
    @click.stop="$emit('open')"
  >
    <span
      v-for="chip in chips"
      :key="chip.kind"
      class="flex items-center gap-1 rounded-full px-1.5 py-0.5 text-[0.65rem] font-medium tabular-nums"
      :class="
        chip.lethal
          ? 'bg-destructive/15 text-destructive'
          : 'bg-foreground/10 text-muted-foreground'
      "
    >
      <Skull v-if="chip.lethal" class="size-3" aria-hidden="true" />
      {{ chip.label }} {{ chip.value }}
    </span>
    <span
      v-if="chips.length === 0"
      class="text-muted-foreground/60 text-[0.65rem] font-medium uppercase tracking-wide"
    >
      Counters
    </span>
  </button>
</template>
