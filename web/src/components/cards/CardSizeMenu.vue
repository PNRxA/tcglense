<script setup lang="ts">
import { computed } from 'vue'
import { LayoutGrid } from '@lucide/vue'
import RadioSelectMenu from '@/components/cards/RadioSelectMenu.vue'
import { CARD_SIZE_OPTIONS, isCardSize } from '@/lib/cardSize'
import { useCardSizeStore } from '@/stores/cardSize'

// Self-contained like ThemeToggle: it reads and writes the shared preference store
// directly, so a view only has to drop <CardSizeMenu /> into its toolbar.
const cardSize = useCardSizeStore()

const activeLabel = computed(
  () => CARD_SIZE_OPTIONS.find((o) => o.value === cardSize.size)?.label ?? 'Size',
)

// Bridge the menu's string model to the typed store, narrowing back to a CardSize
// on commit (the radio group's values are always valid sizes).
const model = computed({
  get: () => cardSize.size as string,
  set: (value) => {
    if (isCardSize(value)) cardSize.setSize(value)
  },
})
</script>

<template>
  <RadioSelectMenu
    v-model="model"
    :options="CARD_SIZE_OPTIONS"
    label="Card size"
    :trigger-icon="LayoutGrid"
    :trigger-label="activeLabel"
    content-class="w-44"
  />
</template>
