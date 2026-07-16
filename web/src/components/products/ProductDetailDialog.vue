<script setup lang="ts">
import DetailDialogShell from '@/components/shared/DetailDialogShell.vue'
import ProductDetailContent from '@/components/products/ProductDetailContent.vue'
import { PRODUCT_CARDS_MODAL_SEARCH_KEYS } from '@/composables/useProductCardsSearch'
import { useProductNavStore } from '@/stores/productNav'

// The sealed-product detail modal (issue #438): the shared shell, opened on `?product=<id>`,
// wrapped around the product body. The frame — URL-driven open/close, prev/next through the
// browse grid underneath, the arrow keys, the escape hatches — is DetailDialogShell's; this
// names the sealed surface's half.
const nav = useProductNavStore()

// The canonical, crawlable product page the "Open full page" escape hatch links to.
const canonical = (game: string, id: string) => `/sealed/${game}/${id}`

// Unlike the card body, this one carries URL-backed state of its own: the contained-cards list
// searches and sorts through the query. It overlays a browse route that already owns `?q=`/
// `?sort=`, so it takes namespaced keys — per-product state the shell drops on stepping to a
// neighbour and on close alike (issue #448).
const ownedKeys = Object.values(PRODUCT_CARDS_MODAL_SEARCH_KEYS)
</script>

<template>
  <DetailDialogShell
    query-key="product"
    noun="sealed product"
    :canonical="canonical"
    :nav="nav"
    :owned-keys="ownedKeys"
  >
    <template #default="{ game, id }">
      <ProductDetailContent :game="game" :id="id" :search-keys="PRODUCT_CARDS_MODAL_SEARCH_KEYS" />
    </template>
  </DetailDialogShell>
</template>
