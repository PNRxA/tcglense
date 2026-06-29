<script setup lang="ts">
import { computed } from 'vue'
import { ChevronLeft, ChevronRight } from '@lucide/vue'
import { Button } from '@/components/ui/button'

const page = defineModel<number>('page', { required: true })
const props = defineProps<{
  pageSize: number
  total: number
}>()

const totalPages = computed(() => Math.max(1, Math.ceil(props.total / props.pageSize)))

function go(target: number) {
  page.value = Math.min(totalPages.value, Math.max(1, target))
}
</script>

<template>
  <div v-if="totalPages > 1" class="flex items-center justify-center gap-4">
    <Button variant="outline" size="sm" :disabled="page <= 1" @click="go(page - 1)">
      <ChevronLeft />
      Prev
    </Button>
    <span class="text-muted-foreground text-sm tabular-nums">
      Page {{ page }} of {{ totalPages }}
    </span>
    <Button variant="outline" size="sm" :disabled="page >= totalPages" @click="go(page + 1)">
      Next
      <ChevronRight />
    </Button>
  </div>
</template>
