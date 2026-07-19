<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { Check, Heart, Loader2, Minus, Plus, Sparkles } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import OwnedCountBadge from '@/components/cards/OwnedCountBadge.vue'
import { useCollectionEntryQuery } from '@/composables/useCollection'
import { useWishlistEntryQuery } from '@/composables/useWishlist'
import { useOwnedCountEditor, type OwnedCountSeed } from '@/composables/useOwnedCountEditor'

// Quick-add control overlaid on a card tile (issue #95): a corner trigger showing the
// owned count (or a "+" on a card you don't own yet) that opens a small popover with
// regular/foil steppers, so a signed-in user can add to or adjust their collection
// without leaving the grid. Rendered by CardGrid / CollectionGrid only while signed in.
//
// The control is COLLECTION-primary everywhere: its steppers and resting count chips are the
// card's collection holding. The popover also carries a secondary "Wish list" quick-add row
// (issue #364 follow-up) that reads/writes the card's wish-list holding — a regular-only
// stepper. When the grid knows the card's resting want (the wishlist surface, or a wish-listed
// catalog tile) it passes `wishlistSeed`, which both appends a Heart "wanted" chip to the
// resting badge (so a wish-listed card is flagged even before the popover opens) and seeds the
// row's display so the want shows at once on open instead of flashing 0.
//
// `quantity`/`foilQuantity` are the *display* counts from the grid's ownership source —
// good enough for the resting badge, but they can lag (a browse grid loads them async).
// Because the save writes ABSOLUTE counts, the editor is instead seeded from the
// authoritative single-card holding fetched when the popover opens, and the steppers stay
// disabled until it resolves — so an early click can never clobber the real count with a
// stale zero.
const props = withDefaults(
  defineProps<{
    game: string
    cardId: string
    name: string
    quantity: number
    foilQuantity: number
    // The card's resting wish-list want (regular + foil split), or undefined when the card
    // isn't wanted / the source overlay hasn't landed (issue #364 follow-up). Its total lights
    // an appended Heart "wanted" chip on the resting badge (so a wish-listed card shows even
    // when unowned), and — crucially — it seeds the wish-list row's DISPLAY the moment the
    // popover opens, so the known want shows at once instead of flashing 0 while the
    // authoritative single-card fetch is in flight. The steppers still wait for that fetch
    // before they go live, so the seed can never drive an absolute-count save — this mirrors
    // how `quantity`/`foilQuantity` seed the collection row's display above.
    wishlistSeed?: OwnedCountSeed
  }>(),
  { wishlistSeed: undefined },
)

const open = ref(false)
const game = toRef(props, 'game')
const cardId = toRef(props, 'cardId')

// Fetch the authoritative collection holding only once the popover is open (a big grid must
// not fire one request per tile). `staleTime: 0` forces a re-fetch every time it re-opens,
// and `ready` waits for that fetch to settle (not just any prior success) — otherwise a
// reopen could seed the steppers off a stale cached count and an absolute-count save would
// clobber the true value. Until ready, seed the display from the grid counts so an owned card
// doesn't flash "0"; the steppers stay disabled, so acting on the fallback is impossible.
const entryQuery = useCollectionEntryQuery(game, cardId, { enabled: open, staleTime: 0 })
const seed = computed<OwnedCountSeed>(
  () => entryQuery.data.value ?? { quantity: props.quantity, foil_quantity: props.foilQuantity },
)
const ready = computed(() => entryQuery.isSuccess.value && !entryQuery.isFetching.value)

const { regular, foil, adjust, saving, saveError } = useOwnedCountEditor(game, cardId, seed, {
  list: 'collection',
})

// Wish-list quick-add row (always present — the control is collection-primary everywhere).
// The row's entry hook stays disabled until the popover opens; the editor's mutation pick is
// setup-time and cheap. Its display seeds from the authoritative single-card want once that
// resolves, falling back to the resting `wishlistSeed` (the grid's wanted overlay) meanwhile so
// a known want shows at once instead of flashing 0 — the steppers stay disabled until
// `wishReady`, so the fallback can never drive an absolute-count save (mirrors the collection
// row's props fallback above). Only the regular want is editable here; the seeded foil want is
// preserved on save.
const wishEntryQuery = useWishlistEntryQuery(game, cardId, {
  enabled: open,
  staleTime: 0,
})
const wishSeed = computed<OwnedCountSeed | undefined>(
  () => wishEntryQuery.data.value ?? props.wishlistSeed,
)
const wishReady = computed(() => wishEntryQuery.isSuccess.value && !wishEntryQuery.isFetching.value)
const wishEditor = useOwnedCountEditor(game, cardId, wishSeed, { list: 'wishlist' })
const wishCount = wishEditor.regular
const wishTotal = computed(() => wishEditor.regular.value + wishEditor.foil.value)
// Status stays scoped per editor: `saveError` is sticky (cleared only by that editor's
// next successful save), so a merged region would pin one target's failure over the
// other's later success with no hint which write failed. The header reports the
// collection editor; the wish row carries its own compact status below. Top-level
// aliases because nested refs don't unwrap in templates.
const wishSaving = wishEditor.saving
const wishSaveError = wishEditor.saveError

// Resting trigger reflects the grid counts; the live edited counts show inside the panel.
const displayTotal = computed(() => props.quantity + props.foilQuantity)
const owned = computed(() => displayTotal.value > 0)
// The resting wanted total (regular + foil) behind the Heart chip, derived from the seed.
const wishlistTotal = computed(() =>
  props.wishlistSeed ? props.wishlistSeed.quantity + props.wishlistSeed.foil_quantity : 0,
)
const wanted = computed(() => wishlistTotal.value > 0)
// Show the resting badge when the card is owned OR wish-listed (a wish-listed-but-unowned
// card rests as a heart, not a bare "+"). Only the truly-untouched card keeps the "+".
const showBadge = computed(() => owned.value || wanted.value)
const editorTotal = computed(() => regular.value + foil.value)

const rows = computed(() => [
  { key: 'quantity' as const, label: 'Regular', value: regular.value, icon: null },
  { key: 'foil' as const, label: 'Foil', value: foil.value, icon: Sparkles },
])
</script>

<template>
  <Popover v-model:open="open">
    <PopoverTrigger as-child>
      <!-- Sibling of CardTile's stretched nav link (not nested inside it), so clicking it
        opens the popover instead of navigating; `.stop` is belt-and-braces. Anchored
        bottom-left to match the owned-count badge placement (issue #100). On a card you
        already own the count chip is always shown (its icons morphing to a "+" on
        hover/focus to signal you can add more — issue #136, via the `group/add` tag below).
        On an unowned card the "+" stays visible on small screens (and any touch device) —
        there's no hover to reveal it — and only hides behind hover/focus from sm up, to
        keep a dense desktop grid clean. -->
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
          :wanted-quantity="wishlistTotal"
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

    <!-- Opens above the bottom-left trigger (over the card art) so it doesn't cover the
      name/price below; reka flips it if there isn't room above. -->
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
          <template v-else-if="editorTotal > 0">
            <Check class="size-3.5" aria-hidden="true" />
            Saved
          </template>
        </span>
      </div>

      <div class="space-y-2">
        <div v-for="row in rows" :key="row.key" class="flex items-center justify-between gap-3">
          <span class="flex items-center gap-1.5 text-sm">
            <component :is="row.icon" v-if="row.icon" class="size-3.5" aria-hidden="true" />
            {{ row.label }}
          </span>
          <div class="flex items-center gap-2">
            <!-- At 0 the minus is inert but stays focusable (aria-disabled + a click that
              no-ops), not natively `disabled` — so decrementing the last copy while
              keyboard-focused here doesn't drop focus out of the non-modal popover. -->
            <Button
              variant="outline"
              size="icon"
              :disabled="!ready"
              :aria-disabled="row.value <= 0"
              :class="{ 'pointer-events-none opacity-50': row.value <= 0 }"
              :aria-label="`Remove one ${row.label.toLowerCase()} copy of ${name}`"
              @click="adjust(row.key, -1)"
            >
              <Minus />
            </Button>
            <span
              class="w-8 text-center text-sm font-medium tabular-nums"
              aria-live="polite"
              aria-atomic="true"
              :aria-label="`${row.label}: ${row.value}`"
              >{{ row.value }}</span
            >
            <Button
              variant="outline"
              size="icon"
              :disabled="!ready"
              :aria-label="`Add one ${row.label.toLowerCase()} copy of ${name}`"
              @click="adjust(row.key, 1)"
            >
              <Plus />
            </Button>
          </div>
        </div>
      </div>

      <!-- Wish-list quick-add (issue #364 follow-up): regular wants only (the seeded foil want
        is preserved on save). The minus at 0 is inert-but-focusable for the same popover-focus
        reason as the rows above. -->
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
              :aria-label="`Remove one copy of ${name} from your wish list`"
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
              :aria-label="`Add one copy of ${name} to your wish list`"
              @click="wishEditor.adjust('quantity', 1)"
            >
              <Plus />
            </Button>
          </div>
        </div>
        <!-- The row's own save status (the header above reports only the collection
          editor). Fixed height so it never shifts the popover as it changes. -->
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
