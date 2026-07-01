<script setup lang="ts">
import { computed, useId } from 'vue'
import { Eraser, SlidersHorizontal } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { cn } from '@/lib/utils'
import {
  activeFilterCount,
  clearBuilderFilters,
  COLOR_MODES,
  COLOR_PIPS,
  FORMAT_OPTIONS,
  getColors,
  getFormat,
  getManaValue,
  getRarity,
  getType,
  getUsd,
  RARITY_OPTIONS,
  setColors,
  setFormat,
  setManaValue,
  setRarity,
  setType,
  setUsd,
  TYPE_OPTIONS,
  type ColorMode,
} from '@/lib/searchBuilder'

// Point-and-click helpers that assemble Scryfall syntax into the shared search box
// (issue #146). It is bound to the very same `q` string the CardSearchBox edits, so
// each control derives its state from the query and writes its own tokens back —
// hand-typed syntax and the builder stay in sync and compose, and free text / unrelated
// filters are never disturbed. All the query↔control mapping lives in lib/searchBuilder.
const query = defineModel<string>({ required: true })

const activeCount = computed(() => activeFilterCount(query.value))

// --- Colours: pips + colourless toggle + comparison mode ---
const colors = computed(() => getColors(query.value))

function togglePip(letter: string) {
  const { letters, mode } = colors.value
  const next = letters.includes(letter) ? letters.filter((l) => l !== letter) : [...letters, letter]
  query.value = setColors(query.value, { letters: next, colorless: false, mode })
}

function toggleColorless() {
  query.value = setColors(query.value, {
    letters: [],
    colorless: !colors.value.colorless,
    mode: colors.value.mode,
  })
}

function setColorMode(mode: ColorMode) {
  query.value = setColors(query.value, { ...colors.value, mode })
}

// --- Type / format single-selects ---
const type = computed({
  get: () => getType(query.value),
  set: (value: string) => (query.value = setType(query.value, value)),
})
const format = computed({
  get: () => getFormat(query.value),
  set: (value: string) => (query.value = setFormat(query.value, value)),
})

// --- Rarity: a value plus an "and higher" toggle ---
const rarity = computed(() => getRarity(query.value))
const raritySelect = computed({
  get: () => rarity.value.value,
  // Clearing the rarity also clears "and higher" (it's meaningless with no rarity).
  set: (value: string) =>
    (query.value = setRarity(query.value, {
      value,
      orHigher: value ? rarity.value.orHigher : false,
    })),
})
function toggleOrHigher() {
  if (!rarity.value.value) return
  query.value = setRarity(query.value, {
    value: rarity.value.value,
    orHigher: !rarity.value.orHigher,
  })
}

// --- Mana value / price ranges ---
type NumberInput = string | number | undefined

const mv = computed(() => getManaValue(query.value))
const mvMin = computed({
  get: () => mv.value.min,
  set: (value: NumberInput) =>
    (query.value = setManaValue(query.value, { ...mv.value, min: String(value ?? '').trim() })),
})
const mvMax = computed({
  get: () => mv.value.max,
  set: (value: NumberInput) =>
    (query.value = setManaValue(query.value, { ...mv.value, max: String(value ?? '').trim() })),
})

const usd = computed(() => getUsd(query.value))
const usdMin = computed({
  get: () => usd.value.min,
  set: (value: NumberInput) =>
    (query.value = setUsd(query.value, { ...usd.value, min: String(value ?? '').trim() })),
})
const usdMax = computed({
  get: () => usd.value.max,
  set: (value: NumberInput) =>
    (query.value = setUsd(query.value, { ...usd.value, max: String(value ?? '').trim() })),
})

function clearAll() {
  query.value = clearBuilderFilters(query.value)
}

// Stable, unique ids so each <select> pairs with its <Label for>, and each control-group
// (colours, mana value, price) names itself via aria-labelledby.
const typeId = useId()
const rarityId = useId()
const formatId = useId()
const colorsLabelId = useId()
const mvLabelId = useId()
const usdLabelId = useId()

// A native <select> styled to match the shadcn Input, so the panel needs no extra
// primitive for its single-selects.
const selectClass =
  'border-input dark:bg-input/30 focus-visible:border-ring focus-visible:ring-ring/50 h-9 w-full rounded-md border bg-transparent px-2 text-sm shadow-xs outline-none transition-[color,box-shadow] focus-visible:ring-[3px]'
</script>

<template>
  <Popover>
    <PopoverTrigger as-child>
      <Button
        variant="outline"
        size="sm"
        class="gap-2"
        :aria-label="`Advanced search filters${activeCount ? `, ${activeCount} active` : ''}`"
      >
        <SlidersHorizontal class="size-4" />
        <span>Filters</span>
        <span
          v-if="activeCount"
          class="bg-primary text-primary-foreground inline-flex h-5 min-w-5 items-center justify-center rounded-full px-1 text-xs font-medium tabular-nums"
        >
          {{ activeCount }}
        </span>
      </Button>
    </PopoverTrigger>

    <PopoverContent align="end" class="w-80 space-y-4">
      <div class="flex items-center justify-between">
        <p class="text-sm font-medium">Filters</p>
        <Button
          variant="ghost"
          size="sm"
          class="text-muted-foreground -mr-2 h-7 gap-1.5"
          :disabled="!activeCount"
          @click="clearAll"
        >
          <Eraser class="size-3.5" />
          Clear
        </Button>
      </div>

      <!-- Colours -->
      <div class="space-y-2">
        <span :id="colorsLabelId" class="text-sm font-medium leading-none">Colors</span>
        <div
          class="flex flex-wrap items-center gap-1.5"
          role="group"
          :aria-labelledby="colorsLabelId"
        >
          <button
            v-for="pip in COLOR_PIPS"
            :key="pip.letter"
            type="button"
            :aria-pressed="colors.letters.includes(pip.letter)"
            :aria-label="pip.label"
            :disabled="colors.colorless"
            :class="
              cn(
                'flex size-8 items-center justify-center rounded-full transition disabled:opacity-40',
                colors.letters.includes(pip.letter)
                  ? 'ring-ring ring-2 ring-offset-1 ring-offset-background'
                  : 'opacity-50 hover:opacity-100',
              )
            "
            @click="togglePip(pip.letter)"
          >
            <i :class="['ms', `ms-${pip.letter}`, 'ms-cost']" aria-hidden="true" />
          </button>
          <button
            type="button"
            :aria-pressed="colors.colorless"
            aria-label="Colorless"
            :class="
              cn(
                'flex size-8 items-center justify-center rounded-full transition',
                colors.colorless
                  ? 'ring-ring ring-2 ring-offset-1 ring-offset-background'
                  : 'opacity-50 hover:opacity-100',
              )
            "
            @click="toggleColorless"
          >
            <i class="ms ms-c ms-cost" aria-hidden="true" />
          </button>
        </div>
        <div
          v-if="colors.letters.length"
          role="group"
          aria-label="Colour comparison mode"
          class="bg-muted text-muted-foreground inline-flex rounded-md p-0.5 text-xs"
        >
          <button
            v-for="option in COLOR_MODES"
            :key="option.value"
            type="button"
            :aria-pressed="colors.mode === option.value"
            :class="
              cn(
                'rounded px-2.5 py-1 font-medium transition-colors',
                colors.mode === option.value
                  ? 'bg-background text-foreground shadow-sm'
                  : 'hover:text-foreground',
              )
            "
            @click="setColorMode(option.value)"
          >
            {{ option.label }}
          </button>
        </div>
      </div>

      <!-- Type -->
      <div class="space-y-2">
        <Label :for="typeId">Type</Label>
        <select :id="typeId" v-model="type" :class="selectClass">
          <option v-for="option in TYPE_OPTIONS" :key="option.value" :value="option.value">
            {{ option.label }}
          </option>
        </select>
      </div>

      <!-- Rarity -->
      <div class="space-y-2">
        <Label :for="rarityId">Rarity</Label>
        <div class="flex items-center gap-2">
          <select :id="rarityId" v-model="raritySelect" :class="selectClass">
            <option v-for="option in RARITY_OPTIONS" :key="option.value" :value="option.value">
              {{ option.label }}
            </option>
          </select>
          <button
            type="button"
            :aria-pressed="rarity.orHigher"
            :disabled="!rarity.value"
            title="This rarity and higher"
            :class="
              cn(
                'h-9 shrink-0 rounded-md border px-2.5 text-sm font-medium transition-colors disabled:opacity-40',
                rarity.orHigher
                  ? 'border-ring bg-primary text-primary-foreground'
                  : 'border-input text-muted-foreground hover:text-foreground',
              )
            "
            @click="toggleOrHigher"
          >
            &amp; up
          </button>
        </div>
      </div>

      <!-- Mana value -->
      <div class="space-y-2">
        <span :id="mvLabelId" class="text-sm font-medium leading-none">Mana value</span>
        <div class="flex items-center gap-2" role="group" :aria-labelledby="mvLabelId">
          <Input
            v-model="mvMin"
            type="number"
            min="0"
            placeholder="Min"
            aria-label="Minimum mana value"
          />
          <span class="text-muted-foreground text-sm">–</span>
          <Input
            v-model="mvMax"
            type="number"
            min="0"
            placeholder="Max"
            aria-label="Maximum mana value"
          />
        </div>
      </div>

      <!-- Format legality -->
      <div class="space-y-2">
        <Label :for="formatId">Legal in</Label>
        <select :id="formatId" v-model="format" :class="selectClass">
          <option v-for="option in FORMAT_OPTIONS" :key="option.value" :value="option.value">
            {{ option.label }}
          </option>
        </select>
      </div>

      <!-- Price -->
      <div class="space-y-2">
        <span :id="usdLabelId" class="text-sm font-medium leading-none">Price (USD)</span>
        <div class="flex items-center gap-2" role="group" :aria-labelledby="usdLabelId">
          <Input
            v-model="usdMin"
            type="number"
            min="0"
            step="0.01"
            placeholder="Min"
            aria-label="Minimum USD price"
          />
          <span class="text-muted-foreground text-sm">–</span>
          <Input
            v-model="usdMax"
            type="number"
            min="0"
            step="0.01"
            placeholder="Max"
            aria-label="Maximum USD price"
          />
        </div>
      </div>
    </PopoverContent>
  </Popover>
</template>
