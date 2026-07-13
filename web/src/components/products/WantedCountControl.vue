<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { Check, Loader2, Minus, Plus } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import OwnedCountBadge from '@/components/cards/OwnedCountBadge.vue'
import { useWishlistProductEntryQuery } from '@/composables/useWishlist'
import { useOwnedCountEditor, type OwnedCountSeed } from '@/composables/useOwnedCountEditor'

// Quick-add control overlaid on a sealed-product tile (issues #95/#364): the product twin
// of OwnedCountControl. A corner trigger shows the wanted count (or a "+" on a product not
// yet wanted) and opens a small popover with a single quantity stepper, so a signed-in user
// can add to or adjust their wish list without leaving the grid. Wish-list only — sealed
// products have no collection holding — so it needs no `list` prop and no foil row (the
// seeded foil want is preserved on save; full foil-want editing lives on the product page).
// Rendered by ProductGrid only while signed in; it self-positions in ProductTile's bare
// #badge slot as a SIBLING of the stretched link, so its clicks open the popover instead of
// navigating (the CardTile idiom).
//
// `quantity`/`foilQuantity` are the *display* counts from the grid's wanted map — good
// enough for the resting badge, but they can lag (a browse grid loads them async). Because
// the save writes ABSOLUTE counts, the editor is instead seeded from the authoritative
// single-product holding fetched when the popover opens, and the steppers stay disabled
// until it resolves — so an early click can never clobber the real count with a stale zero.
const props = defineProps<{
  game: string
  productId: string
  name: string
  quantity: number
  foilQuantity: number
}>()

const open = ref(false)
const game = toRef(props, 'game')
const productId = toRef(props, 'productId')

// Fetch the authoritative want only once the popover is open (a big grid must not fire one
// request per tile). `staleTime: 0` forces a re-fetch every reopen, and `ready` waits for
// that fetch to settle (not just any prior success) so an absolute-count save never seeds
// off a stale cached count. Until ready, seed the display from the grid counts so a wanted
// product doesn't flash "0"; the steppers stay disabled, so acting on the fallback is
// impossible.
const entryQuery = useWishlistProductEntryQuery(game, productId, { enabled: open, staleTime: 0 })
const seed = computed<OwnedCountSeed>(
  () => entryQuery.data.value ?? { quantity: props.quantity, foil_quantity: props.foilQuantity },
)
const ready = computed(() => entryQuery.isSuccess.value && !entryQuery.isFetching.value)

// Product mode: only the regular want is editable here; the seeded foil want is preserved
// on save (the editor never touches `foil`). Its mutation already invalidates the summary
// trio and the wanted-count badges.
const { regular, adjust, saving, saveError } = useOwnedCountEditor(game, productId, seed, {
  kind: 'product',
})

// The resting trigger reflects the grid's display counts; the live edited count shows in
// the panel.
const displayTotal = computed(() => props.quantity + props.foilQuantity)
const wanted = computed(() => displayTotal.value > 0)
</script>

<template>
  <Popover v-model:open="open">
    <PopoverTrigger as-child>
      <!-- Sibling of ProductTile's stretched nav link (not nested inside it), so clicking it
        opens the popover instead of navigating; `.stop` is belt-and-braces. Anchored
        bottom-left over the product art. On a wanted product the count chip is always shown
        (its icons morphing to a "+" on hover/focus to signal you can add more — via the
        `group/add` tag below). On a product you don't want yet the "+" stays visible on
        small screens (and any touch device) — there's no hover to reveal it — and only hides
        behind hover/focus from sm up, to keep a dense desktop grid clean. -->
      <button
        type="button"
        class="group/add absolute bottom-2 left-2 z-20 inline-flex items-center rounded-md outline-none transition focus-visible:ring-2 focus-visible:ring-ring"
        :class="
          wanted
            ? ''
            : 'opacity-100 sm:opacity-0 sm:group-hover:opacity-100 sm:group-focus-within:opacity-100 sm:focus-visible:opacity-100 [@media(hover:none)]:opacity-100'
        "
        :aria-label="
          wanted ? `Edit copies of ${name} in your wish list` : `Add ${name} to your wish list`
        "
        @click.stop
      >
        <OwnedCountBadge
          v-if="wanted"
          :quantity="quantity"
          :foil-quantity="foilQuantity"
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
      name/price below; reka flips it if there isn't room above. -->
    <PopoverContent side="top" align="start" :side-offset="6" class="w-56 p-3">
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

      <!-- One quantity stepper (no foil counterpart for sealed products). At 0 the minus is
        inert but stays focusable (aria-disabled + a click that no-ops), not natively
        `disabled` — so decrementing the last copy while keyboard-focused here doesn't drop
        focus out of the non-modal popover. The name is in each stepper's label because a
        grid mounts many of these. -->
      <div class="flex items-center justify-between gap-3">
        <span class="text-sm">Quantity</span>
        <div class="flex items-center gap-2">
          <Button
            variant="outline"
            size="icon"
            :disabled="!ready"
            :aria-disabled="regular <= 0"
            :class="{ 'pointer-events-none opacity-50': regular <= 0 }"
            :aria-label="`Remove one ${name} from your wish list`"
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
            :aria-label="`Add one ${name} to your wish list`"
            @click="adjust('quantity', 1)"
          >
            <Plus />
          </Button>
        </div>
      </div>
    </PopoverContent>
  </Popover>
</template>
