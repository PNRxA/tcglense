<script setup lang="ts">
import { computed, ref } from 'vue'
import { Loader2 } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import CardSortMenu from '@/components/cards/CardSortMenu.vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import type { Card } from '@/lib/api'
import type { SortOption } from '@/lib/cardSort'
import { PRINTING_DEFAULT_SORT, PRINTING_SORT_OPTIONS, sortPrintings } from '@/lib/printingSort'

// Shared result-state shell for every visual printing picker. It renders the loading,
// error, empty, filter, sort, accumulated-page count, load-more control, and responsive
// grid; callers provide only the tile/action adapter through the named slot.
const props = withDefaults(
  defineProps<{
    printings: Card[]
    filteredPrintings: Card[]
    filter: string
    total: number
    pending: boolean
    error: boolean
    hasMore: boolean
    loadingMore: boolean
    errorMessage?: string
    emptyMessage?: string
    sortOptions?: SortOption[]
    /** Show the "In my collection" checkbox that narrows the loaded printings to owned ones. */
    collectionFilter?: boolean
    /** Whether that checkbox is currently on (v-model). */
    collectionOnly?: boolean
    /** Owned counts are still loading after the checkbox flipped on — show a checking state. */
    collectionLoading?: boolean
  }>(),
  {
    errorMessage: 'Could not load printings. Please try again.',
    emptyMessage: 'No printings found.',
    sortOptions: () => PRINTING_SORT_OPTIONS,
    collectionFilter: false,
    collectionOnly: false,
    collectionLoading: false,
  },
)

const emit = defineEmits<{
  'update:filter': [string]
  'update:collectionOnly': [boolean]
  loadMore: []
}>()

function onCollectionToggle(event: Event) {
  emit('update:collectionOnly', (event.target as HTMLInputElement).checked)
}

// Sort is a purely presentational reordering of the already-loaded printings, so it lives
// here (grid-local) rather than being threaded through every caller like the filter: it
// changes neither the loaded-page count nor which pages are fetched.
const sort = ref(PRINTING_DEFAULT_SORT)
const sortedPrintings = computed(() => sortPrintings(props.filteredPrintings, sort.value))

const filterActive = computed(() => props.filter.trim().length > 0 || props.collectionOnly)
const countLabel = computed(() => {
  if (filterActive.value) {
    return `${props.filteredPrintings.length} matching · ${props.printings.length} loaded of ${props.total}`
  }
  return `${props.printings.length} of ${props.total} printings loaded`
})

// The zero-match copy adapts to which filters are active: the collection toggle, the text
// filter, or both. All three scope to the loaded pages, matching the "loaded only" hint.
const emptyFilterMessage = computed(() => {
  const trimmed = props.filter.trim()
  if (props.collectionOnly) {
    return trimmed
      ? `No loaded printings in your collection match “${trimmed}”.`
      : 'None of the loaded printings are in your collection.'
  }
  return `No loaded printings match “${trimmed}”.`
})
</script>

<template>
  <LoadingRow v-if="pending" label="Loading printings…" />
  <p
    v-else-if="error && printings.length === 0"
    class="text-destructive py-8 text-center text-sm"
    role="alert"
  >
    {{ errorMessage }}
  </p>
  <p v-else-if="printings.length === 0" class="text-muted-foreground py-8 text-center text-sm">
    {{ emptyMessage }}
  </p>
  <div v-else>
    <div class="mb-4 flex flex-wrap items-start justify-between gap-2">
      <!-- Filter/sort are pointless for a lone printing, but the collection toggle must stay
        reachable whenever it's offered — it persists across card picks, so a single-printing
        card mustn't strand it "on" with no way back off. -->
      <div v-if="total > 1 || collectionFilter" class="flex flex-wrap items-center gap-2">
        <template v-if="total > 1">
          <CardSearchBox
            :model-value="filter"
            class="w-full sm:w-72"
            placeholder="Filter by set, number, or rarity…"
            aria-label="Filter loaded printings by set, number, or rarity"
            @update:model-value="emit('update:filter', $event)"
          />
          <CardSortMenu v-model="sort" :options="sortOptions" />
        </template>
        <label
          v-if="collectionFilter"
          class="text-muted-foreground flex cursor-pointer items-center gap-1.5 text-sm select-none"
        >
          <input
            type="checkbox"
            class="accent-primary size-3.5 rounded border"
            :checked="collectionOnly"
            @change="onCollectionToggle"
          />
          In my collection
        </label>
      </div>
      <div class="ml-auto text-right">
        <p class="text-muted-foreground text-xs">{{ countLabel }}</p>
        <p v-if="hasMore" class="text-muted-foreground mt-0.5 text-xs">
          Filter searches loaded printings only.
        </p>
      </div>
    </div>

    <LoadingRow v-if="collectionLoading" label="Checking your collection…" />
    <p
      v-else-if="filteredPrintings.length === 0"
      class="text-muted-foreground py-8 text-center text-sm"
    >
      {{ emptyFilterMessage }}
    </p>
    <div v-else class="grid grid-cols-[repeat(auto-fill,minmax(min(100%,12rem),1fr))] gap-4">
      <slot
        v-for="printing in sortedPrintings"
        :key="printing.id"
        name="tile"
        :printing="printing"
      />
    </div>

    <p v-if="error" class="text-destructive mt-4 text-center text-sm" role="alert">
      Could not load more printings. Please retry.
    </p>
    <div v-if="hasMore" class="mt-5 flex justify-center">
      <Button variant="outline" :disabled="loadingMore" @click="emit('loadMore')">
        <Loader2 v-if="loadingMore" class="size-4 animate-spin" aria-hidden="true" />
        {{ loadingMore ? 'Loading…' : 'Load more printings' }}
      </Button>
    </div>
  </div>
</template>
