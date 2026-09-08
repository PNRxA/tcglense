<script setup lang="ts">
import DetailDialogShell from '@/components/shared/DetailDialogShell.vue'
import ProductDetailContent from '@/components/products/ProductDetailContent.vue'
import {
  PRODUCT_CARDS_MODAL_SEARCH_KEYS,
  PRODUCT_OPENER_MODAL_KEYS,
} from '@/composables/useProductCardsSearch'
import { useProductNavStore } from '@/stores/productNav'

// The sealed-product detail modal (issue #438): the shared shell, opened on `?product=<id>`,
// wrapped around the product body. The frame — URL-driven open/close, prev/next through the
// browse grid underneath, the arrow keys, the escape hatches — is DetailDialogShell's; this
// names the sealed surface's half.
const nav = useProductNavStore()

// The canonical, crawlable product page the "Open full page" escape hatch links to.
const canonical = (game: string, id: string) => `/sealed/${game}/${id}`

// Unlike the card body, this one carries URL-backed state of its own: the contained-cards list
// searches and sorts through the query (issue #448), and the pack opener puts its seed + copies
// there so a run stays shareable (issue #682). Both overlay a browse route that already owns
// `?q=`/`?sort=`, so both take namespaced keys — per-product state the shell drops on stepping
// to a neighbour and on close alike. The opener's pair MUST be listed here: a seed left behind
// in the browse URL is not inert the way a stale search string is — the next product opened
// would read it, auto-deal, and fire an `/open` request for up to 36 packs nobody asked for.
const ownedKeys = [
  ...Object.values(PRODUCT_CARDS_MODAL_SEARCH_KEYS),
  ...Object.values(PRODUCT_OPENER_MODAL_KEYS),
]
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
      <ProductDetailContent
        :game="game"
        :id="id"
        :search-keys="PRODUCT_CARDS_MODAL_SEARCH_KEYS"
        :opener-keys="PRODUCT_OPENER_MODAL_KEYS"
      />
    </template>
  </DetailDialogShell>
</template>
