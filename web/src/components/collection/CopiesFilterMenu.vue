<script setup lang="ts">
import { computed, ref, useId, watch } from 'vue'
import { Layers } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import {
  COPIES_COMPARATORS,
  FINISH_OPTIONS,
  boundsFromComparison,
  comparisonFromBounds,
  describeCopiesFilter,
  isCopiesFilterActive,
  type CopiesComparator,
  type CopiesFilter,
  type HoldingFinish,
} from '@/lib/holdingsFilter'
import { cn } from '@/lib/utils'

// The copy-count + finish filter chip in the collection / wish-list browse toolbars (issue
// #677): "which cards do I own more than four of?", "which playsets am I one short of?",
// "foils I own". A small form in a popover — a comparator (exactly / at least / more than /
// at most / less than / between), the number to compare against, and which finish the count
// reads — rather than a fixed preset ladder, so any threshold is one form away.
//
// One prop in, one event out: `filter` is the committed URL state, `apply` hands back the
// whole filter the owner commits in ONE URL write (`useCopiesFilter.set`). Never two model
// writes: each `router.replace` snapshots the route before the previous navigation lands,
// so writing the bounds and the finish separately would re-land whichever key the first
// write had just dropped. The trigger states the active filter in words (the same prose
// the count line reads) and wears the same active pill as GhostToggle. Meaningless on the
// catalog listing, so the browse views hide it in show-ghosts mode.
const props = defineProps<{ filter: CopiesFilter }>()
const emit = defineEmits<{ apply: [filter: CopiesFilter]; clear: [] }>()

const open = ref(false)
const active = computed(() => isCopiesFilterActive(props.filter))
const triggerLabel = computed(() => describeCopiesFilter(props.filter) ?? 'Copies')

// The form's draft, seeded from the committed filter each time the popover opens (or the
// URL changes underneath it — Back/forward, the count line's chip), so it never shows a
// stale draft over a live filter. The number boxes hold whatever the input hands back (Vue
// casts a `type="number"` model to a number, an emptied box to ''): an empty box is "no
// bound", and the box must be free to hold a half-typed value until Apply.
const comparator = ref<CopiesComparator>('gte')
const value = ref<string | number>('')
const upper = ref<string | number>('')
const finish = ref<HoldingFinish>('any')

function seed() {
  const comparison = comparisonFromBounds(props.filter)
  comparator.value = comparison?.comparator ?? 'gte'
  value.value = comparison ? String(comparison.value) : ''
  upper.value = comparison?.upper != null ? String(comparison.upper) : ''
  finish.value = props.filter.finish
}
seed()
watch(() => props.filter, seed, { deep: true })
watch(open, (isOpen) => {
  if (isOpen) seed()
})

const isBetween = computed(() => comparator.value === 'between')

/** A whole non-negative count, or null for a blank / half-typed / junk box. */
function readCount(raw: string | number): number | null {
  const trimmed = String(raw).trim()
  if (!/^\d+$/.test(trimmed)) return null
  const n = Number(trimmed)
  return Number.isSafeInteger(n) ? n : null
}

/** Whether a box holds anything at all (a blank box is "no bound", not an error). */
const blank = (raw: string | number) => String(raw).trim() === ''

// The draft as the filter it would commit: no number = a finish-only filter (or nothing);
// a number that can't be a bound (e.g. "less than 0", "between 5 and 2") is invalid and
// blocks Apply rather than silently committing something else.
const draftBounds = computed(() => {
  const n = readCount(value.value)
  if (n == null) return isBetween.value || !blank(value.value) ? null : {}
  const hi = isBetween.value ? readCount(upper.value) : undefined
  if (isBetween.value && hi == null) return null
  return boundsFromComparison({ comparator: comparator.value, value: n, upper: hi ?? undefined })
})
const invalid = computed(() => draftBounds.value === null)
const draft = computed<CopiesFilter | null>(() =>
  draftBounds.value === null ? null : { ...draftBounds.value, finish: finish.value },
)

function apply() {
  if (!draft.value) return
  emit('apply', draft.value)
  open.value = false
}

function clear() {
  emit('clear')
  open.value = false
}

function onSelectComparator(next: unknown) {
  const option = COPIES_COMPARATORS.find((candidate) => candidate.value === next)
  if (option) comparator.value = option.value
}

function onSelectFinish(next: unknown) {
  const option = FINISH_OPTIONS.find((candidate) => candidate.value === next)
  if (option) finish.value = option.value
}

// A Select's dropdown is portalled to <body>, so picking a comparator reads as "outside"
// the popover and would dismiss the whole form (AdvancedSearchPanel's guard).
function keepOpenForSelect(event: {
  detail?: { originalEvent?: Event }
  preventDefault: () => void
}) {
  const target = event.detail?.originalEvent?.target as HTMLElement | null | undefined
  if (target?.closest('[data-slot="select-content"]')) event.preventDefault()
}

const comparatorId = useId()
const valueId = useId()
const upperId = useId()
</script>

<template>
  <Popover v-model:open="open">
    <PopoverTrigger as-child>
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
    </PopoverTrigger>

    <PopoverContent
      align="start"
      class="w-80 space-y-4"
      @pointer-down-outside="keepOpenForSelect"
      @focus-outside="keepOpenForSelect"
    >
      <form class="space-y-4" @submit.prevent="apply">
        <div class="space-y-2">
          <Label :for="comparatorId">Copies held</Label>
          <div class="flex items-center gap-2">
            <Select :model-value="comparator" @update:model-value="onSelectComparator">
              <SelectTrigger
                :id="comparatorId"
                size="sm"
                class="w-full"
                aria-label="How to compare the count"
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem
                  v-for="option in COPIES_COMPARATORS"
                  :key="option.value"
                  :value="option.value"
                >
                  {{ option.label }}
                </SelectItem>
              </SelectContent>
            </Select>
            <Input
              :id="valueId"
              v-model="value"
              type="number"
              inputmode="numeric"
              min="0"
              step="1"
              placeholder="any"
              aria-label="Number of copies"
              class="h-8 w-20 shrink-0 tabular-nums"
              :aria-invalid="invalid || undefined"
            />
          </div>
          <!-- `between` needs its upper end; every other comparison is one number. -->
          <div v-if="isBetween" class="flex items-center gap-2">
            <Label :for="upperId" class="text-muted-foreground shrink-0 text-xs">and</Label>
            <Input
              :id="upperId"
              v-model="upper"
              type="number"
              inputmode="numeric"
              min="0"
              step="1"
              aria-label="Upper number of copies"
              class="h-8 w-20 shrink-0 tabular-nums"
              :aria-invalid="invalid || undefined"
            />
          </div>
          <p class="text-muted-foreground text-xs">
            Leave the number blank to filter by finish alone.
          </p>
        </div>

        <div class="space-y-2">
          <p class="text-sm font-medium">Finish</p>
          <!-- Fill the row so the three options share it evenly and none can run off the
               popover's edge; `w-full` overrides the primitive's `w-fit`. -->
          <ToggleGroup
            type="single"
            variant="outline"
            size="sm"
            class="w-full"
            :model-value="finish"
            aria-label="Which finish the count reads"
            @update:model-value="onSelectFinish"
          >
            <ToggleGroupItem
              v-for="option in FINISH_OPTIONS"
              :key="option.value"
              :value="option.value"
              class="flex-1 text-xs"
            >
              {{ option.label }}
            </ToggleGroupItem>
          </ToggleGroup>
        </div>

        <div class="flex items-center justify-between gap-2">
          <!-- One click back to the unfiltered list; only offered while something is set. -->
          <Button
            v-if="active"
            type="button"
            variant="ghost"
            size="sm"
            class="text-muted-foreground -ml-2"
            @click="clear"
          >
            Clear filter
          </Button>
          <span v-else />
          <Button type="submit" size="sm" :disabled="invalid">Apply</Button>
        </div>
      </form>
    </PopoverContent>
  </Popover>
</template>
