<script setup lang="ts">
import { computed, toRef } from 'vue'
import { ArrowLeft } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import CardDetailContent from '@/components/cards/CardDetailContent.vue'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import { useCardBackLink } from '@/composables/useCardBackLink'
import { useCardQuery } from '@/composables/useCatalog'
import { cardImageUrl } from '@/lib/api'
import { absoluteUrl, usePageMeta } from '@/lib/seo'
import {
  breadcrumbList,
  cardCrumbs,
  cardMetaDescription,
  cardProductNode,
  graph,
  type Crumb,
} from '@/lib/structuredData'

// The full card-detail page. The detail body itself lives in CardDetailContent
// (shared with the browse-grid modal, CardDetailDialog); this view adds what only a
// real page needs — the per-URL meta/JSON-LD and the in-app back link. Its card query
// shares CardDetailContent's ['card', game, id] key, so the two never double-fetch.
const props = defineProps<{ game: string; id: string }>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')

// Shares CardDetailContent's ['card', game, id] key + seeding (see useCardQuery), so the
// page and an overlay never double-fetch. This view reads it only for meta/JSON-LD.
const cardQuery = useCardQuery(game, id)

const card = computed(() => cardQuery.data.value)

// Absolute URL of the card's image (via the caching proxy) for the social preview
// and JSON-LD; undefined when the card has no image.
const cardImage = computed(() =>
  card.value?.has_image ? absoluteUrl(cardImageUrl(game.value, card.value.id, 'large')) : undefined,
)

// Home › Cards › {Set} › {Card}, shared by the visible trail and the JSON-LD breadcrumb.
const crumbs = computed<Crumb[]>(() => (card.value ? cardCrumbs(game.value, card.value) : []))

usePageMeta({
  // The set name disambiguates reprints (same card across sets) in the tab title + SERP.
  title: () => (card.value ? `${card.value.name} · ${card.value.set_name}` : undefined),
  description: () => (card.value ? cardMetaDescription(card.value) : undefined),
  canonicalPath: () => (card.value ? `/cards/${game.value}/cards/${card.value.id}` : undefined),
  image: cardImage,
  type: 'product',
  // A schema.org `Product` node (name, stats, oracle text — deliberately NO `offers`; this is
  // a price tracker, not a storefront) plus a `BreadcrumbList`, in one `@graph`. Builders +
  // the no-offers rationale live in lib/structuredData.ts.
  jsonLd: () =>
    card.value
      ? graph(cardProductNode(card.value, cardImage.value), breadcrumbList(crumbs.value))
      : undefined,
})

// The in-app "back" link, mirroring the path the user arrived by (issue #18/#63).
const backLink = useCardBackLink(game, card)
// The "Set price alert" affordance (issue #525) now lives in the shared CardDetailContent body
// (near the prices), so it shows on both this page and the browse-grid modal — see that
// component and SetPriceAlertButton.
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-10">
    <!-- Hierarchy trail (mirrors the JSON-LD BreadcrumbList; adds crawlable set links). The
      back link below stays — it's history-aware (issue #18/#63), a different affordance. -->
    <PageBreadcrumbs v-if="crumbs.length" :items="crumbs" />

    <div v-if="card" class="mb-6">
      <RouterLink
        :to="backLink.to"
        class="text-muted-foreground hover:text-foreground inline-flex items-center gap-1.5 text-sm"
      >
        <ArrowLeft class="size-4" />
        {{ backLink.label }}
      </RouterLink>
    </div>

    <CardDetailContent :game="game" :id="id" />
  </div>
</template>
