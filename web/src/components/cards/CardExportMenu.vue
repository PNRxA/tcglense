<script setup lang="ts">
import { computed, ref } from 'vue'
import { FileDown, TriangleAlert } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import {
  ApiError,
  type CardExportFormat,
  LARGE_EXPORT_CARDS,
  exportCards,
  exportCollectionCards,
  exportSetCards,
  exportWishlistCards,
} from '@/lib/api'
import { toSortParam } from '@/lib/cardSort'
import { downloadBlob } from '@/lib/download'
import { useAuthStore } from '@/stores/auth'

// Download the *whole* result set of the card search currently on screen as a .txt file
// (the grid only ever shows one 60-card page of it). Sits beside the size/sort menus in
// the catalog and holdings browse views; the endpoint streams the result set, however
// large.
//
// The search itself is not re-derived here — the same `q`/`sort`/`include_related` the
// grid queried with are passed straight through, and the endpoint reuses the listing's
// own query builder, so the file can't disagree with what the visitor is looking at.
// On a holdings surface (`list` set) the export is the signed-in user's own listing —
// authed, `set`-scoped by query param, and rendered with the real held counts.
const props = defineProps<{
  game: string
  /** Set code when exporting a set's cards; absent = the all-cards search. On a
   * holdings surface this rides as the `?set=` scope instead of a path segment. */
  setCode?: string
  /** Holdings surfaces: export the signed-in user's own held-card listing (real counts,
   * foil lines tagged `*F*`) instead of the public catalog search. The browse views
   * omit it in show-ghosts mode — the grid there *is* the catalog listing. */
  list?: 'collection' | 'wishlist'
  /** The committed search (`?q`), if any. */
  query?: string
  /** The active `field:dir` sort value, and the view's default to compare it against. */
  sort: string
  defaultSort: string
  /** Set views only: whether the listing spans the set's related group. */
  includeRelated?: boolean
  /** How many cards the search matched, so the menu can flag a large download. */
  total?: number
  /** Nothing to export (no results yet, or the query is in flight). */
  disabled?: boolean
}>()

const exporting = ref(false)
const errorMessage = ref<string | null>(null)

// Exports are uncapped, so there is no limit to disclose — but a big one is a big file
// and a slow download, and a visitor should know that before the browser stalls rather
// than after. Warn only once the result set is genuinely large; below that the menu items
// already say everything worth saying.
const isLarge = computed(() => (props.total ?? 0) >= LARGE_EXPORT_CARDS)
const sizeNote = computed(
  () => `Exporting all ${(props.total ?? 0).toLocaleString()} matches — this may take a moment.`,
)

// A holdings line carries the real held counts (and a foil marker); a catalog line is
// always a single copy. Say which one this menu will produce.
const textNote = computed(() =>
  props.list
    ? 'One line per printing and finish with your counts, e.g. “4 Sol Ring (LTC) 284”.'
    : 'One line per printing, e.g. “1 Sol Ring (LTC) 284”.',
)

const params = computed(() => ({
  q: props.query || undefined,
  ...toSortParam(props.sort, props.defaultSort),
  includeRelated: props.includeRelated,
}))

// Mirrors the server's own filename, so a visitor who opens the API directly and one who
// clicks this button end up with identically-named files. Holdings filenames are
// scope-free (no set code): the server never puts the visitor-typed `?set=` in one.
function filename(format: CardExportFormat): string {
  const slug = format === 'names' ? 'card-names' : 'cards'
  if (props.list) return `tcglense-${props.game}-${props.list}-${slug}.txt`
  const scope = props.setCode ? `${props.game}-${props.setCode}` : props.game
  return `tcglense-${scope}-${slug}.txt`
}

async function download(format: CardExportFormat) {
  if (exporting.value) return
  exporting.value = true
  errorMessage.value = null
  try {
    const request = { ...params.value, format }
    let blob: Blob
    if (props.list) {
      // Per-user download: through the auth store's authFetch (one 401-refresh-and-retry),
      // with the set scope riding as a query param on the holdings listing.
      const exportHolding = props.list === 'wishlist' ? exportWishlistCards : exportCollectionCards
      const auth = useAuthStore()
      blob = await auth.authFetch((token) =>
        exportHolding(token, props.game, { ...request, set: props.setCode }),
      )
    } else {
      blob = props.setCode
        ? await exportSetCards(props.game, props.setCode, request)
        : await exportCards(props.game, request)
    }
    downloadBlob(blob, filename(format))
  } catch (err) {
    errorMessage.value = err instanceof ApiError ? err.message : 'Export failed. Please try again.'
  } finally {
    exporting.value = false
  }
}
</script>

<template>
  <div class="relative">
    <DropdownMenu>
      <DropdownMenuTrigger as-child>
        <Button variant="outline" size="sm" :disabled="disabled || exporting">
          <FileDown />
          Export
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" class="w-64">
        <DropdownMenuLabel>Export these results</DropdownMenuLabel>
        <DropdownMenuSeparator />
        <DropdownMenuItem @select="download('text')">
          <span class="flex flex-col">
            <span class="font-medium">Card list (.txt)</span>
            <span class="text-muted-foreground text-xs">
              {{ textNote }}
            </span>
          </span>
        </DropdownMenuItem>
        <DropdownMenuItem @select="download('names')">
          <span class="flex flex-col">
            <span class="font-medium">Card names (.txt)</span>
            <span class="text-muted-foreground text-xs">
              Names only, one per card — printings folded together.
            </span>
          </span>
        </DropdownMenuItem>
        <!-- Heads-up for a large export: it's the whole result set, so say how big
             that is. Absent for ordinary searches — nothing is capped or withheld. -->
        <template v-if="isLarge">
          <DropdownMenuSeparator />
          <p class="text-muted-foreground px-2 py-1.5 text-xs">
            <TriangleAlert class="mr-1 inline size-3 align-[-2px]" />
            {{ sizeNote }}
          </p>
        </template>
      </DropdownMenuContent>
    </DropdownMenu>
    <p v-if="errorMessage" class="text-destructive mt-2 text-sm" aria-live="polite">
      {{ errorMessage }}
    </p>
  </div>
</template>
