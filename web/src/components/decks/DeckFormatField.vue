<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { formatGroupsFor } from '@/lib/deckFormats'
import { formatLabel, normalizeFormatKey } from '@/lib/legality'

const props = defineProps<{ game: string }>()
const model = defineModel<string>({ required: true })

const NONE = 'none'
const CUSTOM = 'custom'

const groups = computed(() => formatGroupsFor(props.game))
const options = computed(() => groups.value.flatMap((group) => group.options))

function canon(value: string): string {
  return value.toLowerCase().replace(/[^a-z0-9]/g, '')
}

function matchingOption(value: string): string | null {
  const direct = options.value.find((option) => canon(option) === canon(value))
  if (direct) return direct

  const formatKey = normalizeFormatKey(value)
  if (!formatKey) return null

  const normalizedLabel = formatLabel(formatKey)
  return options.value.find((option) => canon(option) === canon(normalizedLabel)) ?? null
}

const derivedSelection = computed(() => {
  if (model.value === '') return NONE
  return matchingOption(model.value) ?? CUSTOM
})

// Selecting Custom must reveal the input even when the current value is a preset. Keep that
// explicit editing mode until the parent changes the model independently (for example, when a
// dialog resets or reopens).
const customEditing = ref(false)
let pendingInternalValue: string | undefined

watch(model, (value) => {
  if (value === pendingInternalValue) {
    pendingInternalValue = undefined
    return
  }
  customEditing.value = false
})

const selection = computed({
  get: () => (customEditing.value ? CUSTOM : derivedSelection.value),
  set: (value: string) => {
    if (value === CUSTOM) {
      customEditing.value = true
      return
    }

    customEditing.value = false
    model.value = value === NONE ? '' : value
  },
})

const customModel = computed({
  get: () => model.value,
  set: (value: string) => {
    pendingInternalValue = value
    model.value = value
  },
})
</script>

<template>
  <div v-if="groups.length" class="space-y-2">
    <Select v-model="selection">
      <SelectTrigger class="w-full" aria-label="Format">
        <SelectValue placeholder="No format" />
      </SelectTrigger>
      <SelectContent>
        <SelectItem :value="NONE">No format</SelectItem>
        <SelectGroup v-for="group in groups" :key="group.label">
          <SelectLabel>{{ group.label }}</SelectLabel>
          <SelectItem v-for="option in group.options" :key="option" :value="option">
            {{ option }}
          </SelectItem>
        </SelectGroup>
        <SelectItem :value="CUSTOM">Custom…</SelectItem>
      </SelectContent>
    </Select>
    <Input
      v-if="selection === CUSTOM"
      v-model="customModel"
      placeholder="Format (optional)"
      autofocus
    />
  </div>
  <Input v-else v-model="model" placeholder="Format (optional)" />
</template>
