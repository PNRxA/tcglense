<script setup lang="ts">
import { computed, useId } from 'vue'
import { Eraser, SlidersHorizontal } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  NumberField,
  NumberFieldContent,
  NumberFieldDecrement,
  NumberFieldIncrement,
  NumberFieldInput,
} from '@/components/ui/number-field'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Toggle } from '@/components/ui/toggle'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import ArtTagFilter from './ArtTagFilter.vue'
import {
  activeFilterCount,
  CARD_FLAG_OPTIONS,
  clearBuilderFilters,
  COLOR_MODES,
  COLOR_PIPS,
  FORMAT_OPTIONS,
  getArtist,
  getCardFlags,
  getCollectorNumber,
  getColors,
  getFormat,
  getManaValue,
  getOracleText,
  getPower,
  getRarity,
  getSetCode,
  getToughness,
  getType,
  getUsd,
  RARITY_OPTIONS,
  setArtist,
  setCardFlag,
  setCollectorNumber,
  setColors,
  setFormat,
  setManaValue,
  setOracleText,
  setPower,
  setRarity,
  setSetCode,
  setToughness,
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

// The art-tag control fetches its suggestions per game. Every surface mounting the
// panel is MTG-scoped today (the panel's whole syntax is), so the default keeps the
// seven call sites untouched; a future TCG passes its own slug.
withDefaults(defineProps<{ game?: string }>(), { game: 'mtg' })

const activeCount = computed(() => activeFilterCount(query.value))

// The Select can't use '' as an item value (reka reserves it for "no selection"), so the
// "Any" option rides a sentinel that maps back to the builder's empty value.
const ANY = 'any'
const toSel = (value: string) => value || ANY
const fromSel = (value: string) => (value === ANY ? '' : value)

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

function onColorMode(mode: unknown) {
  // A single ToggleGroup emits '' when the active item is re-clicked; keep the mode set.
  if (mode) query.value = setColors(query.value, { ...colors.value, mode: mode as ColorMode })
}

// --- Type / format single-selects ---
const typeSelect = computed({
  get: () => toSel(getType(query.value)),
  set: (value: string) => (query.value = setType(query.value, fromSel(value))),
})
const formatSelect = computed({
  get: () => toSel(getFormat(query.value)),
  set: (value: string) => (query.value = setFormat(query.value, fromSel(value))),
})

// --- Rarity: a value plus an "and higher" toggle ---
const rarity = computed(() => getRarity(query.value))
const raritySelect = computed({
  get: () => toSel(rarity.value.value),
  // Clearing the rarity also clears "and higher" (it's meaningless with no rarity).
  set: (value: string) => {
    const val = fromSel(value)
    query.value = setRarity(query.value, {
      value: val,
      orHigher: val ? rarity.value.orHigher : false,
    })
  },
})
function toggleOrHigher() {
  if (!rarity.value.value) return
  query.value = setRarity(query.value, {
    value: rarity.value.value,
    orHigher: !rarity.value.orHigher,
  })
}

// --- Mana value / price ranges ---
// The NumberField models a `number` (empty = undefined/NaN); the builder stores each
// bound as a string ('' = unset), so map between them at the boundary.
type NumberModel = number | undefined
const strToNum = (s: string): NumberModel => {
  if (s === '') return undefined
  const n = Number(s)
  return Number.isNaN(n) ? undefined : n
}
const numToStr = (n: NumberModel): string => (n == null || Number.isNaN(n) ? '' : String(n))

const mv = computed(() => getManaValue(query.value))
const mvMin = computed({
  get: () => strToNum(mv.value.min),
  set: (value: NumberModel) =>
    (query.value = setManaValue(query.value, { ...mv.value, min: numToStr(value) })),
})
const mvMax = computed({
  get: () => strToNum(mv.value.max),
  set: (value: NumberModel) =>
    (query.value = setManaValue(query.value, { ...mv.value, max: numToStr(value) })),
})

const usd = computed(() => getUsd(query.value))
const usdMin = computed({
  get: () => strToNum(usd.value.min),
  set: (value: NumberModel) =>
    (query.value = setUsd(query.value, { ...usd.value, min: numToStr(value) })),
})
const usdMax = computed({
  get: () => strToNum(usd.value.max),
  set: (value: NumberModel) =>
    (query.value = setUsd(query.value, { ...usd.value, max: numToStr(value) })),
})

// Power / toughness ranges (no :min — power can be negative).
const pow = computed(() => getPower(query.value))
const powMin = computed({
  get: () => strToNum(pow.value.min),
  set: (value: NumberModel) =>
    (query.value = setPower(query.value, { ...pow.value, min: numToStr(value) })),
})
const powMax = computed({
  get: () => strToNum(pow.value.max),
  set: (value: NumberModel) =>
    (query.value = setPower(query.value, { ...pow.value, max: numToStr(value) })),
})

const tou = computed(() => getToughness(query.value))
const touMin = computed({
  get: () => strToNum(tou.value.min),
  set: (value: NumberModel) =>
    (query.value = setToughness(query.value, { ...tou.value, min: numToStr(value) })),
})
const touMax = computed({
  get: () => strToNum(tou.value.max),
  set: (value: NumberModel) =>
    (query.value = setToughness(query.value, { ...tou.value, max: numToStr(value) })),
})

// --- Free-text filters (oracle text, set code, artist) ---
// Plain string round-trips: the builder quotes multi-word values and strips the quotes
// back off on read, so typing (including inner spaces) survives the get/set cycle.
const oracleText = computed({
  get: () => getOracleText(query.value),
  set: (value: string) => (query.value = setOracleText(query.value, value)),
})
const setCode = computed({
  get: () => getSetCode(query.value),
  set: (value: string) => (query.value = setSetCode(query.value, value)),
})
const artist = computed({
  get: () => getArtist(query.value),
  set: (value: string) => (query.value = setArtist(query.value, value)),
})
const collectorNumber = computed({
  get: () => getCollectorNumber(query.value),
  set: (value: string) => (query.value = setCollectorNumber(query.value, value)),
})

// --- Card flags (`is:` toggles) ---
const cardFlags = computed(() => getCardFlags(query.value))
function toggleCardFlag(flag: string) {
  query.value = setCardFlag(query.value, flag, !cardFlags.value.includes(flag))
}

function clearAll() {
  query.value = clearBuilderFilters(query.value)
}

// A Select's dropdown is portalled to <body>, so interacting with it reads as "outside"
// the popover and would dismiss the whole panel. The art-tag browser dialog (and its
// overlay) is portalled the same way. Keep the panel open in those cases; a genuine
// outside click (or Escape) still closes it.
function keepOpenForSelect(event: {
  detail?: { originalEvent?: Event }
  preventDefault: () => void
}) {
  const target = event.detail?.originalEvent?.target as HTMLElement | null | undefined
  if (
    target?.closest(
      '[data-slot="select-content"], [data-slot="dialog-content"], [data-slot="dialog-overlay"]',
    )
  )
    event.preventDefault()
}

// Stable, unique ids so each Select trigger pairs with its <Label for>, and each
// control-group (colours, mana value, price) names itself via aria-labelledby.
const typeId = useId()
const rarityId = useId()
const formatId = useId()
const oracleId = useId()
const setId = useId()
const artistId = useId()
const cnId = useId()
const colorsLabelId = useId()
const mvLabelId = useId()
const usdLabelId = useId()
const powLabelId = useId()
const touLabelId = useId()
const flagsLabelId = useId()

// Shared styling for the round mana pips — a Toggle whose "on" state is a ring, not the
// default filled background (which would clash with the coloured mana glyph).
const pipClass =
  'size-8 min-w-0 rounded-full p-0 opacity-50 hover:bg-transparent hover:opacity-100 data-[state=on]:bg-transparent data-[state=on]:opacity-100 data-[state=on]:ring-2 data-[state=on]:ring-ring data-[state=on]:ring-offset-1 data-[state=on]:ring-offset-background'
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

    <PopoverContent
      align="end"
      class="max-h-[min(70vh,40rem)] w-80 space-y-4 overflow-y-auto"
      @pointer-down-outside="keepOpenForSelect"
      @focus-outside="keepOpenForSelect"
    >
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
          <Toggle
            v-for="pip in COLOR_PIPS"
            :key="pip.letter"
            :model-value="colors.letters.includes(pip.letter)"
            :disabled="colors.colorless"
            :aria-label="pip.label"
            :class="pipClass"
            @update:model-value="togglePip(pip.letter)"
          >
            <i :class="['ms', `ms-${pip.letter}`, 'ms-cost']" aria-hidden="true" />
          </Toggle>
          <Toggle
            :model-value="colors.colorless"
            aria-label="Colorless"
            :class="pipClass"
            @update:model-value="toggleColorless"
          >
            <i class="ms ms-c ms-cost" aria-hidden="true" />
          </Toggle>
        </div>
        <ToggleGroup
          v-if="colors.letters.length"
          type="single"
          variant="outline"
          size="sm"
          :model-value="colors.mode"
          aria-label="Colour comparison mode"
          @update:model-value="onColorMode"
        >
          <ToggleGroupItem
            v-for="option in COLOR_MODES"
            :key="option.value"
            :value="option.value"
            class="text-xs"
          >
            {{ option.label }}
          </ToggleGroupItem>
        </ToggleGroup>
      </div>

      <!-- Type -->
      <div class="space-y-2">
        <Label :for="typeId">Type</Label>
        <Select v-model="typeSelect">
          <SelectTrigger :id="typeId" size="sm" class="w-full" aria-label="Type">
            <SelectValue placeholder="Any type" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem
              v-for="option in TYPE_OPTIONS"
              :key="option.value"
              :value="option.value || ANY"
            >
              {{ option.label }}
            </SelectItem>
          </SelectContent>
        </Select>
      </div>

      <!-- Card text -->
      <div class="space-y-2">
        <Label :for="oracleId">Card text</Label>
        <Input
          :id="oracleId"
          v-model="oracleText"
          type="text"
          class="h-8"
          placeholder="e.g. draw a card"
          autocomplete="off"
        />
      </div>

      <!-- Rarity -->
      <div class="space-y-2">
        <Label :for="rarityId">Rarity</Label>
        <div class="flex items-center gap-2">
          <Select v-model="raritySelect">
            <SelectTrigger :id="rarityId" size="sm" class="w-full" aria-label="Rarity">
              <SelectValue placeholder="Any" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem
                v-for="option in RARITY_OPTIONS"
                :key="option.value"
                :value="option.value || ANY"
              >
                {{ option.label }}
              </SelectItem>
            </SelectContent>
          </Select>
          <Toggle
            :model-value="rarity.orHigher"
            :disabled="!rarity.value"
            variant="outline"
            size="sm"
            class="shrink-0"
            aria-label="This rarity and higher"
            @update:model-value="toggleOrHigher"
          >
            &amp; up
          </Toggle>
        </div>
      </div>

      <!-- Mana value -->
      <div class="space-y-2">
        <span :id="mvLabelId" class="text-sm font-medium leading-none">Mana value</span>
        <div class="flex items-center gap-2" role="group" :aria-labelledby="mvLabelId">
          <NumberField
            v-model="mvMin"
            :min="0"
            :step-snapping="false"
            :format-options="{ maximumFractionDigits: 0 }"
            class="flex-1"
          >
            <NumberFieldContent>
              <NumberFieldDecrement />
              <NumberFieldInput placeholder="Min" aria-label="Minimum mana value" />
              <NumberFieldIncrement />
            </NumberFieldContent>
          </NumberField>
          <span class="text-muted-foreground text-sm">–</span>
          <NumberField
            v-model="mvMax"
            :min="0"
            :step-snapping="false"
            :format-options="{ maximumFractionDigits: 0 }"
            class="flex-1"
          >
            <NumberFieldContent>
              <NumberFieldDecrement />
              <NumberFieldInput placeholder="Max" aria-label="Maximum mana value" />
              <NumberFieldIncrement />
            </NumberFieldContent>
          </NumberField>
        </div>
      </div>

      <!-- Power -->
      <div class="space-y-2">
        <span :id="powLabelId" class="text-sm font-medium leading-none">Power</span>
        <div class="flex items-center gap-2" role="group" :aria-labelledby="powLabelId">
          <NumberField
            v-model="powMin"
            :step-snapping="false"
            :format-options="{ maximumFractionDigits: 0 }"
            class="flex-1"
          >
            <NumberFieldContent>
              <NumberFieldDecrement />
              <NumberFieldInput placeholder="Min" aria-label="Minimum power" />
              <NumberFieldIncrement />
            </NumberFieldContent>
          </NumberField>
          <span class="text-muted-foreground text-sm">–</span>
          <NumberField
            v-model="powMax"
            :step-snapping="false"
            :format-options="{ maximumFractionDigits: 0 }"
            class="flex-1"
          >
            <NumberFieldContent>
              <NumberFieldDecrement />
              <NumberFieldInput placeholder="Max" aria-label="Maximum power" />
              <NumberFieldIncrement />
            </NumberFieldContent>
          </NumberField>
        </div>
      </div>

      <!-- Toughness -->
      <div class="space-y-2">
        <span :id="touLabelId" class="text-sm font-medium leading-none">Toughness</span>
        <div class="flex items-center gap-2" role="group" :aria-labelledby="touLabelId">
          <NumberField
            v-model="touMin"
            :step-snapping="false"
            :format-options="{ maximumFractionDigits: 0 }"
            class="flex-1"
          >
            <NumberFieldContent>
              <NumberFieldDecrement />
              <NumberFieldInput placeholder="Min" aria-label="Minimum toughness" />
              <NumberFieldIncrement />
            </NumberFieldContent>
          </NumberField>
          <span class="text-muted-foreground text-sm">–</span>
          <NumberField
            v-model="touMax"
            :step-snapping="false"
            :format-options="{ maximumFractionDigits: 0 }"
            class="flex-1"
          >
            <NumberFieldContent>
              <NumberFieldDecrement />
              <NumberFieldInput placeholder="Max" aria-label="Maximum toughness" />
              <NumberFieldIncrement />
            </NumberFieldContent>
          </NumberField>
        </div>
      </div>

      <!-- Format legality -->
      <div class="space-y-2">
        <Label :for="formatId">Legal in</Label>
        <Select v-model="formatSelect">
          <SelectTrigger :id="formatId" size="sm" class="w-full" aria-label="Legal in">
            <SelectValue placeholder="Any format" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem
              v-for="option in FORMAT_OPTIONS"
              :key="option.value"
              :value="option.value || ANY"
            >
              {{ option.label }}
            </SelectItem>
          </SelectContent>
        </Select>
      </div>

      <!-- Set / artist -->
      <div class="flex gap-2">
        <div class="w-24 shrink-0 space-y-2">
          <Label :for="setId">Set</Label>
          <Input
            :id="setId"
            v-model="setCode"
            type="text"
            class="h-8"
            placeholder="e.g. mh3"
            autocomplete="off"
          />
        </div>
        <div class="min-w-0 flex-1 space-y-2">
          <Label :for="artistId">Artist</Label>
          <Input
            :id="artistId"
            v-model="artist"
            type="text"
            class="h-8"
            placeholder="Artist name"
            autocomplete="off"
          />
        </div>
      </div>

      <!-- Art tags (Tagger `art:` filters) -->
      <ArtTagFilter v-model="query" :game="game" />

      <!-- Collector number -->
      <div class="space-y-2">
        <Label :for="cnId">Collector number</Label>
        <Input
          :id="cnId"
          v-model="collectorNumber"
          type="text"
          class="h-8"
          placeholder="e.g. 234 or 234a"
          autocomplete="off"
        />
      </div>

      <!-- Price -->
      <div class="space-y-2">
        <span :id="usdLabelId" class="text-sm font-medium leading-none">Price (USD)</span>
        <div class="flex items-center gap-2" role="group" :aria-labelledby="usdLabelId">
          <NumberField
            v-model="usdMin"
            :min="0"
            :step-snapping="false"
            :format-options="{ maximumFractionDigits: 2 }"
            class="flex-1"
          >
            <NumberFieldContent>
              <NumberFieldDecrement />
              <NumberFieldInput placeholder="Min" aria-label="Minimum USD price" />
              <NumberFieldIncrement />
            </NumberFieldContent>
          </NumberField>
          <span class="text-muted-foreground text-sm">–</span>
          <NumberField
            v-model="usdMax"
            :min="0"
            :step-snapping="false"
            :format-options="{ maximumFractionDigits: 2 }"
            class="flex-1"
          >
            <NumberFieldContent>
              <NumberFieldDecrement />
              <NumberFieldInput placeholder="Max" aria-label="Maximum USD price" />
              <NumberFieldIncrement />
            </NumberFieldContent>
          </NumberField>
        </div>
      </div>

      <!-- Card flags (is: toggles) -->
      <div class="space-y-2">
        <span :id="flagsLabelId" class="text-sm font-medium leading-none">Flags</span>
        <div class="flex flex-wrap gap-1.5" role="group" :aria-labelledby="flagsLabelId">
          <Toggle
            v-for="option in CARD_FLAG_OPTIONS"
            :key="option.value"
            :model-value="cardFlags.includes(option.value)"
            variant="outline"
            size="sm"
            class="text-xs"
            :aria-label="option.label"
            @update:model-value="toggleCardFlag(option.value)"
          >
            {{ option.label }}
          </Toggle>
        </div>
      </div>
    </PopoverContent>
  </Popover>
</template>
