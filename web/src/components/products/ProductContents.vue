<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { ChevronDown, ChevronRight, Package } from '@lucide/vue'
import type { ProductComponent } from '@/lib/api'
import { cardImageUrl, productImageUrl } from '@/lib/api'
import { useProductCardSectionsQuery, useProductContentsQuery } from '@/composables/useProducts'
import { boxItemCount } from '@/lib/productCounts'
import { useDetailModalLink, type DetailModalKind } from '@/composables/useDetailModalLink'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'

// The sealed product's structural composition — "what's in the box". Lists the nested
// packs/boxes it bundles (each linked to its own product page), precon decks, fixed promo
// cards (linked to the card), and physical extras, with quantities. Renders nothing when
// the product has no ingested composition — a bare booster pack, or a product neither
// MTGJSON nor the curated fallback describes. Mounts off the route id, so it fetches in
// parallel with the product query above it.
const props = defineProps<{ game: string; id: string }>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')

// A component that is *not* individually sold has no page to open — but when the card
// manifest carries a section named after it (an unlisted sub-product's packed cards), the
// row scrolls down to that section instead: the parent (ProductDetailContent) routes this
// to ProductCards, which expands + scrolls the matching block(s).
const emit = defineEmits<{ openComponent: [name: string] }>()

const contentsQuery = useProductContentsQuery(game, id)
// The unfiltered sections manifest — the same key ProductOverview and ProductCards (no
// committed search) read, so this adds no fetch. Only the component names are used here,
// to decide which unlinked rows are scroll-through targets.
const sectionsQuery = useProductCardSectionsQuery(game, id, ref(''))
const componentSections = computed(() => {
  const names = new Set<string>()
  for (const section of sectionsQuery.data.value?.data ?? []) {
    if (section.component) names.add(section.component)
  }
  return names
})

// Nested packs/boxes and fixed promo cards both open in the shared detail modal over the current
// route — the same in-place open as the browse-grid tiles and the "collector booster exclusives"
// card links (issue #485) — while the anchor keeps the canonical page as its href for
// modifier/middle clicks, new tabs, and crawlers.
const { hrefFor, onActivate, warm } = useDetailModalLink()

// The in-app detail-modal target for a component that resolves to a catalog product or card, or
// null for a textual line item (a deck, a physical extra, or an unresolved link).
function linkFor(c: ProductComponent): { kind: DetailModalKind; id: string; href: string } | null {
  if (c.product) {
    return { kind: 'product', id: c.product.id, href: hrefFor('product', game.value, c.product.id) }
  }
  if (c.card) return { kind: 'card', id: c.card.id, href: hrefFor('card', game.value, c.card.id) }
  return null
}

// A small thumbnail URL when the component links to a product or card that has art; else
// null, so a kind icon stands in (rather than a broken image for an art-less link).
function thumbUrl(c: ProductComponent): string | null {
  if (c.product?.has_image) return productImageUrl(game.value, c.product.id, 'small')
  if (c.card?.has_image) return cardImageUrl(game.value, c.card.id, 'small')
  return null
}

// Decorate each component with its resolved link + thumbnail + scroll-target flag once, so
// the template stays flat. A row is a scroll target only when it has no page of its own to
// link AND the manifest holds sections named after it.
const rows = computed(() =>
  (contentsQuery.data.value?.data ?? []).map((component) => {
    const link = linkFor(component)
    return {
      component,
      link,
      thumb: thumbUrl(component),
      scrolls: !link && componentSections.value.has(component.name),
    }
  }),
)
const show = computed(() => rows.value.length > 0)
// The physical piece count, not the row count — it must agree with the `30×` quantities the
// rows below show, and with ProductOverview's "items in the box" chip.
const itemTotal = computed(() => boxItemCount(contentsQuery.data.value?.data ?? []))

// The element a row renders as: a link (modal-opening anchor), a scroll-through button,
// or a plain div for purely textual line items.
function rowTag(row: { link: unknown; scrolls: boolean }): string {
  if (row.link) return 'a'
  return row.scrolls ? 'button' : 'div'
}
</script>

<template>
  <section v-if="show">
    <h2 class="mb-1 flex items-baseline gap-2 text-base font-semibold tracking-tight">
      What's in the box
      <span class="text-muted-foreground text-xs font-normal">
        {{ itemTotal.toLocaleString() }} item{{ itemTotal === 1 ? '' : 's' }}
      </span>
    </h2>
    <p class="text-muted-foreground mb-4 text-xs">
      The products and extras this sealed product contains.
    </p>
    <ul class="grid gap-2 sm:grid-cols-2">
      <li v-for="(row, i) in rows" :key="i">
        <component
          :is="rowTag(row)"
          :href="row.link?.href"
          :type="rowTag(row) === 'button' ? 'button' : undefined"
          class="flex w-full items-center gap-3 rounded-lg border p-2 text-left"
          :class="row.link || row.scrolls ? 'group hover:bg-muted/50 transition-colors' : ''"
          @click="
            row.link
              ? onActivate($event, row.link.kind, game, row.link.id)
              : row.scrolls && emit('openComponent', row.component.name)
          "
          @pointerenter="row.link && warm(row.link.kind)"
          @focusin="row.link && warm(row.link.kind)"
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
          <!-- Quantity + name (a tooltip carries the full name — long ones truncate here),
            with an affordance chevron revealed on hover: right for a linked row (opens the
            child's page), down for a scroll-through row (jumps to its card section). -->
          <div class="flex min-w-0 flex-1 items-center gap-2">
            <Tooltip>
              <TooltipTrigger as-child>
                <p class="min-w-0 flex-1 truncate text-sm font-medium">
                  <span class="text-muted-foreground tabular-nums"
                    >{{ row.component.quantity }}×</span
                  >
                  {{ row.component.name }}
                </p>
              </TooltipTrigger>
              <TooltipContent>{{ row.component.name }}</TooltipContent>
            </Tooltip>
            <ChevronRight
              v-if="row.link"
              class="text-muted-foreground size-4 shrink-0 opacity-0 transition-opacity group-hover:opacity-100"
              aria-hidden="true"
            />
            <ChevronDown
              v-else-if="row.scrolls"
              class="text-muted-foreground size-4 shrink-0 opacity-0 transition-opacity group-hover:opacity-100"
              aria-hidden="true"
            />
          </div>
        </component>
      </li>
    </ul>
  </section>
</template>
