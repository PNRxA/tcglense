<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  Check,
  Copy,
  ExternalLink,
  LoaderCircle,
  ShoppingCart,
  TriangleAlert,
  X,
} from '@lucide/vue'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import { useCurrency } from '@/composables/useCurrency'
import { type BuyList, type BuyListParams, getWishlistBuyList } from '@/lib/api'
import { type BulkBuyOption, bulkBuyOptionsFor } from '@/lib/bulkBuy'
import { useAuthedQuery } from '@/lib/queries'

// "Buy all" (issue #292): send the wish list — or the filtered slice of it on screen — to a
// store's bulk-entry page. The trigger opens a dialog that fetches the shopping list
// (`GET /api/wishlist/{game}/buy-list`, the listing's own query, so the rows are the grid's)
// only once open, then offers one button per store from `lib/bulkBuy.ts`: a **link** store
// (TCGplayer's Mass Entry) is a plain outbound anchor with the cart prefilled, and a
// **paste** store (MTG Mate) copies the list to the clipboard and opens its decklist page —
// two clicks the user can see, never a popup opened behind an async fetch that a browser
// would block. The signed-in currency orders the stores (AUD/NZD → Australia first).
const props = defineProps<{
  game: string
  /** The listing's filter params (a `q`, a set scope, the copy-count filter) when the
   * button sits on a browse grid, so "Buy all" means the rows on screen. Absent on the
   * landing, where it means the whole list — cards and sealed products alike. */
  params?: BuyListParams
  /** Nothing to buy yet (an empty list, or the grid still loading). */
  disabled?: boolean
}>()

const open = ref(false)
const money = useCurrency()

const query = computed(() => props.params ?? {})
// Fetched only while the dialog is open, and afresh on every opening (a wanted count
// edited since is a different cart) — hence `staleTime: 0` and no cache reuse. Keyed on
// the params so a filter change is a different list.
const game = computed(() => props.game)
const options = {
  queryKey: ['wishlist-buy-list', game, query],
  queryFn: (token: string) => getWishlistBuyList(token, game.value, query.value),
  enabled: open,
  staleTime: 0,
  gcTime: 0,
}
const buyList = useAuthedQuery<BuyList>(options)

const list = computed(() => buyList.data.value)
const storeOptions = computed<BulkBuyOption[]>(() =>
  list.value ? bulkBuyOptionsFor(props.game, list.value, money.currency.value) : [],
)
const cardCopies = computed(() =>
  (list.value?.cards ?? []).reduce((sum, row) => sum + row.quantity + row.foil_quantity, 0),
)
const empty = computed(
  () => !!list.value && list.value.cards.length === 0 && list.value.products.length === 0,
)

// What the list holds, in one line: "12 cards (18 copies) and 2 sealed products".
const summary = computed(() => {
  const l = list.value
  if (!l) return ''
  const cards = `${l.cards.length.toLocaleString()} ${l.cards.length === 1 ? 'card' : 'cards'} (${cardCopies.value.toLocaleString()} ${cardCopies.value === 1 ? 'copy' : 'copies'})`
  if (l.products.length === 0) return cards
  const products = `${l.products.length.toLocaleString()} sealed ${l.products.length === 1 ? 'product' : 'products'}`
  return `${cards} and ${products}`
})
const truncatedNote = computed(() => {
  const l = list.value
  if (!l?.truncated) return null
  const cut = l.total_cards - l.cards.length + (l.total_products - l.products.length)
  return `Only the first ${l.cards.length.toLocaleString()} card rows are sent — ${cut.toLocaleString()} more are on the list. Narrow the list with a search or a set to buy the rest.`
})

// Paste stores: copy, then open. Clipboard access can be denied (insecure context /
// permissions); the list stays in the dialog for manual copying either way.
const copied = ref<string | null>(null)
const copyFailed = ref<string | null>(null)
async function pasteAndOpen(option: Extract<BulkBuyOption, { kind: 'paste' }>) {
  copyFailed.value = null
  try {
    await navigator.clipboard.writeText(option.list)
    copied.value = option.name
    setTimeout(() => {
      if (copied.value === option.name) copied.value = null
    }, 2500)
  } catch {
    copyFailed.value = option.name
  }
  window.open(option.href, '_blank', 'noopener,noreferrer')
}

// The paste list, selectable by hand when the clipboard is denied.
const shownList = ref<string | null>(null)
watch(open, (isOpen) => {
  if (!isOpen) {
    shownList.value = null
    copied.value = null
    copyFailed.value = null
  }
})
</script>

<template>
  <Dialog v-model:open="open">
    <DialogTrigger as-child>
      <Button variant="outline" size="sm" :disabled="disabled">
        <ShoppingCart />
        Buy all
      </Button>
    </DialogTrigger>
    <DialogContent
      class="bg-background flex max-h-[85svh] w-[min(96vw,32rem)] flex-col gap-4 overflow-y-auto rounded-xl border p-5 shadow-lg"
    >
      <div class="flex items-start justify-between gap-2">
        <DialogTitle>Buy these cards</DialogTitle>
        <DialogClose
          class="text-muted-foreground hover:text-foreground -mt-1 -mr-1 rounded-md p-1"
          aria-label="Close"
        >
          <X class="size-4" aria-hidden="true" />
        </DialogClose>
      </div>
      <DialogDescription class="text-muted-foreground text-sm">
        Send your list to a store's bulk-entry page. Prices and stock are the store's own — nothing
        is bought until you check out there.
      </DialogDescription>

      <p
        v-if="buyList.isPending.value"
        class="text-muted-foreground flex items-center gap-2 text-sm"
      >
        <LoaderCircle class="size-4 animate-spin" aria-hidden="true" />
        Gathering your list…
      </p>
      <p v-else-if="buyList.isError.value" class="text-destructive text-sm" aria-live="polite">
        {{ buyList.error.value?.message ?? 'Could not load your list. Please try again.' }}
      </p>
      <p v-else-if="empty" class="text-muted-foreground text-sm">
        Nothing to buy — no wanted cards match.
      </p>
      <template v-else-if="list">
        <p class="text-sm">{{ summary }}</p>
        <p v-if="truncatedNote" class="text-warning flex gap-2 text-sm" aria-live="polite">
          <TriangleAlert class="mt-0.5 size-4 shrink-0" aria-hidden="true" />
          <span>{{ truncatedNote }}</span>
        </p>

        <ul class="divide-border divide-y">
          <li
            v-for="option in storeOptions"
            :key="option.name"
            class="flex flex-wrap items-center justify-between gap-3 py-3"
          >
            <div class="min-w-0 flex-1">
              <p class="font-medium">
                {{ option.name }}
                <span class="text-muted-foreground text-xs font-normal">· {{ option.region }}</span>
              </p>
              <p class="text-muted-foreground text-xs">{{ option.note }}</p>
              <p v-if="copyFailed === option.name" class="text-destructive mt-1 text-xs">
                Couldn't copy the list — select it below and copy it yourself.
                <button type="button" class="underline" @click="shownList = option.name">
                  Show list
                </button>
              </p>
              <textarea
                v-if="option.kind === 'paste' && shownList === option.name"
                class="bg-muted mt-2 h-32 w-full rounded-md p-2 font-mono text-xs"
                readonly
                :value="option.list"
                aria-label="Card list to paste"
              />
            </div>
            <Button
              v-if="option.kind === 'link'"
              as="a"
              :href="option.href"
              target="_blank"
              rel="noopener noreferrer"
              size="sm"
            >
              Open {{ option.name }}
              <ExternalLink class="size-3.5" aria-hidden="true" />
            </Button>
            <Button v-else type="button" size="sm" @click="pasteAndOpen(option)">
              <Check v-if="copied === option.name" class="size-3.5" aria-hidden="true" />
              <Copy v-else class="size-3.5" aria-hidden="true" />
              {{ copied === option.name ? 'Copied — opening' : `Copy list & open ${option.name}` }}
            </Button>
          </li>
        </ul>
      </template>
    </DialogContent>
  </Dialog>
</template>
