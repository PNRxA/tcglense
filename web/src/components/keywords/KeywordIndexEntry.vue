<script setup lang="ts">
import { RouterLink, useRouter } from 'vue-router'
import ManaSymbols from '@/components/cards/ManaSymbols.vue'
import KeywordKindChip from '@/components/keywords/KeywordKindChip.vue'
import { firstSentence } from '@/lib/keywords'
import { prefetchRouteChunks } from '@/lib/prefetch'
import type { KeywordEntry } from '@/lib/api'

// One tile in the glossary index: the keyword and enough of its explanation to answer
// the question without a click. The kind chip only shows for actions and ability words —
// abilities are the bulk of the list, so chipping them would just add noise to every row.
const props = defineProps<{ entry: KeywordEntry }>()

const router = useRouter()
const to = () => `/keywords/${props.entry.slug}`

// Warm the target route's chunk on hover/focus, as the set tiles do.
function warm() {
  prefetchRouteChunks(router, to())
}
</script>

<template>
  <RouterLink
    :to="to()"
    class="bg-card hover:border-ring/60 hover:bg-accent/40 group flex h-full flex-col gap-1 rounded-xl border p-3 transition-colors"
    @pointerenter="warm"
    @focusin="warm"
  >
    <div class="flex items-baseline justify-between gap-2">
      <span class="font-medium">{{ entry.name }}</span>
      <KeywordKindChip v-if="entry.kind !== 'ability'" :kind="entry.kind" />
    </div>
    <span class="text-muted-foreground line-clamp-2 text-xs leading-relaxed"
      ><ManaSymbols :text="firstSentence(entry.text)"
    /></span>
  </RouterLink>
</template>
