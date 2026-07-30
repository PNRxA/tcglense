<script setup lang="ts">
import { watch } from 'vue'
import { useRoute } from 'vue-router'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import StickySearchBar from '@/components/cards/StickySearchBar.vue'
import KeywordIndexEntry from '@/components/keywords/KeywordIndexEntry.vue'
import KeywordKindTabs from '@/components/keywords/KeywordKindTabs.vue'
import KeywordLetterNav from '@/components/keywords/KeywordLetterNav.vue'
import { useKeywordIndex } from '@/composables/useKeywords'
import { usePageMeta } from '@/lib/seo'
import { definedTermSetNode, graph } from '@/lib/structuredData'

// The A–Z glossary index: every Magic keyword ability, keyword action and ability word,
// filterable by text and by kind. The per-keyword pages under /keywords/:slug are the
// SERP landing targets; this is the hub that links them all and the page someone browses
// when they don't yet know what they're looking for.
const { query, filter, kind, filtering, kindCounts, sections, letters, total } = useKeywordIndex()

const route = useRoute()

usePageMeta({
  title: 'Magic: The Gathering keyword glossary',
  description:
    'Every Magic: The Gathering keyword ability, keyword action and ability word explained — ' +
    'look up what each one does and see the cards that use it.',
  // Fixed: the filter and kind tabs are local state and never touch the URL, so this
  // page has no query-string variants to collapse.
  canonicalPath: '/keywords',
  jsonLd: () => graph(definedTermSetNode()),
})

// A deep link like /keywords#d should still land on that letter. The jump strip itself
// uses buttons (see KeywordLetterNav), so this only runs for an incoming hash, once the
// sections exist to scroll to.
watch(
  () => query.isSuccess.value,
  (loaded) => {
    if (!loaded || !route.hash) return
    requestAnimationFrame(() => {
      document.getElementById(route.hash.slice(1))?.scrollIntoView({ block: 'start' })
    })
  },
  { immediate: true },
)
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-12">
    <header class="mb-6">
      <h1 class="text-3xl font-semibold tracking-tight">Magic: The Gathering keywords</h1>
      <!-- The count only appears once the glossary lands — a placeholder "0 keyword
        abilities" is worse than no number at all. The `, explained.` is hugged onto the
        branches so it doesn't render with a space before the comma. -->
      <p v-if="total" class="text-muted-foreground mt-2">
        {{ total }} keyword abilities, actions and ability words, explained.
      </p>
      <p v-else class="text-muted-foreground mt-2">
        Every keyword ability, action and ability word, explained.
      </p>
    </header>

    <StickySearchBar class="mb-6 flex flex-wrap items-center gap-3">
      <CardSearchBox
        v-model="filter"
        placeholder="Filter keywords…"
        aria-label="Filter keywords by name or rules text"
        class="w-full sm:w-64"
      />
      <KeywordKindTabs v-model="kind" :counts="kindCounts" />
    </StickySearchBar>

    <KeywordLetterNav v-if="!filtering" :letters="letters" class="mb-6" />

    <LoadingRow v-if="query.isPending.value" label="Loading keywords…" />
    <p v-else-if="query.isError.value" class="text-destructive py-12">
      Couldn't load the keyword glossary. Please retry.
    </p>
    <p v-else-if="!sections.length" class="text-muted-foreground py-12">
      No keywords match “{{ filter }}”.
    </p>

    <div v-else class="space-y-10">
      <!-- `content-visibility` keeps the ~350-tile single page cheap to render; the
        scroll margin stops a jump landing under the sticky filter bar. -->
      <section
        v-for="section in sections"
        :id="section.id"
        :key="section.letter"
        class="scroll-mt-40 [content-visibility:auto] sm:scroll-mt-28"
      >
        <!-- The filter bar wraps to two rows below `sm` (the search box goes full-width),
          so the offset a letter heading has to clear is taller there. -->
        <div
          class="bg-background/85 sticky top-[6.75rem] z-10 -mx-4 mb-3 flex items-baseline gap-2 border-b px-4 py-2 backdrop-blur sm:top-15"
        >
          <h2 class="text-lg font-semibold">{{ section.letter }}</h2>
          <span class="text-muted-foreground text-xs tabular-nums">{{ section.group.length }}</span>
        </div>
        <ul class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
          <li v-for="entry in section.group" :key="entry.slug">
            <KeywordIndexEntry :entry="entry" />
          </li>
        </ul>
      </section>
    </div>
  </div>
</template>
