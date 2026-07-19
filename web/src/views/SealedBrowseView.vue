<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { useRoute, useRouter, type LocationQueryRaw } from 'vue-router'
import UpdatingCue from '@/components/cards/UpdatingCue.vue'
import UpdatingOverlay from '@/components/cards/UpdatingOverlay.vue'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import ProductGrid from '@/components/products/ProductGrid.vue'
import ProductGridSkeleton from '@/components/products/ProductGridSkeleton.vue'
import CardPagination from '@/components/cards/CardPagination.vue'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import CardSizeMenu from '@/components/cards/CardSizeMenu.vue'
import CardSortMenu from '@/components/cards/CardSortMenu.vue'
import StickySearchBar from '@/components/cards/StickySearchBar.vue'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useCardSearch } from '@/composables/useCardSearch'
import { useGameName } from '@/composables/useCatalog'
import { useCollectionProductCounts } from '@/composables/useCollection'
import { useWishlistProductCounts } from '@/composables/useWishlist'
import {
  PRODUCT_PAGE_SIZE,
  useProductFacetsQuery,
  useProductsQuery,
} from '@/composables/useProducts'
import { useClampPage } from '@/composables/useClampPage'
import { PRODUCT_DEFAULT_SORT, PRODUCT_SORT_OPTIONS } from '@/lib/cardSort'
import { productTypeLabel } from '@/lib/productType'
import { usePageMeta } from '@/lib/seo'

// Serves both the flat all-products browse (`/sealed/:game/products`) and a set-scoped browse
// (`/sealed/:game/sets/:code`) — the click-through target of the landing's set tiles. `code`
// (undefined = the all-products view) is the only per-route difference: it pins the set filter
// and hides the set `<Select>`.
const props = defineProps<{ game: string; code?: string }>()
const game = toRef(props, 'game')
const scoped = computed(() => !!props.code)

// Page, name search and sort live in the URL (shared with useCardSearch, same as the
// card browse views). Note `q` here matches each word as an order-independent name
// substring (all words must be present), not Scryfall syntax (issue #273).
const { page, searchInput, query, sort } = useCardSearch(
  PRODUCT_DEFAULT_SORT,
  PRODUCT_SORT_OPTIONS.map((option) => option.value),
)

// The set + type filters also live in the URL. reka's Select can't hold '' as a value
// (it reserves it for "no selection"), so an `all` sentinel means "no filter". Writes
// merge into the existing query, reset paging, and drop the key when cleared.
const route = useRoute()
const router = useRouter()
const ALL = 'all'
function patchFilter(key: 'set' | 'type', value: string) {
  const next: LocationQueryRaw = { ...route.query }
  if (value === ALL) delete next[key]
  else next[key] = value
  delete next.page
  router.replace({ query: next })
}
function readFilter(key: 'set' | 'type'): string {
  const raw = route.query[key]
  return typeof raw === 'string' && raw ? raw : ''
}
const setFilter = computed(() => readFilter('set'))
const typeFilter = computed(() => readFilter('type'))
const setSelect = computed({
  get: () => setFilter.value || ALL,
  set: (value: string) => patchFilter('set', value),
})
const typeSelect = computed({
  get: () => typeFilter.value || ALL,
  set: (value: string) => patchFilter('type', value),
})

const gameName = useGameName(game)

const facetsQuery = useProductFacetsQuery(game)
const typeOptions = computed(() => facetsQuery.data.value?.data.types ?? [])
const setOptions = computed(() => facetsQuery.data.value?.data.sets ?? [])

// Set-scoped mode: resolve the scoped set's display name from the facet list (fallback to the
// upper-cased code) for the heading + breadcrumb, and pin the products query's `set` param to
// `code` — any `?set=` in the URL is ignored (the set `<Select>` is hidden). The unscoped view
// keeps the in-URL set filter.
const scopedSetRef = computed(() => setOptions.value.find((s) => s.code === props.code))
const heading = computed(() =>
  scoped.value ? (scopedSetRef.value?.name ?? props.code?.toUpperCase() ?? '') : 'All products',
)
const effectiveSet = computed(() => (scoped.value ? (props.code ?? '') : setFilter.value))

usePageMeta({
  title: () =>
    scoped.value
      ? `${heading.value} sealed products — ${gameName.value}`
      : `${gameName.value} sealed products`,
  description: () =>
    scoped.value
      ? `Browse sealed ${gameName.value} products from ${heading.value} — booster boxes, ` +
        `bundles and decks — with current prices and price history on TCGLense.`
      : `Browse and filter sealed ${gameName.value} products — booster boxes, bundles and ` +
        `decks — with current prices and price history on TCGLense.`,
  canonicalPath: () =>
    scoped.value ? `/sealed/${game.value}/sets/${props.code}` : `/sealed/${game.value}/products`,
})

const productsQuery = useProductsQuery(game, {
  page,
  query,
  set: effectiveSet,
  type: typeFilter,
  sort,
  defaultSort: PRODUCT_DEFAULT_SORT,
})

const products = computed(() => productsQuery.data.value?.data ?? [])
const total = computed(() => productsQuery.data.value?.total ?? 0)
// Resting wanted-count badges for the tiles' quick-add controls: one POST per 60-product
// page, far under the id caps. Signed out returns `{}` (the hook is auth-safe), so the grid
// renders no controls anyway.
const { ownership: wanted } = useWishlistProductCounts(game, products)
const { ownership: owned } = useCollectionProductCounts(game, products)
// The top of the results block — paging scrolls here so the new page starts at the top
// of the grid, clearing the sticky search bar via its scroll-mt (issue #258).
const resultsTop = ref<HTMLElement | null>(null)

useClampPage(page, () => ({
  ready: productsQuery.isSuccess.value,
  total: total.value,
  pageSize: PRODUCT_PAGE_SIZE,
}))
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-10">
    <PageBreadcrumbs
      :items="[
        { label: 'Sealed', to: '/sealed' },
        { label: gameName, to: `/sealed/${game}` },
        { label: heading },
      ]"
    />

    <h1 class="mb-4 text-3xl font-semibold tracking-tight">{{ heading }}</h1>

    <StickySearchBar>
      <div class="flex flex-wrap items-center gap-2">
        <CardSearchBox
          v-model="searchInput"
          placeholder="Search sealed products…"
          aria-label="Search sealed products"
          class="min-w-48 flex-1"
        />
        <!-- The set filter is an in-page filter on the all-products view; the set-scoped view is
             pinned to its set, so it hides the select entirely. -->
        <Select v-if="!scoped" v-model="setSelect">
          <SelectTrigger size="sm" class="w-40" aria-label="Filter by set">
            <SelectValue placeholder="All sets" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem :value="ALL">All sets</SelectItem>
            <SelectItem v-for="s in setOptions" :key="s.code" :value="s.code">
              {{ s.name ?? s.code.toUpperCase() }}
            </SelectItem>
          </SelectContent>
        </Select>
        <Select v-model="typeSelect">
          <SelectTrigger size="sm" class="w-44" aria-label="Filter by product type">
            <SelectValue placeholder="All types" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem :value="ALL">All types</SelectItem>
            <SelectItem v-for="t in typeOptions" :key="t" :value="t">
              {{ productTypeLabel(t) }}
            </SelectItem>
          </SelectContent>
        </Select>
      </div>
    </StickySearchBar>

    <p class="text-muted-foreground mt-4 mb-6 text-sm">
      <template v-if="productsQuery.isFetching.value && !products.length">Searching…</template>
      <!-- Refetching over stale results (page/filter change held by keepPreviousData):
           an honest in-flight cue rather than silently showing the old total. -->
      <template v-else-if="productsQuery.isFetching.value && productsQuery.isPlaceholderData.value">
        <UpdatingCue />
      </template>
      <template v-else
        >{{ total.toLocaleString() }} {{ total === 1 ? 'product' : 'products' }}</template
      >
      <template v-if="query"> matching “{{ query }}”</template>
    </p>

    <ProductGridSkeleton v-if="productsQuery.isPending.value" />
    <p v-else-if="productsQuery.isError.value" class="text-destructive py-12">
      Couldn't load sealed products. Please retry.
    </p>
    <p v-else-if="!products.length" class="text-muted-foreground py-12">
      No sealed products found.
    </p>

    <template v-else>
      <!-- scroll-mt clears the sticky filter bar on a page change (#258). That bar wraps its
           search + up to two selects to as many as three rows on a narrow phone (~140px), so the
           mobile offset is taller; from sm up it's a single ~60px row, hence the tight sm: value. -->
      <div
        ref="resultsTop"
        class="mb-4 flex scroll-mt-40 flex-wrap justify-end gap-2 sm:scroll-mt-24"
      >
        <CardSizeMenu />
        <CardSortMenu v-model="sort" :options="PRODUCT_SORT_OPTIONS" />
      </div>
      <!-- Top pager mirrors the one below (#264) so a long grid can be paged from the top too. -->
      <div class="mb-6">
        <CardPagination
          v-model:page="page"
          :page-size="PRODUCT_PAGE_SIZE"
          :total="total"
          :loading="productsQuery.isPlaceholderData.value"
          :scroll-target="resultsTop"
        />
      </div>
      <UpdatingOverlay :loading="productsQuery.isPlaceholderData.value">
        <ProductGrid :game="game" :products="products" :owned="owned" :wanted="wanted" />
      </UpdatingOverlay>
      <div class="mt-10">
        <CardPagination
          v-model:page="page"
          :page-size="PRODUCT_PAGE_SIZE"
          :total="total"
          :loading="productsQuery.isPlaceholderData.value"
          :scroll-target="resultsTop"
        />
      </div>
    </template>
  </div>
</template>
