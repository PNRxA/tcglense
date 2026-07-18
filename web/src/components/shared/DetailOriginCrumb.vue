<script setup lang="ts">
import { computed } from 'vue'
import { useQueryClient } from '@tanstack/vue-query'
import { ArrowLeft } from '@lucide/vue'
import type { Card, Product } from '@/lib/api'
import { findCardInCache, findProductInCache } from '@/lib/placeholders'
import type { DetailOriginKind } from '@/lib/detailOrigin'

// The "← Back to <origin>" crumb the detail modal shows when you crossed from a sealed product
// into one of its cards, or the reverse (see DetailDialogShell). It names the surface you came
// from so the return trip reads as one obvious tap instead of a hunt for the browser's Back
// button. The shell owns the URL rewrite; this is presentation + naming only, emitting `navigate`
// on click.
//
// The name is read straight from the query cache — you were just looking at that item, so its
// row is warm — with NO network fetch (data/images stay off the lazy, Scryfall-friendly path).
// A cold deep link (`?card=…&from=product:…` shared and opened fresh) that can't resolve a name
// still labels the button with the generic noun, which is unambiguous: there is exactly one
// place to go back to.
const props = defineProps<{ game: string; kind: DetailOriginKind; id: string }>()
defineEmits<{ navigate: [] }>()

const NOUNS: Record<DetailOriginKind, string> = { card: 'card', product: 'sealed product' }

// useQueryClient throws when no client is provided. The modal always mounts under one in the app,
// but guarding keeps the crumb renderable in isolation (and testable without the query layer) —
// it just falls back to the generic label.
let qc: ReturnType<typeof useQueryClient> | null = null
try {
  qc = useQueryClient()
} catch {
  qc = null
}

const name = computed(() => {
  if (!qc) return null
  if (props.kind === 'product') {
    const found =
      qc.getQueryData<Product>(['product', props.game, props.id]) ??
      findProductInCache(qc, props.game, props.id)
    return found?.name ?? null
  }
  const found =
    qc.getQueryData<Card>(['card', props.game, props.id]) ??
    findCardInCache(qc, props.game, props.id)
  return found?.name ?? null
})
</script>

<template>
  <button
    type="button"
    class="text-muted-foreground hover:text-foreground hover:bg-accent -ml-1.5 inline-flex max-w-full items-center gap-1.5 rounded-md px-2 py-1 text-xs font-medium"
    @click="$emit('navigate')"
  >
    <ArrowLeft class="size-3.5 shrink-0" aria-hidden="true" />
    <span class="truncate">Back to {{ name ?? NOUNS[kind] }}</span>
  </button>
</template>
