<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
import { RouterLink } from 'vue-router'
import { ChevronDown, ChevronUp } from '@lucide/vue'
import { useCardArtTags } from '@/composables/useArtTags'
import { addArtTag } from '@/lib/searchBuilder'

// A card's "Artwork tags" panel: the community Tagger labels describing what this
// card's painting depicts (issue #140's data, on the card page). Tags key on the
// artwork, not the printing, so every printing of the same painting shows the same
// list; clicking one runs the `art:` search it corresponds to.
//
// The list is hierarchy-expanded server-side (an artwork tagged `squirrel` also
// carries `rodent`, `animal`, …) and ordered rarest-first, so the specific, descriptive
// tags come first and the broad ancestors trail — which is why showing only the first
// few and hiding the rest behind "Show more" loses nothing interesting. Renders nothing
// when the artwork is untagged.
const props = defineProps<{ game: string; id: string }>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')

/** How many tags show before the "Show more" cut. Enough to cover a typical artwork's
 * descriptive tags; the tail is the broad ancestry (`creature`, `object`, …). */
const VISIBLE_TAGS = 10

const query = useCardArtTags(game, id)
const tags = computed(() => query.data.value?.data ?? [])

// Collapsed by default, and re-collapsed when the id changes — the component is reused
// across card-to-card navigation (and inside the long-lived detail modal), so an
// expansion must not leak from one card to the next, same as CardLegalities.
const showAll = ref(false)
watch(id, () => {
  showAll.value = false
})

const visible = computed(() => (showAll.value ? tags.value : tags.value.slice(0, VISIBLE_TAGS)))
const hiddenCount = computed(() => Math.max(0, tags.value.length - VISIBLE_TAGS))

/** The card-browse search for one tag — the same `art:` token the advanced-search
 * panel writes, built through the shared query builder so quoting stays in one place. */
const searchFor = (slug: string) => ({
  path: `/cards/${game.value}/cards`,
  query: { q: addArtTag('', slug) },
})
</script>

<template>
  <div v-if="tags.length" class="bg-card rounded-xl border p-4 shadow-sm">
    <h2 class="text-sm font-semibold">Artwork tags</h2>
    <p class="text-muted-foreground mt-0.5 mb-3 text-xs">
      Community Tagger labels for what this artwork depicts — pick one to search every card sharing
      it.
    </p>
    <div class="flex flex-wrap gap-1.5">
      <RouterLink
        v-for="tag in visible"
        :key="tag.slug"
        :to="searchFor(tag.slug)"
        class="hover:bg-accent hover:text-accent-foreground inline-flex h-7 items-center gap-1.5 rounded-md border px-2 text-xs transition-colors"
        :title="tag.description ?? undefined"
      >
        <span>{{ tag.label }}</span>
        <!-- The count is *artworks*, while the link runs a card search — which returns
          printings, so the two numbers legitimately differ (one painting, many reprints).
          Spelled out here rather than left as a bare number the reader would read as a
          result count. -->
        <span
          class="text-muted-foreground tabular-nums"
          :title="`${tag.count.toLocaleString()} artworks tagged ${tag.label}`"
        >
          {{ tag.count.toLocaleString() }}
        </span>
      </RouterLink>
    </div>
    <button
      v-if="hiddenCount"
      type="button"
      class="text-muted-foreground hover:text-foreground mt-3 inline-flex items-center gap-1 text-xs font-medium"
      :aria-expanded="showAll"
      @click="showAll = !showAll"
    >
      <component :is="showAll ? ChevronUp : ChevronDown" class="size-3.5" aria-hidden="true" />
      {{ showAll ? 'Show fewer tags' : `Show all tags (${hiddenCount} more)` }}
    </button>
  </div>
</template>
