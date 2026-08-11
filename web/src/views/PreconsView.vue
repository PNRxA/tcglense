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
import PreconSetGroupTile from '@/components/decks/PreconSetGroup.vue'
import { useGameName, useSetsQuery } from '@/composables/useCatalog'
import { usePreconFacetsQuery } from '@/composables/usePrecons'
import { useSetTileSections } from '@/composables/useSetTileSections'
import { groupPreconSets, preconGroupMatchesRelated } from '@/lib/preconSetGroups'
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
    `Challenger decks, Jumpstart themes and intro packs — with full decklists, prices ` +
    `and one-click copying into your own decks on TCGLense.`,
  canonicalPath: () => `/decks/${game.value}/precons`,
})

// The sets that actually have precons (code + name + count), from the same facets read the
// browse's set filter uses. Effectively static per game.
const facetsQuery = usePreconFacetsQuery(game)
const facetSets = computed(() => facetsQuery.data.value?.data.sets ?? [])
const total = computed(() => facetsQuery.data.value?.data.total ?? 0)

// Nest each set's related sub-sets under it, the way the card landing groups `/cards/{game}`:
// a set that shipped both a main deck and a Commander sub-set reads as one entry with the
// siblings tucked inside, instead of two tiles a year apart. Grouping happens **before**
// filtering and sectioning so a group is filtered and dated as a unit — a child that matches
// the filter keeps its whole group (via `alsoMatches`), and the group sits in its *main* set's
// release year rather than being split across two.
const catalogSetsQuery = useSetsQuery(game)
const groups = computed(() =>
  groupPreconSets(facetSets.value, catalogSetsQuery.data.value?.data ?? []),
)

// Filtering, the pinned "Featured" split and the release-year sections come from the shared
// set-tile landing engine — the same one the sealed landing runs, here fed groups rather than
// bare sets (a group exposes its main's `code`/`name`, which is all the engine reads).
const { filter, trimmedFilter, filtering, filteredSets, catalogSetByCode, sections } =
  useSetTileSections(
    game,
    computed(() => groups.value.map((group) => ({ ...group, ...group.main }))),
    { alsoMatches: (group, needle) => preconGroupMatchesRelated(group, needle) },
  )

// Every set on screen, counting a group's nested sub-sets — the tiles are fewer than the sets.
const shownSetCount = computed(() =>
  filteredSets.value.reduce((sum, group) => sum + 1 + group.children.length, 0),
)

// Whether a section holds any grouped tile, so the plain tiles beside them reserve the height
// of a group's toggle row and the row stays level. Per section rather than globally: most
// years ship no related-set pairs at all, and reserving there would just add dead space.
const sectionHasGroup = (section: { sets: { children: unknown[] }[] }) =>
  section.sets.some((entry) => entry.children.length > 0)
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
        Jumpstart themes and intro packs.
      </p>
      <p class="text-muted-foreground mt-1 text-sm">
        <!-- Sets, not tiles: a group is one tile standing for itself plus its sub-sets, and
             the honest count is how many sets published decks. -->
        {{ shownSetCount }} {{ shownSetCount === 1 ? 'set' : 'sets' }}
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
        <!-- items-start so a group that expands its sub-sets grows downwards instead of
             stretching every tile in its row (the card landing's grid does the same). -->
        <div class="grid items-start gap-3 sm:grid-cols-2 lg:grid-cols-3">
          <template v-for="entry in section.sets" :key="entry.code">
            <SetCountTile
              v-if="!entry.children.length"
              :game="game"
              :code="entry.main.code"
              :name="entry.main.name"
              :count="entry.main.count"
              noun="deck"
              :reserve-group-space="sectionHasGroup(section)"
              :catalog-set="catalogSetByCode[entry.main.code]"
              :to="`/decks/${game}/precons/sets/${entry.main.code}`"
            />
            <PreconSetGroupTile
              v-else
              :game="game"
              :group="entry"
              :catalog-set-by-code="catalogSetByCode"
              :query="trimmedFilter"
            />
          </template>
        </div>
      </section>
    </div>
  </div>
</template>
