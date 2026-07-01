<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
import { useQuery } from '@tanstack/vue-query'
import { LayoutGrid, Loader2 } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import { buttonVariants } from '@/components/ui/button'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import SetTile from '@/components/cards/SetTile.vue'
import SetGroup from '@/components/cards/SetGroup.vue'
import { useGameName, useSetsQuery } from '@/composables/useCatalog'
import { gameStatus } from '@/lib/api'
import { usePageMeta } from '@/lib/seo'
import { filterSets, groupByYear, groupSets, partitionPinned } from '@/lib/setGroups'

const props = defineProps<{ game: string }>()
const game = toRef(props, 'game')

const gameName = useGameName(game)

usePageMeta({
  title: () => gameName.value,
  description: () =>
    `Browse ${gameName.value} sets and cards on TCGLense, with singles prices tracked over time.`,
  canonicalPath: () => `/cards/${game.value}`,
})

const statusQuery = useQuery({
  queryKey: ['status', game],
  queryFn: () => gameStatus(game.value),
  // Poll while an import is in progress; stop once it finishes or fails.
  refetchInterval: (query) => {
    const status = query.state.data?.status
    return status === 'complete' || status === 'error' ? false : 4000
  },
})

const setsQuery = useSetsQuery(game)

// When the import finishes, pull the freshly-populated sets.
watch(
  () => statusQuery.data.value?.status,
  (status, previous) => {
    if (status === 'complete' && previous && previous !== 'complete') {
      setsQuery.refetch()
    }
  },
)

const importing = computed(() => {
  const status = statusQuery.data.value?.status
  return status !== undefined && status !== 'complete' && status !== 'error'
})
const sets = computed(() => setsQuery.data.value?.data ?? [])

// Client-side filter box: the whole set list is already in memory, so narrowing
// by name/code is instant — no extra request. Clears when switching games (the
// route reuses this component across :game).
const filter = ref('')
watch(game, () => {
  filter.value = ''
})
const trimmedFilter = computed(() => filter.value.trim())
const filtering = computed(() => trimmedFilter.value.length > 0)
const filteredSets = computed(() => filterSets(sets.value, filter.value))

// Nest sub-sets (tokens, promos, Commander decks, art series, …) under the main
// set they belong to instead of scattering them across the date-sorted list.
const groups = computed(() => groupSets(filteredSets.value))
const relatedCount = computed(() => filteredSets.value.length - groups.value.length)
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
    <nav class="text-muted-foreground mb-4 text-sm">
      <RouterLink to="/cards" class="hover:underline">Cards</RouterLink>
      <span class="mx-1.5">/</span>
      <span class="text-foreground">{{ gameName }}</span>
    </nav>

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
    <div
      class="bg-background/85 sticky top-0 z-30 -mx-4 mb-6 flex items-center gap-3 border-b px-4 py-3 backdrop-blur"
    >
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
    </div>

    <!-- First-boot import progress. -->
    <div
      v-if="importing"
      class="bg-muted/50 text-muted-foreground mb-6 flex items-center gap-3 rounded-lg border p-4 text-sm"
    >
      <Loader2 class="size-4 shrink-0 animate-spin" />
      <span>
        Importing card data…
        <template v-if="statusQuery.data.value?.cards_imported">
          {{ statusQuery.data.value.cards_imported.toLocaleString() }} cards so far.
        </template>
        This page will update automatically.
      </span>
    </div>

    <LoadingRow v-if="setsQuery.isPending.value" label="Loading sets…" />
    <p v-else-if="setsQuery.isError.value" class="text-destructive py-12">
      Couldn't load sets. Please retry.
    </p>
    <p v-else-if="!sets.length && !importing" class="text-muted-foreground py-12">
      No sets available yet.
    </p>
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
        <!-- scroll-mt on the focusable tiles keeps a Tab-focused set clear of both
             the sticky filter bar and section heading above it (WCAG 2.4.11 Focus
             Not Obscured). -->
        <div
          class="grid items-start gap-3 [&_a]:scroll-mt-28 [&_button]:scroll-mt-28 sm:grid-cols-2 lg:grid-cols-3"
        >
          <template v-for="group in section.groups" :key="group.main.code">
            <SetTile v-if="!group.children.length" :game="game" :set="group.main" />
            <SetGroup v-else :game="game" :group="group" />
          </template>
        </div>
      </section>
    </div>
  </div>
</template>
