<script setup lang="ts">
import { computed } from 'vue'
import SetTile from '@/components/cards/SetTile.vue'
import SetGroup from '@/components/cards/SetGroup.vue'
import type { CountNoun } from '@/lib/ownership'
import type { SetGroup as SetGroupModel } from '@/lib/setGroups'

// The responsive grid of set tiles that both landing views render: a childless set is a
// plain tile, a set with related sub-sets a collapsible SetGroup. Shared by the catalog
// game view and the collection landing, which differ only in the sticky-bar offset
// (`scrollMt`), the route prefix (`basePath`), and whether owned counts ride along.
const props = withDefaults(
  defineProps<{
    game: string
    groups: SetGroupModel[]
    // Focus scroll-margin so a Tab-focused tile clears the sticky bar above it (WCAG
    // 2.4.11). The two landings sit under bars of different heights.
    scrollMt?: 20 | 28
    // Route prefix the tiles link under (catalog set pages by default; `/collection` for
    // the collection landing).
    basePath?: string
    // The landing view's active set-list filter, forwarded to each SetGroup so a match
    // on a related sub-set auto-opens that group's dropdown (issue #149).
    query?: string
    // Owned counts per set code (the collection + wish-list landings only). Collapsed
    // into one object so it's a single prop rather than several parallel maps.
    ownership?: {
      counts: Record<string, number>
      copies: Record<string, number>
      values: Record<string, string | null>
      bulkValues: Record<string, string | null>
    }
    // The word each tile's count line ends with ("owned" by default; the wish-list
    // landing passes "wanted", issue #167).
    countNoun?: CountNoun
  }>(),
  { scrollMt: 28, basePath: '/cards', query: '', ownership: undefined, countNoun: 'owned' },
)

// Literal class strings (not interpolated) so Tailwind's JIT emits them.
const scrollClass = computed(() =>
  props.scrollMt === 20
    ? '[&_a]:scroll-mt-20 [&_button]:scroll-mt-20'
    : '[&_a]:scroll-mt-28 [&_button]:scroll-mt-28',
)
const setLink = (code: string) => `${props.basePath}/${props.game}/sets/${code}`
</script>

<template>
  <div class="grid items-start gap-3 sm:grid-cols-2 lg:grid-cols-3" :class="scrollClass">
    <template v-for="group in groups" :key="group.main.code">
      <SetTile
        v-if="!group.children.length"
        :game="game"
        :set="group.main"
        :to="setLink(group.main.code)"
        :owned-count="ownership?.counts[group.main.code]"
        :owned-copies="ownership?.copies[group.main.code]"
        :owned-value="ownership?.values[group.main.code]"
        :bulk-value="ownership?.bulkValues[group.main.code]"
        :count-noun="countNoun"
      />
      <SetGroup
        v-else
        :game="game"
        :group="group"
        :base-path="basePath"
        :query="query"
        :owned-counts="ownership?.counts"
        :owned-copies="ownership?.copies"
        :owned-values="ownership?.values"
        :bulk-values="ownership?.bulkValues"
        :count-noun="countNoun"
      />
    </template>
  </div>
</template>
