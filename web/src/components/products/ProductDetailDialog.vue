<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { ChevronLeft, ChevronRight, Expand, X } from '@lucide/vue'
import { Dialog, DialogClose, DialogContent, DialogTitle } from '@/components/ui/dialog'
import ProductDetailContent from '@/components/products/ProductDetailContent.vue'
import { useProductNavStore } from '@/stores/productNav'

// The sealed-product detail modal (issue #438). ProductTile adds `?product=<id>` to the URL while
// keeping its canonical product page as the anchor href. The URL makes Back close/reopen the
// modal, preserves the browse page underneath, and allows an open modal to be shared.
const route = useRoute()
const router = useRouter()

const productId = computed(() => {
  const value = route.query.product
  return typeof value === 'string' && value ? value : null
})
const game = computed(() => {
  const param = route.params.game
  if (typeof param === 'string' && param) return param
  // Match CardDetailDialog's fallback for any future product grid on a route without a
  // `:game` path param: ProductTile carries the game in the query there.
  const q = route.query.game
  return typeof q === 'string' && q ? q : null
})
const open = computed(() => productId.value !== null && game.value !== null)

// Prev/next through the products on the page underneath. ProductGrid publishes its current
// ordered ids to the store, keeping the global dialog decoupled from each browse surface.
const nav = useProductNavStore()
const position = computed(() =>
  open.value && game.value && productId.value
    ? nav.locate(game.value, productId.value)
    : { prev: null, next: null, index: -1, total: 0 },
)
const hasNav = computed(() => position.value.index >= 0 && position.value.total > 1)

// Replace while stepping so repeated arrow navigation remains one Back-button entry.
function goTo(id: string | null) {
  if (!id) return
  void router.replace({ query: { ...route.query, product: id } })
}

function onKeydown(event: KeyboardEvent) {
  if (event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) return
  const target = event.target as HTMLElement | null
  if (
    target &&
    (target.isContentEditable ||
      target.tagName === 'INPUT' ||
      target.tagName === 'TEXTAREA' ||
      target.tagName === 'SELECT')
  ) {
    return
  }
  if (event.key === 'ArrowLeft' && position.value.prev) {
    event.preventDefault()
    goTo(position.value.prev)
  } else if (event.key === 'ArrowRight' && position.value.next) {
    event.preventDefault()
    goTo(position.value.next)
  }
}

// X / ESC / overlay click removes only the modal parameters, preserving search, sort, page,
// and every other bit of state on the browse route underneath.
function close() {
  const next = { ...route.query }
  delete next.product
  if (typeof route.params.game !== 'string' || !route.params.game) delete next.game
  void router.push({ query: next })
}
function onOpenChange(value: boolean) {
  if (!value) close()
}
</script>

<template>
  <Dialog :open="open" @update:open="onOpenChange">
    <DialogContent
      v-if="game && productId"
      class="bg-background max-h-[90svh] w-[min(96vw,64rem)] overflow-y-auto rounded-xl border p-5 shadow-lg sm:p-8"
      @keydown="onKeydown"
    >
      <DialogTitle class="sr-only">Sealed product details</DialogTitle>

      <div class="mb-4 flex items-center justify-between gap-2">
        <div class="flex items-center gap-1">
          <template v-if="hasNav">
            <button
              type="button"
              class="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex items-center justify-center rounded-md p-1.5 disabled:pointer-events-none disabled:opacity-40"
              :disabled="!position.prev"
              aria-label="Previous sealed product"
              title="Previous sealed product (←)"
              @click="goTo(position.prev)"
            >
              <ChevronLeft class="size-4" aria-hidden="true" />
            </button>
            <button
              type="button"
              class="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex items-center justify-center rounded-md p-1.5 disabled:pointer-events-none disabled:opacity-40"
              :disabled="!position.next"
              aria-label="Next sealed product"
              title="Next sealed product (→)"
              @click="goTo(position.next)"
            >
              <ChevronRight class="size-4" aria-hidden="true" />
            </button>
            <span
              class="text-muted-foreground ml-1 text-xs tabular-nums"
              aria-live="polite"
              :aria-label="`Sealed product ${position.index + 1} of ${position.total}`"
            >
              {{ position.index + 1 }} / {{ position.total }}
            </span>
          </template>
        </div>

        <div class="flex items-center gap-1">
          <RouterLink
            :to="`/sealed/${game}/${productId}`"
            class="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex items-center gap-1.5 rounded-md px-2 py-1.5 text-xs font-medium"
          >
            <Expand class="size-3.5" aria-hidden="true" />
            Open full page
          </RouterLink>
          <DialogClose
            class="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex items-center justify-center rounded-md p-1.5"
            aria-label="Close"
          >
            <X class="size-4" aria-hidden="true" />
          </DialogClose>
        </div>
      </div>

      <ProductDetailContent :game="game" :id="productId" />
    </DialogContent>
  </Dialog>
</template>
