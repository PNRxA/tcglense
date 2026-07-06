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
  // The element marking the top of the section this pager controls. On a page change it's
  // scrolled to the top of the viewport, so the next page starts at the top of its own
  // section rather than wherever the prev/next button happened to sit (issue #258) — the
  // point being that on a page with several independently-paged sections (a sealed product's
  // card sections) each pager jumps to *its* section, not the whole page. The router can't do
  // this: a page change is a same-path `?page=` replace, indistinguishable from opening the
  // `?card=` dialog (which must NOT scroll), so its scrollBehavior deliberately leaves all
  // query-only changes alone (see router/index.ts). Optional — omit to keep the current scroll
  // position. Give the target a `scroll-mt-*` to clear any sticky header sitting over it.
  scrollTarget?: HTMLElement | null
}>()

const totalPages = computed(() => Math.max(1, Math.ceil(props.total / props.pageSize)))

function go(target: number) {
  const next = Math.min(totalPages.value, Math.max(1, target))
  if (next === page.value) return
  page.value = next
  // Jump to the top of this pager's section. Guarded for the SSR/test DOM where a bare
  // element may lack scrollIntoView; a smooth glide unless the user asked for reduced motion.
  props.scrollTarget?.scrollIntoView?.({
    behavior: prefersReducedMotion() ? 'auto' : 'smooth',
    block: 'start',
  })
}

function prefersReducedMotion(): boolean {
  return window.matchMedia?.('(prefers-reduced-motion: reduce)')?.matches === true
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
