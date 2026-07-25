<script setup lang="ts">
import { computed } from 'vue'
import { Rows3 } from '@lucide/vue'
import RadioSelectMenu from '@/components/cards/RadioSelectMenu.vue'
import { DECK_VIEW_MODE_OPTIONS, isDeckViewMode } from '@/lib/deckView'
import { useDeckViewStore } from '@/stores/deckView'

// Self-contained like CardSizeMenu: it reads and writes the shared preference store
// directly, so a deck view only has to drop <DeckViewMenu /> into its toolbar.
const deckView = useDeckViewStore()

const activeLabel = computed(
  () => DECK_VIEW_MODE_OPTIONS.find((o) => o.value === deckView.mode)?.label ?? 'View',
)

// Bridge the menu's string model to the typed store, narrowing back to a DeckViewMode
// on commit (the radio group's values are always valid modes).
const model = computed({
  get: () => deckView.mode as string,
  set: (value) => {
    if (isDeckViewMode(value)) deckView.setMode(value)
  },
})
</script>

<template>
  <RadioSelectMenu
    v-model="model"
    :options="DECK_VIEW_MODE_OPTIONS"
    label="Card display"
    :trigger-icon="Rows3"
    :trigger-label="activeLabel"
    content-class="w-44"
  />
</template>
