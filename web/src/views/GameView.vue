<script setup lang="ts">
import { computed, toRef } from 'vue'
import { LayoutGrid } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import { buttonVariants } from '@/components/ui/button'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import SetGridSkeleton from '@/components/cards/SetGridSkeleton.vue'
import SetGroupGrid from '@/components/cards/SetGroupGrid.vue'
import StickySearchBar from '@/components/cards/StickySearchBar.vue'
import { useGameName, useSetsQuery } from '@/composables/useCatalog'
import { useFilteredSetGroups } from '@/composables/useSetGrouping'
import { usePageMeta } from '@/lib/seo'
import { groupByYear, partitionPinned } from '@/lib/setGroups'

const props = defineProps<{ game: string }>()
const game = toRef(props, 'game')

const gameName = useGameName(game)

usePageMeta({
  title: () => gameName.value,
  description: () =>
    `Browse ${gameName.value} sets and cards on TCGLense, with singles prices tracked over time.`,
  canonicalPath: () => `/cards/${game.value}`,
})

const setsQuery = useSetsQuery(game)

const sets = computed(() => setsQuery.data.value?.data ?? [])

// Client-side filter box + nested sub-set grouping (tokens, promos, Commander decks,
// art series, … nested under their main set), shared with the collection game view: the
// whole set list is already in memory, so narrowing by name/code is instant. The group
// is kept whole when the main set OR any related sub-set matches (issue #128).
const { filter, trimmedFilter, filtering, groups, relatedCount } = useFilteredSetGroups(game, sets)

// Pull pinned sets (e.g. Secret Lair) out so they lead the listing regardless of
// their release date; the rest stay date-sorted.
const partitioned = computed(() => partitionPinned(groups.value))
// Break the (newest-first) remaining groups into release-year sections so a long
// catalog is scannable; undated sets fall into a trailing "Unknown" section.
const years = computed(() => groupByYear(partitioned.value.rest))

const yearLabel = (year: number | null) => (year === null ? 'Unknown year' : String(year))

// One flat list of sections to render: the pinned "Featured" section first (when
// present), then the date-sorted year sections.
const sections = computed(() => {
  const featured = partitioned.value.pinned
  const yearSections = years.value.map((section) => ({
    key: section.year === null ? 'unknown' : String(section.year),
    label: yearLabel(section.year),
    groups: section.groups,
  }))
  return featured.length
    ? [{ key: 'featured', label: 'Featured', groups: featured }, ...yearSections]
    : yearSections
})
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-10">
    <PageBreadcrumbs :items="[{ label: 'Cards', to: '/cards' }, { label: gameName }]" />

    <header class="mb-4">
      <h1 class="text-3xl font-semibold tracking-tight">{{ gameName }}</h1>
      <p class="text-muted-foreground mt-1">
        {{ groups.length }} {{ groups.length === 1 ? 'set' : 'sets' }}
        <template v-if="relatedCount > 0"> · {{ relatedCount }} related</template>
        <template v-if="filtering"> matching “{{ trimmedFilter }}”</template>
      </p>
    </header>

    <!-- The filter bar sticks to the top of the viewport so it stays reachable
         while scrolling the set list; its fixed height is what the year headings
         below offset against (their sticky `top-15`) so the two never overlap. -->
    <StickySearchBar class="mb-6 flex items-center gap-3">
      <CardSearchBox
        v-if="sets.length"
        v-model="filter"
        class="w-full sm:w-64"
        aria-label="Filter sets by name or code"
        placeholder="Filter sets…"
      />
      <RouterLink
        :to="`/cards/${game}/cards`"
        :class="buttonVariants({ variant: 'default' })"
        class="shrink-0"
      >
        <LayoutGrid />
        View all cards
      </RouterLink>
    </StickySearchBar>

    <SetGridSkeleton v-if="setsQuery.isPending.value" />
    <p v-else-if="setsQuery.isError.value" class="text-destructive py-12">
      Couldn't load sets. Please retry.
    </p>
    <p v-else-if="!sets.length" class="text-muted-foreground py-12">No sets available yet.</p>
    <p v-else-if="filtering && !groups.length" class="text-muted-foreground py-12">
      No sets match “{{ trimmedFilter }}”.
    </p>

    <div v-else class="space-y-10">
      <section v-for="section in sections" :key="section.key">
        <!-- Stuck below the sticky filter bar above (top-15 = its height) so the
             two stack rather than overlap at the top of the viewport. -->
        <div
          class="bg-background/85 sticky top-15 z-10 -mx-4 mb-3 flex items-baseline gap-2 border-b px-4 py-2 backdrop-blur"
        >
          <h2 class="text-xl font-semibold tracking-tight">{{ section.label }}</h2>
          <span class="text-muted-foreground text-sm">
            {{ section.groups.length }} {{ section.groups.length === 1 ? 'set' : 'sets' }}
          </span>
        </div>
        <SetGroupGrid
          :game="game"
          :groups="section.groups"
          :scroll-mt="28"
          :query="trimmedFilter"
        />
      </section>
    </div>
  </div>
</template>
