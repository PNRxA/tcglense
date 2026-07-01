<script setup lang="ts">
import { ref } from 'vue'
import { ChevronDown, Layers } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import { buttonVariants } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import SetTile from '@/components/cards/SetTile.vue'
import { subSetLabel, type SetGroup } from '@/lib/setGroups'

const props = withDefaults(
  defineProps<{
    game: string
    group: SetGroup
    // Route prefix the tiles + "View all" link point under (default the catalog
    // set pages); the collection landing passes `/collection` for its own pages.
    basePath?: string
    // Owned-card count per set code. When set, each tile shows "N owned" (the
    // collection landing) instead of the set's total card count.
    ownedCounts?: Record<string, number>
    // Preformatted owned value per set code (e.g. "$123.45"), appended to each tile's
    // meta line alongside the owned count — the collection landing's per-set value.
    ownedValues?: Record<string, string | null>
  }>(),
  { basePath: '/cards', ownedCounts: undefined, ownedValues: undefined },
)

// Collapsed by default to keep the set listing scannable; the sub-sets reveal on
// demand. Each group manages its own toggle state.
const expanded = ref(false)

const setLink = (code: string) => `${props.basePath}/${props.game}/sets/${code}`
const ownedCount = (code: string) => props.ownedCounts?.[code]
const ownedValue = (code: string) => props.ownedValues?.[code]
</script>

<template>
  <div class="bg-card rounded-xl border" :class="expanded ? 'border-ring/40' : ''">
    <SetTile
      :game="game"
      :set="group.main"
      variant="plain"
      :to="setLink(group.main.code)"
      :owned-count="ownedCount(group.main.code)"
      :owned-value="ownedValue(group.main.code)"
    />

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
        :to="{ path: setLink(group.main.code), query: { related: '1' } }"
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
          :to="setLink(child.code)"
          :owned-count="ownedCount(child.code)"
          :owned-value="ownedValue(child.code)"
        />
      </li>
    </ul>
  </div>
</template>
