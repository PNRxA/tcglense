<script setup lang="ts">
import { ref } from 'vue'
import { ChevronDown, Layers } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import { buttonVariants } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import SetTile from '@/components/cards/SetTile.vue'
import { subSetLabel, type SetGroup } from '@/lib/setGroups'

defineProps<{
  game: string
  group: SetGroup
}>()

// Collapsed by default to keep the set listing scannable; the sub-sets reveal on
// demand. Each group manages its own toggle state.
const expanded = ref(false)
</script>

<template>
  <div class="bg-card rounded-xl border" :class="expanded ? 'border-ring/40' : ''">
    <SetTile :game="game" :set="group.main" variant="plain" />

    <div class="flex items-center justify-between gap-2 px-3 pb-2">
      <button
        type="button"
        class="text-muted-foreground hover:text-foreground flex items-center gap-1.5 text-xs"
        :aria-expanded="expanded"
        @click="expanded = !expanded"
      >
        <ChevronDown class="size-3.5 transition-transform" :class="expanded ? 'rotate-180' : ''" />
        {{ expanded ? 'Hide' : 'Show' }} {{ group.children.length }} related
        {{ group.children.length === 1 ? 'set' : 'sets' }}
      </button>

      <!-- One-click jump straight to every card across the whole group. -->
      <RouterLink
        :to="{ path: `/cards/${game}/sets/${group.main.code}`, query: { related: '1' } }"
        :class="cn(buttonVariants({ variant: 'ghost', size: 'sm' }), 'h-7 px-2 text-xs')"
        :aria-label="`View all cards in ${group.main.name} and its related sets`"
      >
        <Layers class="size-3.5" />
        View all
      </RouterLink>
    </div>

    <ul
      v-if="expanded"
      class="space-y-0.5 border-t px-2 pt-1.5 pb-2"
      :aria-label="`Sets related to ${group.main.name}`"
    >
      <li v-for="child in group.children" :key="child.code">
        <SetTile
          :game="game"
          :set="child"
          :label="subSetLabel(group.main.name, child.name)"
          variant="nested"
        />
      </li>
    </ul>
  </div>
</template>
