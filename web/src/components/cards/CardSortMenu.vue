<script setup lang="ts">
import { computed } from 'vue'
import { ArrowDownUp } from '@lucide/vue'
import RadioSelectMenu from '@/components/cards/RadioSelectMenu.vue'
import type { SortOption } from '@/lib/cardSort'

const props = defineProps<{ options: SortOption[] }>()
// The selected option's `field:dir` value (see lib/cardSort).
const model = defineModel<string>({ required: true })

const activeLabel = computed(
  () => props.options.find((o) => o.value === model.value)?.label ?? 'Sort',
)
</script>

<template>
  <RadioSelectMenu
    v-model="model"
    :options="options"
    label="Sort by"
    :trigger-icon="ArrowDownUp"
    :trigger-label="activeLabel"
    content-class="w-52"
  />
</template>
