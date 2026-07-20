<script setup lang="ts">
import { computed, toRef } from 'vue'
import { ChevronRight, Package } from '@lucide/vue'
import { productImageUrl } from '@/lib/api'
import { productTypeLabel } from '@/lib/productType'
import { useProductContainersQuery } from '@/composables/useProducts'
import { useDetailModalLink } from '@/composables/useDetailModalLink'

// The reverse of "What's in the box": parent sealed products whose direct composition
// includes the viewed product. This primarily gives an individual booster page useful
// paths up to its booster box and bundles. It self-hides when no parent is known.
const props = defineProps<{ game: string; id: string }>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')

const containersQuery = useProductContainersQuery(game, id)
const rows = computed(() => containersQuery.data.value?.data ?? [])
const show = computed(() => rows.value.length > 0)

// Each parent opens in the shared sealed-product modal over the current route — the same
// in-place open the browse-grid tiles and the "collector booster exclusives" card links use
// (issue #485) — while the anchor keeps the canonical product page as its href for
// modifier/middle clicks, new tabs, and crawlers.
const { hrefFor, onActivate, warm } = useDetailModalLink()
</script>

<template>
  <section v-if="show">
    <h2 class="mb-1 flex items-baseline gap-2 text-base font-semibold tracking-tight">
      Included in
      <span class="text-muted-foreground text-xs font-normal">
        {{ rows.length }} product{{ rows.length === 1 ? '' : 's' }}
      </span>
    </h2>
    <p class="text-muted-foreground mb-4 text-xs">Other sealed products that contain this item.</p>
    <ul class="grid gap-2 sm:grid-cols-2">
      <li v-for="row in rows" :key="row.product.id">
        <a
          :href="hrefFor('product', game, row.product.id)"
          class="group hover:bg-muted/50 flex items-center gap-3 rounded-lg border p-2 transition-colors"
          @click="onActivate($event, 'product', game, row.product.id)"
          @pointerenter="warm('product')"
          @focusin="warm('product')"
        >
          <div
            class="bg-muted/30 flex size-14 shrink-0 items-center justify-center overflow-hidden rounded-md border"
          >
            <img
              v-if="row.product.has_image"
              :src="productImageUrl(game, row.product.id, 'small')"
              :alt="row.product.name"
              loading="lazy"
              class="h-full w-full object-contain"
            />
            <Package v-else class="text-muted-foreground size-5 opacity-60" aria-hidden="true" />
          </div>
          <div class="min-w-0 flex-1">
            <p class="truncate text-sm font-medium">{{ row.product.name }}</p>
            <p class="text-muted-foreground truncate text-xs">
              {{ productTypeLabel(row.product.product_type) }} · Contains {{ row.quantity }}× this
              product
            </p>
          </div>
          <ChevronRight
            class="text-muted-foreground size-4 shrink-0 opacity-0 transition-opacity group-hover:opacity-100"
            aria-hidden="true"
          />
        </a>
      </li>
    </ul>
  </section>
</template>
