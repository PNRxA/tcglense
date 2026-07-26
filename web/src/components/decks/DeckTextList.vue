<script setup lang="ts">
import { computed } from 'vue'
import { useDetailModalLink } from '@/composables/useDetailModalLink'
import type { DeckCardEntry } from '@/lib/api'
import { textLines } from '@/lib/deckText'

// One section as a plain decklist — the "text" deck view (issue #570). No images, no
// prices, no controls: just `4 Sol Ring`, which is how deck lists are read aloud, posted,
// and pasted between sites. At this density a 100-card deck fits on one screen, so it's
// also the fastest way to check "what's actually in here".
//
// The lines stay links (they open the shared detail modal like every other card surface),
// and flow into responsive columns so a long section doesn't become a single tall ribbon.
// Printings collapse by name — see `textLines`.
const props = defineProps<{ game: string; entries: DeckCardEntry[] }>()

// Card ids by name, so a clicked line can still open the right printing's detail. First
// printing wins, matching the order `textLines` folds in.
const idByName = computed(() => {
  const map = new Map<string, string>()
  for (const entry of props.entries) {
    if (!map.has(entry.card.name)) map.set(entry.card.name, entry.card.id)
  }
  return map
})
const lines = computed(() => textLines(props.entries))

const { hrefFor, onActivate, warm } = useDetailModalLink()
function hrefFrom(name: string) {
  const id = idByName.value.get(name)
  return id ? hrefFor('card', props.game, id) : undefined
}
function onClick(event: MouseEvent, name: string) {
  const id = idByName.value.get(name)
  if (id) onActivate(event, 'card', props.game, id)
}
</script>

<template>
  <ul class="columns-1 gap-x-8 text-sm sm:columns-2 lg:columns-3">
    <li v-for="line in lines" :key="line.name" class="flex gap-2 py-0.5">
      <span class="text-muted-foreground w-6 shrink-0 text-right tabular-nums">{{
        line.copies
      }}</span>
      <a
        :href="hrefFrom(line.name)"
        class="truncate hover:underline"
        :title="line.name"
        @click="onClick($event, line.name)"
        @pointerenter="warm('card')"
        @focusin="warm('card')"
        >{{ line.name }}</a
      >
    </li>
  </ul>
</template>
