<script setup lang="ts">
import { ref, watch } from 'vue'
import { ChevronDown, Layers } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import { buttonVariants } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import SetTile from '@/components/cards/SetTile.vue'
import type { CountNoun } from '@/lib/ownership'
import { queryMatchesRelated, subSetLabel, type SetGroup } from '@/lib/setGroups'

const props = withDefaults(
  defineProps<{
    game: string
    group: SetGroup
    // Route prefix the tiles + "View all" link point under (default the catalog
    // set pages); the collection landing passes `/collection` for its own pages.
    basePath?: string
    // The landing view's active set-list filter. When it matches one of this group's
    // related sub-sets, the dropdown auto-opens so the match isn't hidden behind the
    // collapsed toggle (issue #149); '' (no filter) leaves it collapsed by default.
    query?: string
    // Owned distinct-card count per set code. When set, each tile shows an "N/M owned"
    // completion count (the collection landing) instead of the set's total card count.
    ownedCounts?: Record<string, number>
    // Total owned copies (with duplicates) per set code, shown as "N copies" on each tile
    // when it exceeds the distinct owned count — the collection landing.
    ownedCopies?: Record<string, number>
    // Preformatted owned value per set code (e.g. "$123.45"), appended to each tile's
    // meta line alongside the owned count — the collection landing's per-set value.
    ownedValues?: Record<string, string | null>
    // Preformatted bulk (< $1/card) value per set code, shown under the total on each
    // tile — the collection landing's per-set bulk value.
    bulkValues?: Record<string, string | null>
    // The word each tile's count line ends with ("owned" by default; the wish-list
    // landing passes "wanted", issue #167).
    countNoun?: CountNoun
  }>(),
  {
    basePath: '/cards',
    query: '',
    ownedCounts: undefined,
    ownedCopies: undefined,
    ownedValues: undefined,
    bulkValues: undefined,
    countNoun: 'owned',
  },
)

// Collapsed by default to keep the set listing scannable; the sub-sets reveal on
// demand, and each group manages its own toggle state. But when the active filter
// matches a related sub-set, auto-reveal it so the match that kept this group in the
// filtered listing isn't buried behind the collapsed toggle (issue #149). Additive
// only — it never force-collapses, so the toggle button keeps full manual control (a
// user can still hide the sub-sets while filtering) and a broadening filter that
// re-surfaces the group re-opens it (immediate + fires on the match becoming true).
const expanded = ref(false)
watch(
  () => queryMatchesRelated(props.group, props.query),
  (matched) => {
    if (matched) expanded.value = true
  },
  { immediate: true },
)

const setLink = (code: string) => `${props.basePath}/${props.game}/sets/${code}`
const ownedCount = (code: string) => props.ownedCounts?.[code]
const ownedCopiesCount = (code: string) => props.ownedCopies?.[code]
const ownedValue = (code: string) => props.ownedValues?.[code]
const bulkValue = (code: string) => props.bulkValues?.[code]
</script>

<template>
  <div class="bg-card rounded-xl border" :class="expanded ? 'border-ring/40' : ''">
    <SetTile
      :game="game"
      :set="group.main"
      variant="plain"
      :to="setLink(group.main.code)"
      :owned-count="ownedCount(group.main.code)"
      :owned-copies="ownedCopiesCount(group.main.code)"
      :owned-value="ownedValue(group.main.code)"
      :bulk-value="bulkValue(group.main.code)"
      :count-noun="countNoun"
    />

    <div class="flex items-center justify-between gap-2 px-3 pb-2">
      <button
        type="button"
        class="text-muted-foreground hover:text-foreground -mx-1.5 flex min-h-9 items-center gap-1.5 rounded-md px-1.5 text-xs"
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
          :owned-copies="ownedCopiesCount(child.code)"
          :owned-value="ownedValue(child.code)"
          :bulk-value="bulkValue(child.code)"
          :count-noun="countNoun"
        />
      </li>
    </ul>
  </div>
</template>
