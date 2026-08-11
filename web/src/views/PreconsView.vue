<script setup lang="ts">
import { computed, toRef } from 'vue'
import { RouterLink } from 'vue-router'
import { LayoutGrid } from '@lucide/vue'
import { buttonVariants } from '@/components/ui/button'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import SetGridSkeleton from '@/components/cards/SetGridSkeleton.vue'
import StickySearchBar from '@/components/cards/StickySearchBar.vue'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import SetCountTile from '@/components/shared/SetCountTile.vue'
import { useGameName } from '@/composables/useCatalog'
import { usePreconFacetsQuery } from '@/composables/usePrecons'
import { useSetTileSections } from '@/composables/useSetTileSections'
import { usePageMeta } from '@/lib/seo'

// The preconstructed-deck landing: the sets that published precons, as set tiles that click
// through to that set's decks — the deck mirror of the card catalog's `/cards/{game}` and the
// sealed landing's `/sealed/{game}`. "All decks" opens the flat browse instead.
//
// Public, like the rest of the catalog: a precon is published game data, so a visitor with no
// account can read every list.
const props = defineProps<{ game: string }>()
const game = toRef(props, 'game')
const gameName = useGameName(game)

usePageMeta({
  title: () => `${gameName.value} preconstructed decks`,
  description: () =>
    `Browse preconstructed ${gameName.value} decks by set — Commander decks, Planeswalker and ` +
    `Challenger decks, Jumpstart themes and Secret Lair drops — with full decklists, prices ` +
    `and one-click copying into your own decks on TCGLense.`,
  canonicalPath: () => `/decks/${game.value}/precons`,
})

// The sets that actually have precons (code + name + count), from the same facets read the
// browse's set filter uses. Effectively static per game.
const facetsQuery = usePreconFacetsQuery(game)
const facetSets = computed(() => facetsQuery.data.value?.data.sets ?? [])
const total = computed(() => facetsQuery.data.value?.data.total ?? 0)

// Filtering, the pinned "Featured" split and the release-year sections come from the shared
// set-tile landing engine — the same one the sealed landing runs.
const { filter, trimmedFilter, filtering, filteredSets, catalogSetByCode, sections } =
  useSetTileSections(game, facetSets)
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-10">
    <PageBreadcrumbs
      :items="[
        { label: 'Decks', to: '/decks' },
        { label: gameName, to: `/decks/${game}` },
        { label: 'Preconstructed' },
      ]"
    />

    <header class="mb-4">
      <h1 class="text-3xl font-semibold tracking-tight">Preconstructed decks</h1>
      <p class="text-muted-foreground mt-1">
        The decklists {{ gameName }} shipped — Commander decks, Planeswalker and Challenger decks,
        Jumpstart themes and Secret Lair drops.
      </p>
      <p class="text-muted-foreground mt-1 text-sm">
        {{ filteredSets.length }} {{ filteredSets.length === 1 ? 'set' : 'sets' }}
        <template v-if="filtering"> matching “{{ trimmedFilter }}”</template>
        <template v-else-if="total"> · {{ total.toLocaleString() }} decks</template>
      </p>
    </header>

    <!-- The filter bar sticks to the top of the viewport so it stays reachable while scrolling
         the set list; its fixed height is what the year headings below offset against (their
         sticky `top-15`) so the two never overlap. -->
    <StickySearchBar class="mb-6 flex items-center gap-3">
      <CardSearchBox
        v-if="facetSets.length"
        v-model="filter"
        class="w-full sm:w-64"
        aria-label="Filter sets by name or code"
        placeholder="Filter sets…"
      />
      <RouterLink
        :to="`/decks/${game}/precons/all`"
        :class="buttonVariants({ variant: 'default' })"
        class="shrink-0"
      >
        <LayoutGrid />
        All decks
      </RouterLink>
    </StickySearchBar>

    <SetGridSkeleton v-if="facetsQuery.isPending.value" />
    <p v-else-if="facetsQuery.isError.value" class="text-destructive py-12">
      Couldn't load preconstructed decks. Please retry.
    </p>
    <p v-else-if="!facetSets.length" class="text-muted-foreground py-12">
      No preconstructed decks available yet.
    </p>
    <p v-else-if="filtering && !filteredSets.length" class="text-muted-foreground py-12">
      No sets match “{{ trimmedFilter }}”.
    </p>

    <div v-else class="space-y-10">
      <section v-for="section in sections" :key="section.key">
        <!-- Stuck below the sticky filter bar above (top-15 = its height) so the two stack
             rather than overlap at the top of the viewport. -->
        <div
          class="bg-background/85 sticky top-15 z-10 -mx-4 mb-3 flex items-baseline gap-2 border-b px-4 py-2 backdrop-blur"
        >
          <h2 class="text-xl font-semibold tracking-tight">{{ section.label }}</h2>
          <span class="text-muted-foreground text-sm">
            {{ section.sets.length }} {{ section.sets.length === 1 ? 'set' : 'sets' }}
          </span>
        </div>
        <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
          <SetCountTile
            v-for="set in section.sets"
            :key="set.code"
            :game="game"
            :code="set.code"
            :name="set.name"
            :count="set.count"
            noun="deck"
            :catalog-set="catalogSetByCode[set.code]"
            :to="`/decks/${game}/precons/sets/${set.code}`"
          />
        </div>
      </section>
    </div>
  </div>
</template>
