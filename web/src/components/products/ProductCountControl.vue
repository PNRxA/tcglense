<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { Check, Heart, Layers, Loader2, Minus, Plus } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import OwnedCountBadge from '@/components/cards/OwnedCountBadge.vue'
import { useCollectionProductEntryQuery } from '@/composables/useCollection'
import { useWishlistProductEntryQuery } from '@/composables/useWishlist'
import { useOwnedCountEditor, type OwnedCountSeed } from '@/composables/useOwnedCountEditor'

// Unified quick-add control overlaid on a sealed-product tile (issues #95/#364/#435): the
// product twin of OwnedCountControl. It is collection-primary everywhere, rests at the
// bottom-left, appends a wanted Heart when applicable, and edits both Collection and Wish
// list in one popover. Hidden foil counts are preserved on both saves.
//
// `quantity`/`foilQuantity` are the *display* counts from the grid's collection map — good
// enough for the resting badge, but they can lag (a browse grid loads them async). Because
// saves write ABSOLUTE counts, each editor is instead seeded from its authoritative
// single-product holding fetched when the popover opens. Its stepper stays disabled until
// that seed resolves, so an early click cannot clobber a real count with a stale zero.
const props = withDefaults(
  defineProps<{
    game: string
    productId: string
    name: string
    quantity: number
    foilQuantity: number
    wishlistQuantity?: number
  }>(),
  { wishlistQuantity: 0 },
)

const open = ref(false)
const game = toRef(props, 'game')
const productId = toRef(props, 'productId')

// Fetch the authoritative holding only once the popover is open (a big grid must not fire one
// request per tile). `staleTime: 0` forces a re-fetch every reopen, and `ready` waits for
// that fetch to settle (not just any prior success) so an absolute-count save never seeds
// off a stale cached count. Until ready, seed the display from the grid counts so an owned
// product doesn't flash "0"; the steppers stay disabled, so acting on the fallback is
// impossible.
const entryQuery = useCollectionProductEntryQuery(game, productId, {
  enabled: open,
  staleTime: 0,
})
const seed = computed<OwnedCountSeed>(
  () => entryQuery.data.value ?? { quantity: props.quantity, foil_quantity: props.foilQuantity },
)
const ready = computed(() => entryQuery.isSuccess.value && !entryQuery.isFetching.value)

// Product mode exposes only quantity; the seeded foil count is preserved on save.
const { regular, adjust, saving, saveError } = useOwnedCountEditor(game, productId, seed, {
  kind: 'product',
  list: 'collection',
})

// The wish-list row has its own lazy authoritative seed and save status. There is no display
// fallback because the row remains disabled until the query resolves; the resting Heart
// comes from the batched grid overlay instead.
const wishEntryQuery = useWishlistProductEntryQuery(game, productId, {
  enabled: open,
  staleTime: 0,
})
const wishSeed = computed<OwnedCountSeed | undefined>(() => wishEntryQuery.data.value)
const wishReady = computed(() => wishEntryQuery.isSuccess.value && !wishEntryQuery.isFetching.value)
const wishEditor = useOwnedCountEditor(game, productId, wishSeed, {
  kind: 'product',
  list: 'wishlist',
})
const wishCount = wishEditor.regular
const wishTotal = computed(() => wishEditor.regular.value + wishEditor.foil.value)
const wishSaving = wishEditor.saving
const wishSaveError = wishEditor.saveError

// The resting trigger reflects the grid overlays; live edited counts stay inside the panel.
const displayTotal = computed(() => props.quantity + props.foilQuantity)
const owned = computed(() => displayTotal.value > 0)
const wanted = computed(() => props.wishlistQuantity > 0)
const showBadge = computed(() => owned.value || wanted.value)
</script>

<template>
  <Popover v-model:open="open">
    <PopoverTrigger as-child>
      <!-- Sibling of ProductTile's stretched nav link (not nested inside it), so clicking it
        opens the popover instead of navigating; `.stop` is belt-and-braces. Anchored
        bottom-left over the product art, matching CardGrid. On a held or wanted product the
        combined badge is always shown (its icons morph to "+" on hover/focus). On an
        untouched product the "+" stays visible on
        small screens (and any touch device) — there's no hover to reveal it — and only hides
        behind hover/focus from sm up, to keep a dense desktop grid clean. -->
      <button
        type="button"
        class="group/add absolute bottom-1.5 left-1.5 z-20 inline-flex items-center rounded-md outline-none transition focus-visible:ring-2 focus-visible:ring-ring"
        :class="
          showBadge
            ? ''
            : 'opacity-100 sm:opacity-0 sm:group-hover:opacity-100 sm:group-focus-within:opacity-100 sm:focus-visible:opacity-100 [@media(hover:none)]:opacity-100'
        "
        :aria-label="
          owned ? `Edit copies of ${name} in your collection` : `Add ${name} to your collection`
        "
        @click.stop
      >
        <OwnedCountBadge
          v-if="showBadge"
          :quantity="quantity"
          :foil-quantity="foilQuantity"
          kind="owned"
          :wanted-quantity="wishlistQuantity"
          :tooltip="false"
          hover-as-add
        />
        <span
          v-else
          class="bg-primary/90 text-primary-foreground inline-flex items-center justify-center rounded-md p-1.5 shadow"
        >
          <Plus class="size-4" aria-hidden="true" />
        </span>
      </button>
    </PopoverTrigger>

    <!-- Opens above the bottom-left trigger (over the product art) so it doesn't cover the
      name/price below; reka flips it if there isn't room above. Both list targets live in
      this one panel, matching the card-grid quick add. -->
    <PopoverContent side="top" align="start" :side-offset="6" class="w-60 p-3">
      <div class="mb-3 flex items-center justify-between gap-2">
        <p class="truncate text-sm font-medium" :title="name">{{ name }}</p>
        <span
          class="text-muted-foreground flex shrink-0 items-center gap-1 text-xs"
          aria-live="polite"
        >
          <template v-if="saveError">
            <span class="text-destructive">Retry</span>
          </template>
          <template v-else-if="saving">
            <Loader2 class="size-3.5 animate-spin" aria-hidden="true" />
            Saving…
          </template>
          <template v-else-if="regular > 0">
            <Check class="size-3.5" aria-hidden="true" />
            Saved
          </template>
        </span>
      </div>

      <!-- Collection quantity (no foil counterpart for sealed products). At 0 the minus is
        inert but stays focusable (aria-disabled + a click that no-ops), not natively
        `disabled` — so decrementing the last copy while keyboard-focused here doesn't drop
        focus out of the non-modal popover. The name is in each stepper's label because a
        grid mounts many of these. -->
      <div class="flex items-center justify-between gap-3">
        <span class="flex items-center gap-1.5 text-sm">
          <Layers class="size-3.5" aria-hidden="true" />
          Collection
        </span>
        <div class="flex items-center gap-2">
          <Button
            variant="outline"
            size="icon"
            :disabled="!ready"
            :aria-disabled="regular <= 0"
            :class="{ 'pointer-events-none opacity-50': regular <= 0 }"
            :aria-label="`Remove one ${name} from your collection`"
            @click="adjust('quantity', -1)"
          >
            <Minus />
          </Button>
          <span
            class="w-8 text-center text-sm font-medium tabular-nums"
            aria-live="polite"
            aria-atomic="true"
            :aria-label="`Quantity: ${regular}`"
            >{{ regular }}</span
          >
          <Button
            variant="outline"
            size="icon"
            :disabled="!ready"
            :aria-label="`Add one ${name} to your collection`"
            @click="adjust('quantity', 1)"
          >
            <Plus />
          </Button>
        </div>
      </div>

      <div class="mt-3 border-t pt-2">
        <div class="flex items-center justify-between gap-3">
          <span class="flex items-center gap-1.5 text-sm whitespace-nowrap">
            <Heart class="size-3.5" aria-hidden="true" />
            Wish list
          </span>
          <div class="flex items-center gap-2">
            <Button
              variant="outline"
              size="icon"
              :disabled="!wishReady"
              :aria-disabled="wishCount <= 0"
              :class="{ 'pointer-events-none opacity-50': wishCount <= 0 }"
              :aria-label="`Remove one ${name} from your wish list`"
              @click="wishEditor.adjust('quantity', -1)"
            >
              <Minus />
            </Button>
            <span
              class="w-8 text-center text-sm font-medium tabular-nums"
              aria-live="polite"
              aria-atomic="true"
              :aria-label="`Wish list: ${wishCount}`"
              >{{ wishCount }}</span
            >
            <Button
              variant="outline"
              size="icon"
              :disabled="!wishReady"
              :aria-label="`Add one ${name} to your wish list`"
              @click="wishEditor.adjust('quantity', 1)"
            >
              <Plus />
            </Button>
          </div>
        </div>
        <div
          class="text-muted-foreground mt-1 flex h-4 items-center gap-1 text-xs"
          aria-live="polite"
        >
          <template v-if="wishSaveError">
            <span class="text-destructive">Retry — not saved</span>
          </template>
          <template v-else-if="wishSaving">
            <Loader2 class="size-3.5 animate-spin" aria-hidden="true" />
            Saving…
          </template>
          <template v-else-if="wishTotal > 0">
            <Check class="size-3.5" aria-hidden="true" />
            Saved
          </template>
        </div>
      </div>
    </PopoverContent>
  </Popover>
</template>
