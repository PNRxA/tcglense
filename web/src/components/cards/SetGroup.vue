<script setup lang="ts">
import { ref } from 'vue'
import { ChevronDown } from '@lucide/vue'
import SetTile from '@/components/cards/SetTile.vue'
import type { SetGroup } from '@/lib/setGroups'

const props = defineProps<{
  game: string
  group: SetGroup
}>()

// Collapsed by default to keep the set listing scannable; the sub-sets reveal on
// demand. Each group manages its own toggle state.
const expanded = ref(false)

// Sub-set names usually repeat the parent's (e.g. "Bloomburrow Commander").
// Drop that redundant prefix for the nested rows, keeping the full name as the
// tooltip and falling back to it when there's no shared prefix.
function childLabel(name: string): string {
  const prefix = props.group.main.name
  if (name.length > prefix.length && name.startsWith(prefix)) {
    const rest = name.slice(prefix.length).replace(/^[\s:–-]+/, '')
    if (rest) return rest
  }
  return name
}
</script>

<template>
  <div class="bg-card rounded-xl border" :class="expanded ? 'border-ring/40' : ''">
    <SetTile :game="game" :set="group.main" variant="plain" />

    <button
      type="button"
      class="text-muted-foreground hover:text-foreground flex w-full items-center gap-1.5 px-3 pb-2 text-xs"
      :aria-expanded="expanded"
      @click="expanded = !expanded"
    >
      <ChevronDown class="size-3.5 transition-transform" :class="expanded ? 'rotate-180' : ''" />
      {{ expanded ? 'Hide' : 'Show' }} {{ group.children.length }} related
      {{ group.children.length === 1 ? 'set' : 'sets' }}
    </button>

    <ul
      v-if="expanded"
      class="space-y-0.5 border-t px-2 pt-1.5 pb-2"
      :aria-label="`Sets related to ${group.main.name}`"
    >
      <li v-for="child in group.children" :key="child.code">
        <SetTile :game="game" :set="child" :label="childLabel(child.name)" variant="nested" />
      </li>
    </ul>
  </div>
</template>
