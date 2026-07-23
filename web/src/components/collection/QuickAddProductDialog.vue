<script setup lang="ts">
import { computed, toRef } from 'vue'
import { Check, Loader2, Minus, Plus } from '@lucide/vue'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button, buttonVariants } from '@/components/ui/button'
import ProductImage from '@/components/products/ProductImage.vue'
import { useCollectionProductEntryQuery } from '@/composables/useCollection'
import { useWishlistProductEntryQuery } from '@/composables/useWishlist'
import { useOwnedCountEditor, type CardListTarget } from '@/composables/useOwnedCountEditor'
import { useCurrency } from '@/composables/useCurrency'
import { displayUsdPrice } from '@/lib/cardPrice'
import { productTypeLabel } from '@/lib/productType'
import type { Product } from '@/lib/api'

// Step two of the sealed-product quick-add (issue #364): a product IS the leaf (no
// printings/finishes), so the print picker collapses to one confirm-quantity tile.
// Opened by QuickAddBox once a product is picked. The reka dialog gives a focus trap,
// Escape-to-close, and click-outside dismissal for free.
const props = withDefaults(
  defineProps<{ game: string; product: Product | null; list?: CardListTarget }>(),
  { list: 'wishlist' },
)
const open = defineModel<boolean>('open', { required: true })
// Forwarded to the parent so it can return focus to the quick-add box on close (this
// dialog is opened programmatically, without a trigger, so reka has no element to
// restore focus to and would otherwise drop it to <body>).
const emit = defineEmits<{ closeAutoFocus: [Event] }>()

const game = toRef(props, 'game')
const productId = computed(() => props.product?.id ?? '')

// Authoritative wanted count, refetched on each open (staleTime 0) so the absolute-count
// editor seeds off the true current holding, never a stale one. Gate on
// `isSuccess && !isFetching`, not `isSuccess` alone: reopening the SAME product reuses
// the query key, so `isSuccess` stays true off the retained (possibly stale) cache while
// the staleTime-0 refetch runs — seeding then, and saving before it settles, would
// clobber the true count (mirrors QuickAddPrintDialog's guard).
const enabled = computed(() => open.value && !!props.product)
const entryQuery =
  props.list === 'wishlist'
    ? useWishlistProductEntryQuery(game, productId, { enabled, staleTime: 0 })
    : useCollectionProductEntryQuery(game, productId, { enabled, staleTime: 0 })
const ready = computed(() => entryQuery.isSuccess.value && !entryQuery.isFetching.value)
const seed = computed(() => (ready.value ? entryQuery.data.value : undefined))
const editorOptions =
  props.list === 'wishlist'
    ? ({ kind: 'product' } as const)
    : ({ kind: 'product', list: 'collection' } as const)
const { regular, adjust, saving, saveError } = useOwnedCountEditor(
  game,
  productId,
  seed,
  editorOptions,
)

const money = useCurrency()

// The USD price, falling back to the foil price for foil-only products (same idiom as
// ProductTile), thousands-grouped.
const price = computed(() => {
  if (!props.product) return null
  const pick = displayUsdPrice(props.product.prices)
  return pick ? { text: money.formatUsd(pick.amount), foil: pick.foil } : null
})
const held = computed(() => regular.value > 0)
const listName = computed(() => (props.list === 'wishlist' ? 'wish list' : 'collection'))
const verb = computed(() => (props.list === 'wishlist' ? 'want' : 'own'))
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent
      class="bg-background flex max-h-[85vh] w-[min(94vw,28rem)] flex-col overflow-hidden rounded-xl border p-6 shadow-xl"
      @close-auto-focus="emit('closeAutoFocus', $event)"
    >
      <DialogTitle class="text-lg font-semibold">
        Add <span class="text-primary">{{ product?.name }}</span>
      </DialogTitle>
      <DialogDescription class="text-muted-foreground mt-1 text-sm">
        Set how many you {{ verb }} in your {{ listName }}. Zero removes it.
      </DialogDescription>

      <!-- Body scrolls if it ever outgrows a short viewport, keeping the Done button below
        pinned to the modal's bottom edge instead of sliding off-screen. -->
      <div class="min-h-0 flex-1 overflow-y-auto">
        <div v-if="product" class="mt-4 flex gap-4">
          <ProductImage
            :game="game"
            :id="product.id"
            :name="product.name"
            :has-image="product.has_image"
            size="normal"
            class="w-28 shrink-0"
          />
          <div class="flex min-w-0 flex-1 flex-col">
            <p class="font-medium" :title="product.name">{{ product.name }}</p>
            <p class="text-muted-foreground text-sm">
              {{ product.set_name ?? product.set_code.toUpperCase() }} ·
              {{ productTypeLabel(product.product_type) }}
            </p>
            <p class="mt-1 text-sm font-medium tabular-nums">
              <template v-if="price"
                >{{ price.text
                }}<span
                  v-if="price.foil"
                  class="text-muted-foreground ml-1 text-[0.65rem] tracking-wide uppercase"
                  title="Foil price (no regular listing)"
                  >foil</span
                ></template
              >
              <span v-else class="text-muted-foreground">—</span>
            </p>

            <!-- One quantity stepper (no foil counterpart for sealed products). Disabled
            until the authoritative seed loads, so a click can never save off a stale
            zero. -->
            <div class="mt-4 flex items-center justify-between gap-3">
              <span class="text-sm">Quantity</span>
              <div class="flex items-center gap-2">
                <Button
                  variant="outline"
                  size="icon"
                  :disabled="!ready || regular <= 0"
                  :aria-label="`Remove one from your ${listName}`"
                  @click="adjust('quantity', -1)"
                >
                  <Minus />
                </Button>
                <span class="w-8 text-center text-sm font-medium tabular-nums">{{
                  ready ? regular : '—'
                }}</span>
                <Button
                  variant="outline"
                  size="icon"
                  :disabled="!ready"
                  :aria-label="`Add one to your ${listName}`"
                  @click="adjust('quantity', 1)"
                >
                  <Plus />
                </Button>
              </div>
            </div>

            <!-- Load/save status: a load failure or the in-flight count both explain why the
            steppers are disabled; otherwise the save state. Fixed height so it never shifts
            the dialog as it changes. -->
            <div
              class="text-muted-foreground mt-2 flex h-4 items-center gap-1 text-xs"
              aria-live="polite"
            >
              <template v-if="entryQuery.isError.value">
                <span class="text-destructive">Couldn't load — close and try again</span>
              </template>
              <template v-else-if="!ready">
                <Loader2 class="size-3.5 animate-spin" aria-hidden="true" />
                Loading…
              </template>
              <template v-else-if="saveError">
                <span class="text-destructive">Couldn't save — retry</span>
              </template>
              <template v-else-if="saving">
                <Loader2 class="size-3.5 animate-spin" aria-hidden="true" />
                Saving…
              </template>
              <template v-else-if="held">
                <Check class="size-3.5" aria-hidden="true" />
                Saved
              </template>
            </div>
          </div>
        </div>
      </div>

      <div class="mt-6 flex justify-end">
        <DialogClose :class="buttonVariants({ variant: 'outline' })">Done</DialogClose>
      </div>
    </DialogContent>
  </Dialog>
</template>
