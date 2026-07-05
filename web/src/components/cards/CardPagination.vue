<script setup lang="ts">
import { computed } from 'vue'
import { ChevronLeft, ChevronRight, Loader2 } from '@lucide/vue'
import { Button } from '@/components/ui/button'

const page = defineModel<number>('page', { required: true })
const props = defineProps<{
  pageSize: number
  total: number
  // When true, the next page is loading: both buttons swap their chevron for a spinner and
  // disable until it resolves (issue #223). Lists that page with `keepPreviousData` pass their
  // query's `isPlaceholderData` here. Kept directionless on purpose — `isPlaceholderData` can't
  // tell a page click from an unrelated refetch (a filter change), and a cache-hit navigation
  // never toggles it, so tracking "which button was clicked" would strand a stale spinner.
  loading?: boolean
}>()

const totalPages = computed(() => Math.max(1, Math.ceil(props.total / props.pageSize)))

function go(target: number) {
  page.value = Math.min(totalPages.value, Math.max(1, target))
}
</script>

<template>
  <div v-if="totalPages > 1" class="flex items-center justify-center gap-4">
    <Button variant="outline" size="sm" :disabled="page <= 1 || loading" @click="go(page - 1)">
      <Loader2 v-if="loading" class="animate-spin" />
      <ChevronLeft v-else />
      Prev
    </Button>
    <span class="text-muted-foreground text-sm tabular-nums">
      Page {{ page }} of {{ totalPages }}
    </span>
    <Button
      variant="outline"
      size="sm"
      :disabled="page >= totalPages || loading"
      @click="go(page + 1)"
    >
      Next
      <Loader2 v-if="loading" class="animate-spin" />
      <ChevronRight v-else />
    </Button>
  </div>
</template>
