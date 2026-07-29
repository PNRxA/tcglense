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
  exportSetCards,
} from '@/lib/api'
import { toSortParam } from '@/lib/cardSort'
import { downloadBlob } from '@/lib/download'

// Download the *whole* result set of the card search currently on screen as a .txt file
// (the grid only ever shows one 60-card page of it). Sits beside the size/sort menus in
// the catalog browse views; the endpoint streams the result set, however large.
//
// The search itself is not re-derived here — the same `q`/`sort`/`include_related` the
// grid queried with are passed straight through, and the endpoint reuses the listing's
// own query builder, so the file can't disagree with what the visitor is looking at.
const props = defineProps<{
  game: string
  /** Set code when exporting a set's cards; absent = the all-cards search. */
  setCode?: string
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

const params = computed(() => ({
  q: props.query || undefined,
  ...toSortParam(props.sort, props.defaultSort),
  includeRelated: props.includeRelated,
}))

// Mirrors the server's own filename, so a visitor who opens the API directly and one who
// clicks this button end up with identically-named files.
function filename(format: CardExportFormat): string {
  const scope = props.setCode ? `${props.game}-${props.setCode}` : props.game
  return `tcglense-${scope}-${format === 'names' ? 'card-names' : 'cards'}.txt`
}

async function download(format: CardExportFormat) {
  if (exporting.value) return
  exporting.value = true
  errorMessage.value = null
  try {
    const request = { ...params.value, format }
    const blob = props.setCode
      ? await exportSetCards(props.game, props.setCode, request)
      : await exportCards(props.game, request)
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
              One line per printing, e.g. “1 Sol Ring (LTC) 284”.
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
