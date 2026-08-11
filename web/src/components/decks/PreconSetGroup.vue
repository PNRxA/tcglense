<script setup lang="ts">
import { ref, watch, computed } from 'vue'
import { ChevronDown, Layers } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import { buttonVariants } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import SetCountTile from '@/components/shared/SetCountTile.vue'
import { subSetLabel } from '@/lib/setGroups'
import type { CardSet } from '@/lib/api'
import type { PreconSetGroup } from '@/lib/preconSetGroups'

// A set that published precons together with its related sub-sets — the precon landing's
// mirror of the card landing's `SetGroup`, and deliberately the same shape: the main set's
// tile, a "Show N related sets" toggle, and a link that spans the whole group at once.
//
// It isn't the same *component* because the two landings count different things: the card
// landing's tiles carry a card count and optional ownership, these carry a deck count and
// link under `/decks/{game}/precons`. Both draw their tiles through the tile each landing
// already uses (`SetTile` there, `SetCountTile` here), so the chrome stays in step.
const props = withDefaults(
  defineProps<{
    game: string
    group: PreconSetGroup
    // Catalog rows by set code — the icon + release-date source each tile resolves through.
    catalogSetByCode: Record<string, CardSet>
    // The landing's active filter. When it matches a related sub-set, the dropdown auto-opens
    // so the match that kept this group in the listing isn't buried (the card landing's
    // issue #149 rule).
    query?: string
  }>(),
  { query: '' },
)

const setLink = (code: string) => `/decks/${props.game}/precons/sets/${code}`
// Every deck across the group, which is what the spanning link actually opens.
const totalDecks = computed(() =>
  props.group.children.reduce((sum, set) => sum + set.count, props.group.main.count),
)

// Collapsed by default to keep the listing scannable. Additive auto-reveal only, so the toggle
// keeps full manual control — the card landing's SetGroup behaviour, matched deliberately.
const expanded = ref(false)
watch(
  () => {
    const needle = props.query.trim().toLowerCase()
    if (!needle) return false
    return props.group.children.some(
      (set) => set.name?.toLowerCase().includes(needle) || set.code.toLowerCase().includes(needle),
    )
  },
  (matched) => {
    if (matched) expanded.value = true
  },
  { immediate: true },
)
</script>

<template>
  <div class="bg-card rounded-xl border" :class="expanded ? 'border-ring/40' : ''">
    <SetCountTile
      :game="game"
      :code="group.main.code"
      :name="group.main.name"
      :count="group.main.count"
      noun="deck"
      variant="plain"
      :catalog-set="catalogSetByCode[group.main.code]"
      :to="setLink(group.main.code)"
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

      <!-- One click to every deck across the whole group — the `?related=1` span the browse
           view reads, matching the card landing's own "View all". -->
      <RouterLink
        :to="{ path: setLink(group.main.code), query: { related: '1' } }"
        :class="cn(buttonVariants({ variant: 'ghost', size: 'sm' }), 'h-7 px-2 text-xs')"
        :aria-label="`View all ${totalDecks} decks in ${group.main.name ?? group.main.code} and its related sets`"
      >
        <Layers class="size-3.5" />
        All {{ totalDecks }} decks
      </RouterLink>
    </div>

    <ul
      v-if="expanded"
      class="space-y-0.5 border-t px-2 pt-1.5 pb-2"
      :aria-label="`Sets related to ${group.main.name ?? group.main.code}`"
    >
      <li v-for="child in group.children" :key="child.code">
        <SetCountTile
          :game="game"
          :code="child.code"
          :name="child.name"
          :label="subSetLabel(group.main.name ?? '', child.name ?? child.code)"
          :count="child.count"
          noun="deck"
          variant="nested"
          :catalog-set="catalogSetByCode[child.code]"
          :to="setLink(child.code)"
        />
      </li>
    </ul>
  </div>
</template>
