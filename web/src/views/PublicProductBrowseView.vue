<script setup lang="ts">
import { computed, toRef } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import UpdatingOverlay from '@/components/cards/UpdatingOverlay.vue'
import CardPagination from '@/components/cards/CardPagination.vue'
import ProductGrid from '@/components/products/ProductGrid.vue'
import ProductGridSkeleton from '@/components/products/ProductGridSkeleton.vue'
import { useGameName } from '@/composables/useCatalog'
import { useClampPage } from '@/composables/useClampPage'
import { useCurrency } from '@/composables/useCurrency'
import {
  usePublicCollectionProductSetsQuery,
  usePublicCollectionProductSummaryQuery,
  usePublicCollectionProductsQuery,
} from '@/composables/usePublicCollection'
import { PRODUCT_HOLDING_PAGE_SIZE } from '@/composables/productHoldingQueries'
import { usePageMeta } from '@/lib/seo'
import type { OwnedCountsMap } from '@/lib/api'

// The read-only sealed-product list of a user's public collection: every owned product
// (`/u/:handle/:game/products`) or scoped to one set (`.../products/sets/:code`) — the click
// target of the public landing's sealed set tiles. The read-only public mirror of
// `ProductHoldingsBrowseView`: it drives the token-less public product queries and renders the
// grid READ-ONLY (a static owned badge showing the OWNER's counts, never the quick-add editor).
// A 404 (private/unknown handle or game) renders the not-found state, matching the public card
// browse.
const props = defineProps<{ handle: string; game: string; code?: string }>()
const handle = toRef(props, 'handle')
const game = toRef(props, 'game')
const gameName = useGameName(game)
const money = useCurrency()
const route = useRoute()
const router = useRouter()
// The owner's display handle is the username part of the URL handle (`alice-0001` → `alice`).
const username = computed(() => props.handle.replace(/-\d{1,4}$/, ''))

const scoped = computed(() => !!props.code)
const setCode = computed(() => props.code || undefined)

// The URL is the source of truth for the page number, so it survives opening a product and
// pressing Back and is shareable/reload-safe (mirroring the authed product browse).
const page = computed({
  get: () => {
    const value = Number(route.query.page)
    return Number.isInteger(value) && value > 1 ? value : 1
  },
  set: (value) => {
    const query = { ...route.query }
    if (value > 1) query.page = String(value)
    else delete query.page
    void router.replace({ query })
  },
})

// The held-product sets resolve the scoped set's display name + header stats; the landing already
// mounts this query, so vue-query dedupes.
const setsQuery = usePublicCollectionProductSetsQuery(handle, game)
const scopedSet = computed(() =>
  scoped.value ? setsQuery.data.value?.data.find((s) => s.code === props.code) : undefined,
)
const heading = computed(() =>
  scoped.value ? (scopedSet.value?.name ?? props.code?.toUpperCase() ?? '') : 'All sealed products',
)

// Header count line: the scoped set's tally + value when set-scoped, else the whole surface's
// product summary. The summary also gates the not-found state (it 404s for a private/unknown
// handle or game regardless of the set scope), same key so it dedupes.
const summaryQuery = usePublicCollectionProductSummaryQuery(handle, game)
const countProducts = computed(() =>
  scoped.value ? scopedSet.value?.unique_products : summaryQuery.data.value?.unique_products,
)
const countValue = computed(() =>
  money.formatUsd(
    scoped.value ? scopedSet.value?.total_value_usd : summaryQuery.data.value?.total_value_usd,
  ),
)
const notFound = computed(() => summaryQuery.isError.value)

// The flat (optionally set-scoped) product page. `setCode` rides the query key so a set change
// refetches, and threads through to the `?set=` param.
const productsQuery = usePublicCollectionProductsQuery(handle, game, page, setCode)
const entries = computed(() => productsQuery.data.value?.data ?? [])
const products = computed(() => entries.value.map((entry) => entry.product))
const total = computed(() => productsQuery.data.value?.total ?? 0)

// Read-only grid: the owner's owned counts come straight from the page entries and render as a
// static badge (no editor — a viewer must never edit the owner's holdings).
const owned = computed<OwnedCountsMap>(() =>
  Object.fromEntries(
    entries.value.map((entry) => [
      entry.product.id,
      { quantity: entry.quantity, foil_quantity: entry.foil_quantity },
    ]),
  ),
)

useClampPage(page, () => ({
  ready: productsQuery.isSuccess.value,
  total: total.value,
  pageSize: PRODUCT_HOLDING_PAGE_SIZE,
}))

usePageMeta({
  title: () =>
    scoped.value
      ? `${heading.value} — ${username.value}'s ${gameName.value} sealed products`
      : `${username.value}'s ${gameName.value} sealed products`,
  description: () => `${username.value}'s public ${gameName.value} sealed products on TCGLense.`,
  canonicalPath: () =>
    scoped.value
      ? `/u/${handle.value}/${game.value}/products/sets/${props.code}`
      : `/u/${handle.value}/${game.value}/products`,
})
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-10">
    <div v-if="notFound" class="py-20 text-center">
      <h1 class="text-2xl font-semibold tracking-tight">Collection not found</h1>
      <p class="text-muted-foreground mt-2">This collection is private or doesn't exist.</p>
      <RouterLink to="/" class="text-primary mt-4 inline-block underline underline-offset-2">
        Go home
      </RouterLink>
    </div>

    <template v-else>
      <PageBreadcrumbs
        :items="[
          { label: `@${username}`, to: `/u/${handle}` },
          { label: gameName, to: `/u/${handle}/${game}` },
          { label: scoped ? heading : 'Sealed products' },
        ]"
      />

      <header class="mb-6">
        <h1 class="text-3xl font-semibold tracking-tight">
          <template v-if="scoped">{{ heading }}</template>
          <template v-else>{{ username }}'s {{ gameName }} sealed products</template>
        </h1>
        <p v-if="countProducts != null" class="text-muted-foreground mt-1 text-sm tabular-nums">
          {{ countProducts.toLocaleString() }} {{ countProducts === 1 ? 'product' : 'products' }}
          <template v-if="countValue"> · {{ countValue }}</template>
        </p>
      </header>

      <ProductGridSkeleton v-if="productsQuery.isPending.value" />
      <p v-else-if="productsQuery.isError.value" class="text-destructive py-12">
        Couldn't load sealed products. Please retry.
      </p>
      <p v-else-if="!products.length" class="text-muted-foreground py-12">
        {{ username }} owns no sealed products here.
      </p>

      <template v-else>
        <UpdatingOverlay :loading="productsQuery.isPlaceholderData.value">
          <ProductGrid :game="game" :products="products" :owned="owned" readonly />
        </UpdatingOverlay>
        <CardPagination
          v-if="total > PRODUCT_HOLDING_PAGE_SIZE"
          v-model:page="page"
          :page-size="PRODUCT_HOLDING_PAGE_SIZE"
          :total="total"
          :loading="productsQuery.isPlaceholderData.value"
          class="mt-10"
        />
      </template>
    </template>
  </div>
</template>
