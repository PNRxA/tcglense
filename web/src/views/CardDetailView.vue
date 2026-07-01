<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { useQuery } from '@tanstack/vue-query'
import { ArrowLeft, Loader2 } from '@lucide/vue'
import { RouterLink, onBeforeRouteUpdate, useRouter } from 'vue-router'
import CardImageZoom from '@/components/cards/CardImageZoom.vue'
import CardPrints from '@/components/cards/CardPrints.vue'
import PriceChart from '@/components/cards/PriceChart.vue'
import { cardImageUrl, getCard } from '@/lib/api'
import { absoluteUrl, usePageMeta } from '@/lib/seo'

const props = defineProps<{ game: string; id: string }>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')

const router = useRouter()

const cardQuery = useQuery({
  queryKey: ['card', game, id],
  queryFn: () => getCard(game.value, id.value),
  staleTime: 5 * 60 * 1000,
})

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

// The in-app location we arrived from. vue-router records the previous entry's path
// in history state (null on a direct load or a freshly-opened tab); we resolve it to
// a route so the back link can mirror the user's actual path — the all-cards list, a
// search, or a set page — rather than always pointing at the set (issue #18). Held in
// a ref and refreshed on each card→card navigation (e.g. clicking another printing in
// "Other printings"), since this view is reused across those routes and so setup()
// won't re-run — captured once, the link would stay frozen on the first card's
// referrer (issue #63). Falls back to the card's set otherwise.
const cameFrom = ref(router.options.history.state.back)
onBeforeRouteUpdate((_to, from) => {
  cameFrom.value = from.fullPath
})
const cameFromRoute = computed(() =>
  typeof cameFrom.value === 'string' ? router.resolve(cameFrom.value) : null,
)

// Labels for the catalog list routes a card is reachable from, keyed by route name.
const FROM_LABELS: Record<string, string> = {
  'game-cards': 'All cards',
  game: 'All sets',
}

const backLink = computed(() => {
  const from = cameFromRoute.value
  // Honour the previous page only when it's an in-app catalog list for this game;
  // a deep link or an unrelated referrer falls through to the set page below.
  if (from && from.params.game === game.value) {
    if (from.name === 'set') {
      // Came from a set page: every card there is in that set, so the set name fits.
      return { to: from.fullPath, label: card.value?.set_name ?? 'Set' }
    }
    const label = FROM_LABELS[from.name as string]
    if (label) return { to: from.fullPath, label }
  }
  return {
    to: card.value ? `/cards/${game.value}/sets/${card.value.set_code}` : `/cards/${game.value}`,
    label: card.value?.set_name ?? 'Back',
  }
})

// Layouts whose faces carry their OWN images (so we render one image per face).
// Split / flip / adventure / aftermath also have two faces, but only a single
// top-level image — those use the one-image path so we don't 404 per face.
const SEPARATE_FACE_IMAGE_LAYOUTS = [
  'transform',
  'modal_dfc',
  'double_faced_token',
  'reversible_card',
  'battle',
  'art_series',
]
const isMultiFace = computed(() => (card.value?.faces.length ?? 0) >= 2)
const hasSeparateFaceImages = computed(
  () => isMultiFace.value && SEPARATE_FACE_IMAGE_LAYOUTS.includes(card.value?.layout ?? ''),
)

const priceRows = computed(() => {
  const p = card.value?.prices
  if (!p) return []
  return [
    { label: 'USD', value: p.usd ? `$${p.usd}` : null },
    { label: 'USD foil', value: p.usd_foil ? `$${p.usd_foil}` : null },
    { label: 'EUR', value: p.eur ? `€${p.eur}` : null },
    { label: 'MTGO tix', value: p.tix ?? null },
  ].filter((row) => row.value)
})
</script>

<template>
  <div class="mx-auto max-w-5xl px-4 py-10">
    <div
      v-if="cardQuery.isPending.value"
      class="text-muted-foreground flex items-center gap-2 py-12"
    >
      <Loader2 class="size-4 animate-spin" />
      Loading card…
    </div>
    <p v-else-if="cardQuery.isError.value || !card" class="text-destructive py-12">
      Card not found.
    </p>

    <template v-else-if="card">
      <RouterLink
        :to="backLink.to"
        class="text-muted-foreground hover:text-foreground mb-6 inline-flex items-center gap-1.5 text-sm"
      >
        <ArrowLeft class="size-4" />
        {{ backLink.label }}
      </RouterLink>

      <div class="grid gap-8 md:grid-cols-[minmax(0,20rem)_1fr]">
        <!-- Image(s): one per face only for layouts with separate face images.
          Each is clickable to enlarge it in a lightbox (issue #53). -->
        <div class="flex gap-4" :class="hasSeparateFaceImages ? 'flex-row md:flex-col' : ''">
          <template v-if="hasSeparateFaceImages">
            <CardImageZoom
              v-for="(face, index) in card.faces"
              :key="index"
              :game="game"
              :id="card.id"
              :name="face.name ?? card.name"
              :face="index"
              class="w-full"
            />
          </template>
          <CardImageZoom
            v-else
            :game="game"
            :id="card.id"
            :name="card.name"
            :has-image="card.has_image"
            class="w-full"
          />
        </div>

        <!-- Details -->
        <div>
          <h1 class="text-3xl font-semibold tracking-tight">{{ card.name }}</h1>
          <p v-if="card.type_line" class="text-muted-foreground mt-1">{{ card.type_line }}</p>

          <dl class="mt-6 grid grid-cols-[8rem_1fr] gap-x-4 gap-y-2 text-sm">
            <dt class="text-muted-foreground">Set</dt>
            <dd>
              <RouterLink :to="`/cards/${game}/sets/${card.set_code}`" class="hover:underline">
                {{ card.set_name }} ({{ card.set_code.toUpperCase() }})
              </RouterLink>
            </dd>

            <template v-if="card.drop_name">
              <dt class="text-muted-foreground">Drop</dt>
              <dd>{{ card.drop_name }}</dd>
            </template>

            <dt class="text-muted-foreground">Number</dt>
            <dd>#{{ card.collector_number }}</dd>

            <template v-if="card.rarity">
              <dt class="text-muted-foreground">Rarity</dt>
              <dd class="capitalize">{{ card.rarity }}</dd>
            </template>

            <template v-if="card.mana_cost">
              <dt class="text-muted-foreground">Mana cost</dt>
              <dd>{{ card.mana_cost }}</dd>
            </template>

            <template v-if="card.color_identity.length">
              <dt class="text-muted-foreground">Color identity</dt>
              <dd>{{ card.color_identity.join(', ') }}</dd>
            </template>

            <template v-if="!isMultiFace && card.power && card.toughness">
              <dt class="text-muted-foreground">Power / Toughness</dt>
              <dd class="tabular-nums">{{ card.power }} / {{ card.toughness }}</dd>
            </template>

            <template v-if="!isMultiFace && card.loyalty">
              <dt class="text-muted-foreground">Loyalty</dt>
              <dd class="tabular-nums">{{ card.loyalty }}</dd>
            </template>

            <template v-if="card.released_at">
              <dt class="text-muted-foreground">Released</dt>
              <dd>{{ card.released_at }}</dd>
            </template>
          </dl>

          <!-- Oracle text (single-faced cards; multi-faced show text per face below). -->
          <p
            v-if="!isMultiFace && card.oracle_text"
            class="mt-6 text-sm leading-relaxed whitespace-pre-line"
          >
            {{ card.oracle_text }}
          </p>

          <!-- Per-face text breakdown for any multi-faced card (incl. split/flip). -->
          <div v-if="isMultiFace" class="mt-6 space-y-3">
            <div v-for="(face, index) in card.faces" :key="index" class="rounded-lg border p-3">
              <p class="font-medium">{{ face.name }}</p>
              <p v-if="face.type_line" class="text-muted-foreground text-sm">
                {{ face.type_line }}
              </p>
              <p v-if="face.mana_cost" class="text-muted-foreground text-sm">
                {{ face.mana_cost }}
              </p>
              <p v-if="face.oracle_text" class="mt-1 text-sm leading-relaxed whitespace-pre-line">
                {{ face.oracle_text }}
              </p>
              <p
                v-if="face.power && face.toughness"
                class="text-muted-foreground mt-1 text-sm tabular-nums"
              >
                {{ face.power }} / {{ face.toughness }}
              </p>
              <p v-if="face.loyalty" class="text-muted-foreground mt-1 text-sm tabular-nums">
                Loyalty {{ face.loyalty }}
              </p>
            </div>
          </div>

          <!-- Prices -->
          <div v-if="priceRows.length" class="mt-6">
            <h2 class="mb-2 text-sm font-semibold">Prices</h2>
            <dl class="grid grid-cols-2 gap-2 sm:grid-cols-4">
              <div
                v-for="row in priceRows"
                :key="row.label"
                class="bg-muted/50 rounded-lg border p-3"
              >
                <dt class="text-muted-foreground text-xs">{{ row.label }}</dt>
                <dd class="mt-0.5 font-medium tabular-nums">{{ row.value }}</dd>
              </div>
            </dl>
          </div>
        </div>
      </div>

      <!-- Price history over time (full width, below the card details). -->
      <PriceChart :game="game" :id="card.id" />

      <!-- This card's other printings (same gameplay object, issue #63). Renders
        nothing when there are none. -->
      <CardPrints :game="game" :id="card.id" />
    </template>
  </div>
</template>
