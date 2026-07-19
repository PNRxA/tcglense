<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
import { LayoutGrid } from '@lucide/vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import { buttonVariants } from '@/components/ui/button'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import SetGridSkeleton from '@/components/cards/SetGridSkeleton.vue'
import StickySearchBar from '@/components/cards/StickySearchBar.vue'
import ProductSetTile from '@/components/products/ProductSetTile.vue'
import { useGameName, useSetsQuery } from '@/composables/useCatalog'
import { useProductFacetsQuery } from '@/composables/useProducts'
import { usePageMeta } from '@/lib/seo'
import type { CardSet, ProductSetRef } from '@/lib/api'

// The per-game sealed-product landing — the sealed mirror of the card catalog's GameView. It
// lists the sets that have sealed products as set tiles (grouped into release-year sections)
// that click through to the set-scoped flat browse, with an "All products" shortcut to the
// unscoped browse. Product sets are flat (no related-set nesting), so there's no Featured/pinned
// section and no group folding — just year sections. Public, like the catalog.
const props = defineProps<{ game: string }>()
const game = toRef(props, 'game')
const gameName = useGameName(game)

const route = useRoute()
const router = useRouter()

// Compat shim: this URL used to be the flat sealed browse (before the set-tile landing split),
// so old links/bookmarks may still carry its filters. Forward them to the new browse routes
// rather than silently dropping them here — a `?set=` becomes the set-scoped page (with `set`
// stripped from the carried query), any other browse filter (`q`/`type`/`sort`/`page`) forwards
// to the flat all-products browse with the query intact. Runs once at setup (in-app nav always
// targets the new routes, so only an external legacy link ever hits this).
const BROWSE_QUERY_KEYS = ['q', 'type', 'sort', 'page'] as const
const redirecting = ref(false)
const legacySet = typeof route.query.set === 'string' ? route.query.set : ''
if (legacySet) {
  redirecting.value = true
  const query = { ...route.query }
  delete query.set
  void router.replace({ path: `/sealed/${props.game}/sets/${legacySet}`, query })
} else if (BROWSE_QUERY_KEYS.some((key) => route.query[key] != null)) {
  redirecting.value = true
  void router.replace({ path: `/sealed/${props.game}/products`, query: route.query })
}

usePageMeta({
  title: () => `${gameName.value} sealed products`,
  description: () =>
    `Browse sealed ${gameName.value} products by set — booster boxes, bundles and decks — ` +
    `with current prices and price history on TCGLense.`,
  canonicalPath: () => `/sealed/${game.value}`,
})

// The sets that actually have sealed products (code + name + product_count), from the same
// facets read the flat browse's set filter uses. Effectively static per game.
const facetsQuery = useProductFacetsQuery(game)
const facetSets = computed(() => facetsQuery.data.value?.data.sets ?? [])

// Client-side filter box: the whole facet list is already in memory, so narrowing by name/code
// is instant. Cleared when `game` changes, since the route reuses this component across `:game`
// (mirroring useFilteredSetGroups).
const filter = ref('')
watch(game, () => {
  filter.value = ''
})
const trimmedFilter = computed(() => filter.value.trim())
const filtering = computed(() => trimmedFilter.value.length > 0)
const filteredSets = computed(() => {
  const q = trimmedFilter.value.toLowerCase()
  if (!q) return facetSets.value
  return facetSets.value.filter(
    (set) => set.name?.toLowerCase().includes(q) || set.code.toLowerCase().includes(q),
  )
})

// The public (cached) catalog set list — the same source the card landing uses — resolves each
// product set's code to its catalog row for the tile's icon + release date, and for the
// year sectioning. A set with no catalog row falls back gracefully (Package icon, no date) and
// sinks into the trailing "Unknown year" section.
const catalogSetsQuery = useSetsQuery(game)
const catalogSetByCode = computed(() => {
  const map: Record<string, CardSet> = {}
  for (const set of catalogSetsQuery.data.value?.data ?? []) map[set.code] = set
  return map
})
const releasedAtOf = (set: ProductSetRef) => catalogSetByCode.value[set.code]?.released_at ?? ''

// Bucket the (filtered) product sets into release-year sections — newest year first, undated
// sets in a trailing "Unknown year" section — resolving each set's year from the catalog row.
// Within a year the newest release leads (then code), matching the card landing's year sections.
const sections = computed(() => {
  const byYear = new Map<number | null, ProductSetRef[]>()
  for (const set of filteredSets.value) {
    const releasedAt = releasedAtOf(set)
    // Slice the leading four digits rather than parsing to a Date — avoids a timezone shift
    // across New Year (matching lib/setGroups.ts's releaseYear).
    const parsed = releasedAt ? Number.parseInt(releasedAt.slice(0, 4), 10) : NaN
    const year = Number.isNaN(parsed) ? null : parsed
    const bucket = byYear.get(year)
    if (bucket) bucket.push(set)
    else byYear.set(year, [set])
  }
  return [...byYear.entries()]
    .map(([year, sets]) => ({
      key: year === null ? 'unknown' : String(year),
      label: year === null ? 'Unknown year' : String(year),
      sets: sets.sort((a, b) => {
        // Newest release first; then code for a stable order.
        const da = releasedAtOf(a)
        const db = releasedAtOf(b)
        if (da !== db) return da < db ? 1 : -1
        return a.code.localeCompare(b.code)
      }),
    }))
    .sort((a, b) => {
      // Newest year first; undated (null) sinks to the bottom.
      const ya = a.key === 'unknown' ? null : Number(a.key)
      const yb = b.key === 'unknown' ? null : Number(b.key)
      if (ya === yb) return 0
      if (ya === null) return 1
      if (yb === null) return -1
      return yb - ya
    })
})
</script>

<template>
  <div v-if="!redirecting" class="mx-auto max-w-6xl px-4 py-10">
    <PageBreadcrumbs :items="[{ label: 'Sealed', to: '/sealed' }, { label: gameName }]" />

    <header class="mb-4">
      <h1 class="text-3xl font-semibold tracking-tight">{{ gameName }} sealed products</h1>
      <p class="text-muted-foreground mt-1">
        {{ filteredSets.length }} {{ filteredSets.length === 1 ? 'set' : 'sets' }}
        <template v-if="filtering"> matching “{{ trimmedFilter }}”</template>
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
        :to="`/sealed/${game}/products`"
        :class="buttonVariants({ variant: 'default' })"
        class="shrink-0"
      >
        <LayoutGrid />
        All products
      </RouterLink>
    </StickySearchBar>

    <SetGridSkeleton v-if="facetsQuery.isPending.value" />
    <p v-else-if="facetsQuery.isError.value" class="text-destructive py-12">
      Couldn't load sealed products. Please retry.
    </p>
    <p v-else-if="!facetSets.length" class="text-muted-foreground py-12">
      No sealed products available yet.
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
          <ProductSetTile
            v-for="set in section.sets"
            :key="set.code"
            :game="game"
            :code="set.code"
            :name="set.name"
            :products="set.product_count"
            :catalog-set="catalogSetByCode[set.code]"
            :to="`/sealed/${game}/sets/${set.code}`"
          />
        </div>
      </section>
    </div>
  </div>
</template>
