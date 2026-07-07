<script setup lang="ts">
import { computed, toRef } from 'vue'
import { RouterLink, type RouteLocationRaw } from 'vue-router'
import { ChevronRight, Package } from '@lucide/vue'
import type { ProductComponent } from '@/lib/api'
import { cardImageUrl, productImageUrl } from '@/lib/api'
import { useProductContentsQuery } from '@/composables/useProducts'

// The sealed product's structural composition — "what's in the box". Lists the nested
// packs/boxes it bundles (each linked to its own product page), precon decks, fixed promo
// cards (linked to the card), and physical extras, with quantities. Renders nothing when
// the product has no ingested composition — a bare booster pack, or a product neither
// MTGJSON nor the curated fallback describes. Mounts off the route id, so it fetches in
// parallel with the product query above it.
const props = defineProps<{ game: string; id: string }>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')

const contentsQuery = useProductContentsQuery(game, id)

// The in-app link for a component that resolves to a catalog product or card, or null for a
// textual line item (a deck, a physical extra, or an unresolved link).
function linkTo(c: ProductComponent): RouteLocationRaw | null {
  if (c.product) return { name: 'sealed-product', params: { game: game.value, id: c.product.id } }
  if (c.card) return { name: 'card', params: { game: game.value, id: c.card.id } }
  return null
}

// A small thumbnail URL when the component links to a product or card that has art; else
// null, so a kind icon stands in (rather than a broken image for an art-less link).
function thumbUrl(c: ProductComponent): string | null {
  if (c.product?.has_image) return productImageUrl(game.value, c.product.id, 'small')
  if (c.card?.has_image) return cardImageUrl(game.value, c.card.id, 'small')
  return null
}

// Decorate each component with its resolved link + thumbnail once, so the template stays flat.
const rows = computed(() =>
  (contentsQuery.data.value?.data ?? []).map((component) => ({
    component,
    to: linkTo(component),
    thumb: thumbUrl(component),
  })),
)
const show = computed(() => rows.value.length > 0)
</script>

<template>
  <section v-if="show" class="mt-10">
    <h2 class="mb-1 text-sm font-semibold">What's in the box</h2>
    <p class="text-muted-foreground mb-4 text-xs">
      The products and extras this sealed product contains.
    </p>
    <ul class="grid gap-2 sm:grid-cols-2">
      <li v-for="(row, i) in rows" :key="i">
        <component
          :is="row.to ? RouterLink : 'div'"
          v-bind="row.to ? { to: row.to } : {}"
          class="flex items-center gap-3 rounded-lg border p-2"
          :class="row.to ? 'group hover:bg-muted/50 transition-colors' : ''"
        >
          <!-- Thumbnail: product/card art when linked + available, else a kind icon. -->
          <div
            class="bg-muted/30 flex size-14 shrink-0 items-center justify-center overflow-hidden rounded-md border"
          >
            <img
              v-if="row.thumb"
              :src="row.thumb"
              :alt="row.component.name"
              loading="lazy"
              class="h-full w-full object-contain"
            />
            <Package v-else class="text-muted-foreground size-5 opacity-60" aria-hidden="true" />
          </div>
          <!-- Quantity + name, with a chevron affordance revealed on hover for linked rows. -->
          <div class="flex min-w-0 flex-1 items-center gap-2">
            <p class="min-w-0 flex-1 truncate text-sm font-medium">
              <span class="text-muted-foreground tabular-nums">{{ row.component.quantity }}×</span>
              {{ row.component.name }}
            </p>
            <ChevronRight
              v-if="row.to"
              class="text-muted-foreground size-4 shrink-0 opacity-0 transition-opacity group-hover:opacity-100"
              aria-hidden="true"
            />
          </div>
        </component>
      </li>
    </ul>
  </section>
</template>
