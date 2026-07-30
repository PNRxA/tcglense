<script setup lang="ts">
import { computed } from 'vue'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { layoutOptionsFor, matPlacement } from '@/lib/lifeLayout'
import type { LifeLayout } from '@/lib/api/life'

// Choose how the seats sit. Each option draws itself: a miniature of the actual grid, with each
// seat's cell turned the way that layout turns it. A word like "pinwheel" means nothing until you
// see it, and the diagram is generated from the same placement maths the mat uses — so the
// preview can't disagree with the result.
const props = defineProps<{ modelValue: LifeLayout; playerCount: number }>()
const emit = defineEmits<{ 'update:modelValue': [value: LifeLayout] }>()

const options = computed(() => layoutOptionsFor(props.playerCount))

const selected = computed({
  get: () => props.modelValue,
  // A single ToggleGroup emits '' when the active item is re-clicked; keep the layout set.
  set: (value: string) => {
    if (value) emit('update:modelValue', value as LifeLayout)
  },
})

/** The miniature: the layout's own grid, one small bar per seat, rotated as that seat will be. */
function preview(layout: LifeLayout) {
  const placement = matPlacement(layout, Math.max(1, props.playerCount))
  return {
    style: {
      gridTemplateColumns: placement.columns,
      gridTemplateRows: placement.rows,
    },
    seats: placement.seats,
  }
}
</script>

<template>
  <ToggleGroup v-model="selected" type="single" variant="outline" class="flex flex-wrap gap-2">
    <ToggleGroupItem
      v-for="option in options"
      :key="option.value"
      :value="option.value"
      :aria-label="`${option.label} — ${option.hint}`"
      class="data-[state=on]:ring-ring h-auto flex-col items-start gap-1.5 p-2 data-[state=on]:ring-2"
    >
      <span class="grid h-10 w-14 gap-0.5" :style="preview(option.value).style" aria-hidden="true">
        <span
          v-for="(seat, index) in preview(option.value).seats"
          :key="index"
          class="bg-foreground/25 grid place-items-center rounded-[2px]"
          :style="{ gridColumn: seat.column, gridRow: seat.row }"
        >
          <span
            class="bg-foreground/60 block h-0.5 w-2/3 rounded-full"
            :style="{ transform: `rotate(${seat.rotation}deg)` }"
          />
        </span>
      </span>
      <span class="text-xs font-medium">{{ option.label }}</span>
    </ToggleGroupItem>
  </ToggleGroup>
</template>
