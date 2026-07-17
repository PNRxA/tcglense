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
  }>(),
  {
    errorMessage: 'Could not load printings. Please try again.',
    emptyMessage: 'No printings found.',
    sortOptions: () => PRINTING_SORT_OPTIONS,
  },
)

const emit = defineEmits<{
  'update:filter': [string]
  loadMore: []
}>()

// Sort is a purely presentational reordering of the already-loaded printings, so it lives
// here (grid-local) rather than being threaded through every caller like the filter: it
// changes neither the loaded-page count nor which pages are fetched.
const sort = ref(PRINTING_DEFAULT_SORT)
const sortedPrintings = computed(() => sortPrintings(props.filteredPrintings, sort.value))

const filterActive = computed(() => props.filter.trim().length > 0)
const countLabel = computed(() => {
  if (filterActive.value) {
    return `${props.filteredPrintings.length} matching · ${props.printings.length} loaded of ${props.total}`
  }
  return `${props.printings.length} of ${props.total} printings loaded`
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
      <div v-if="total > 1" class="flex flex-wrap items-center gap-2">
        <CardSearchBox
          :model-value="filter"
          class="w-full sm:w-72"
          placeholder="Filter by set, number, or rarity…"
          aria-label="Filter loaded printings by set, number, or rarity"
          @update:model-value="emit('update:filter', $event)"
        />
        <CardSortMenu v-model="sort" :options="sortOptions" />
      </div>
      <div class="ml-auto text-right">
        <p class="text-muted-foreground text-xs">{{ countLabel }}</p>
        <p v-if="hasMore" class="text-muted-foreground mt-0.5 text-xs">
          Filter searches loaded printings only.
        </p>
      </div>
    </div>

    <p v-if="filteredPrintings.length === 0" class="text-muted-foreground py-8 text-center text-sm">
      No loaded printings match “{{ filter.trim() }}”.
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
