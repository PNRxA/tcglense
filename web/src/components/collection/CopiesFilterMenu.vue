<script setup lang="ts">
import { computed } from 'vue'
import { Layers } from '@lucide/vue'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import {
  COPIES_PRESETS,
  FINISH_OPTIONS,
  describeCopiesFilter,
  isCopiesFilterActive,
  parseCopiesToken,
  type HoldingFinish,
} from '@/lib/holdingsFilter'
import { cn } from '@/lib/utils'

// The copy-count + finish filter chip in the collection / wish-list browse toolbars (issue
// #677): "which cards do I own more than four of?", "which playsets am I one short of?",
// "foils I own". Both halves are one control because they answer one question together —
// the copy bounds are read *through* the finish (`4` + `foil` = four foil copies) — so
// flipping either while the menu stays open lets the pair be set in one visit.
//
// Two v-models, both URL-backed by `useCopiesFilter`: the `?copies=` token (`''` = any
// count) and the `?finish=` value. The trigger states the active filter in words so the
// toolbar says what the grid is showing without opening anything, and wears the same active
// pill as GhostToggle. Meaningless on the catalog listing, so the browse views hide it in
// show-ghosts mode rather than offering a filter the endpoint would ignore.
const copies = defineModel<string>('copies', { required: true })
const finish = defineModel<HoldingFinish>('finish', { required: true })
// "Clear filter" is one event, not two model writes: each v-model write lands its own
// `router.replace`, and the second one snapshots the route *before* the first navigation
// commits — so it would re-land the very key the first just dropped. The owner clears both
// halves (and the page) in the composable's single write.
const emit = defineEmits<{ clear: [] }>()

const filter = computed(() => ({ ...parseCopiesToken(copies.value), finish: finish.value }))
const active = computed(() => isCopiesFilterActive(filter.value))
// The prose is the shared one the count line reads, so the chip and the header agree.
const triggerLabel = computed(() => describeCopiesFilter(filter.value) ?? 'Copies')

function onSelectCopies(value: string | undefined) {
  copies.value = value ?? ''
}

function onSelectFinish(value: string | undefined) {
  // The radio group hands back a bare string; only a token the API knows is committed.
  const option = FINISH_OPTIONS.find((candidate) => candidate.value === value)
  if (option) finish.value = option.value
}

// Keep the menu open while a radio item is picked, so both groups can be set in one visit
// (GhostToggle's pattern for its in-place display toggles).
const keepOpen = (event: Event) => event.preventDefault()
</script>

<template>
  <DropdownMenu>
    <DropdownMenuTrigger as-child>
      <button
        type="button"
        :class="
          cn(
            'focus-visible:ring-ring inline-flex items-center gap-1.5 rounded-md border px-3 py-1.5 text-sm font-medium outline-none transition-colors focus-visible:ring-2',
            active
              ? 'border-primary bg-primary/10 text-foreground'
              : 'text-muted-foreground hover:text-foreground',
          )
        "
        :aria-pressed="active"
        title="Filter by how many copies you hold, and which finish"
      >
        <Layers class="size-4" aria-hidden="true" />
        <span class="truncate">{{ triggerLabel }}</span>
      </button>
    </DropdownMenuTrigger>
    <DropdownMenuContent align="start" class="w-56">
      <DropdownMenuLabel>Copies held</DropdownMenuLabel>
      <DropdownMenuSeparator />
      <DropdownMenuRadioGroup :model-value="copies" @update:model-value="onSelectCopies">
        <DropdownMenuRadioItem
          v-for="option in COPIES_PRESETS"
          :key="option.value || 'any'"
          :value="option.value"
          @select="keepOpen"
        >
          {{ option.label }}
        </DropdownMenuRadioItem>
      </DropdownMenuRadioGroup>

      <DropdownMenuSeparator />
      <DropdownMenuLabel>Finish</DropdownMenuLabel>
      <DropdownMenuSeparator />
      <DropdownMenuRadioGroup :model-value="finish" @update:model-value="onSelectFinish">
        <DropdownMenuRadioItem
          v-for="option in FINISH_OPTIONS"
          :key="option.value"
          :value="option.value"
          @select="keepOpen"
        >
          {{ option.label }}
        </DropdownMenuRadioItem>
      </DropdownMenuRadioGroup>

      <!-- One click back to the unfiltered list; only worth offering while something is set. -->
      <template v-if="active">
        <DropdownMenuSeparator />
        <DropdownMenuItem @select="emit('clear')">Clear filter</DropdownMenuItem>
      </template>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
