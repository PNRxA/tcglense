<script setup lang="ts">
import { computed, toRef } from 'vue'
import { ArrowLeft } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import CardDetailContent from '@/components/cards/CardDetailContent.vue'
import { useCardBackLink } from '@/composables/useCardBackLink'
import { useCardQuery } from '@/composables/useCatalog'
import { cardImageUrl } from '@/lib/api'
import { absoluteUrl, usePageMeta } from '@/lib/seo'

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

const metaDescription = computed(() => {
  const c = card.value
  if (!c) return undefined
  const bits = [c.type_line, `${c.set_name} · #${c.collector_number}`].filter(
    (bit): bit is string => Boolean(bit),
  )
  return `${c.name} — ${bits.join(' · ')}. Prices and price history on TCGLense.`
})

// Product structured data (schema.org) so search engines can identify the card as
// a collectible product. We deliberately DON'T emit an `offers`/`InStock` block:
// this is a price-tracking page, not a storefront, so claiming the card is
// purchasable here would be a false structured-data assertion (and risks Google
// suppressing the rich result). Prices are shown to users but not marked up as an
// offer to sell.
const jsonLd = computed<Record<string, unknown> | undefined>(() => {
  const c = card.value
  if (!c) return undefined
  const data: Record<string, unknown> = {
    '@context': 'https://schema.org',
    '@type': 'Product',
    name: c.name,
    brand: { '@type': 'Brand', name: c.set_name },
  }
  if (cardImage.value) data.image = cardImage.value
  if (c.type_line) data.category = c.type_line
  return data
})

usePageMeta({
  title: () => card.value?.name,
  description: metaDescription,
  canonicalPath: () => (card.value ? `/cards/${game.value}/cards/${card.value.id}` : undefined),
  image: cardImage,
  type: 'product',
  jsonLd,
})

// The in-app "back" link, mirroring the path the user arrived by (issue #18/#63).
const backLink = useCardBackLink(game, card)
</script>

<template>
  <div class="mx-auto max-w-5xl px-4 py-10">
    <RouterLink
      v-if="card"
      :to="backLink.to"
      class="text-muted-foreground hover:text-foreground mb-6 inline-flex items-center gap-1.5 text-sm"
    >
      <ArrowLeft class="size-4" />
      {{ backLink.label }}
    </RouterLink>

    <CardDetailContent :game="game" :id="id" />
  </div>
</template>
