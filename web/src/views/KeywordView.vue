<script setup lang="ts">
import { computed, toRef, watchEffect } from 'vue'
import { useRouter } from 'vue-router'
import { useQuery } from '@tanstack/vue-query'
import { ArrowLeft, ArrowRight, SearchX } from '@lucide/vue'
import CardGrid from '@/components/cards/CardGrid.vue'
import CardGridSkeleton from '@/components/cards/CardGridSkeleton.vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import ManaSymbols from '@/components/cards/ManaSymbols.vue'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import KeywordKindChip from '@/components/keywords/KeywordKindChip.vue'
import { Button } from '@/components/ui/button'
import { useKeywordEntry } from '@/composables/useKeywords'
import { listCards } from '@/lib/api'
import { GLOSSARY_GAME, KIND_BLURBS, KIND_LABELS } from '@/lib/keywords'
import { PRICED_CATALOG_STALE_MS } from '@/lib/queryClient'
import { usePageMeta } from '@/lib/seo'
import {
  breadcrumbList,
  definedTermNode,
  graph,
  keywordCrumbs,
  keywordMetaDescription,
} from '@/lib/structuredData'

// One keyword's page — the landing target for a search like "tcglense vigilance". The
// answer sits above the fold, then the page turns the visitor into a catalog visit:
// cards that actually use the keyword, priced, and the search that finds the rest.
const props = defineProps<{ slug: string }>()
const slug = toRef(props, 'slug')

const router = useRouter()
const { query, entry, notFound, previous, next, related } = useKeywordEntry(slug)

// A non-canonical spelling that still resolved (`/keywords/First-Strike`) is replaced
// with the canonical URL, so one page never renders at two addresses.
watchEffect(() => {
  const found = entry.value
  if (found && found.slug !== props.slug) router.replace(`/keywords/${found.slug}`)
})

const crumbs = computed(() => (entry.value ? keywordCrumbs(entry.value.name) : []))

/** A few cards that carry the keyword, newest and priciest first — the `kw:` search
 * filter is exactly what this page wants, and it's the same query the "browse all"
 * button below hands to the catalog. */
const cardsQuery = useQuery({
  queryKey: ['keyword-cards', GLOSSARY_GAME, computed(() => entry.value?.name)],
  queryFn: ({ signal }) =>
    listCards(
      GLOSSARY_GAME,
      { q: `kw:"${entry.value?.name}"`, pageSize: 8, sort: 'price', dir: 'desc' },
      signal,
    ),
  enabled: computed(() => !!entry.value),
  staleTime: PRICED_CATALOG_STALE_MS,
})

const exampleCards = computed(() => cardsQuery.data.value?.data ?? [])
const exampleTotal = computed(() => cardsQuery.data.value?.total ?? 0)
const browseAll = computed(() => ({
  path: `/cards/${GLOSSARY_GAME}/cards`,
  query: { q: `kw:"${entry.value?.name}"` },
}))

usePageMeta({
  title: () =>
    entry.value
      ? `${entry.value.name} — MTG ${KIND_LABELS[entry.value.kind].toLowerCase()}`
      : undefined,
  // Lead with the definition, not the boilerplate: the snippet has to answer "what does
  // vigilance do" in the SERP itself. `assembleMetaDescription` drops a clause whole when
  // it won't fit, so putting the definition second lost it on most entries.
  description: () => (entry.value ? keywordMetaDescription(entry.value) : undefined),
  canonicalPath: () => (entry.value ? `/keywords/${entry.value.slug}` : undefined),
  // The SPA answers 200 for any slug, so an unknown one has to say "don't index me"
  // itself — that plus the dropped canonical is the soft-404 signal. Deliberate; a
  // "simplification" here quietly fills the index with dead keyword URLs.
  noindex: () => notFound.value,
  jsonLd: () =>
    entry.value ? graph(definedTermNode(entry.value), breadcrumbList(crumbs.value)) : undefined,
})
</script>

<template>
  <div class="mx-auto max-w-3xl px-4 py-10">
    <div v-if="notFound" class="py-16 text-center">
      <SearchX class="text-muted-foreground mx-auto size-10" />
      <h1 class="mt-4 text-2xl font-semibold tracking-tight">Keyword not found</h1>
      <p class="text-muted-foreground mt-2">No glossary entry matches “{{ slug }}”.</p>
      <Button variant="outline" class="mt-6" as-child>
        <RouterLink to="/keywords">Browse all keywords</RouterLink>
      </Button>
    </div>

    <LoadingRow v-else-if="query.isPending.value" label="Loading keyword…" />

    <template v-else-if="entry">
      <PageBreadcrumbs :items="crumbs" />

      <header>
        <KeywordKindChip :kind="entry.kind" />
        <h1 class="mt-2 text-3xl font-semibold tracking-tight">{{ entry.name }}</h1>
        <p class="text-muted-foreground mt-1 text-sm">{{ KIND_BLURBS[entry.kind] }}</p>
      </header>

      <!-- The answer, above the fold — this is what the search visitor came for. -->
      <div class="bg-card mt-6 rounded-xl border p-5 shadow-sm">
        <p class="text-lg leading-relaxed whitespace-pre-line">
          <ManaSymbols :text="entry.text" />
        </p>
        <p v-if="entry.parameterized" class="text-muted-foreground mt-3 text-xs">
          Always printed with a value after it — a cost, a number, or a quality.
        </p>
      </div>

      <section v-if="cardsQuery.isPending.value || exampleCards.length" class="mt-10">
        <h2 class="mb-3 text-lg font-semibold">Cards with {{ entry.name }}</h2>
        <CardGridSkeleton v-if="cardsQuery.isPending.value" />
        <template v-else>
          <CardGrid :game="GLOSSARY_GAME" :cards="exampleCards" />
          <Button variant="outline" class="mt-4" as-child>
            <RouterLink :to="browseAll">
              Browse all {{ exampleTotal }} cards with {{ entry.name }}
            </RouterLink>
          </Button>
        </template>
      </section>

      <section v-if="related.length" class="mt-10">
        <h2 class="text-muted-foreground mb-2 text-xs font-semibold tracking-wide uppercase">
          Related keywords
        </h2>
        <div class="flex flex-wrap gap-2">
          <RouterLink
            v-for="item in related"
            :key="item.slug"
            :to="`/keywords/${item.slug}`"
            class="bg-muted hover:bg-accent hover:text-accent-foreground rounded-md px-2.5 py-1 text-sm transition-colors"
          >
            {{ item.name }}
          </RouterLink>
        </div>
      </section>

      <nav
        aria-label="Glossary A to Z"
        class="mt-10 flex items-center justify-between gap-4 border-t pt-6 text-sm"
      >
        <RouterLink
          v-if="previous"
          :to="`/keywords/${previous.slug}`"
          class="text-muted-foreground hover:text-foreground inline-flex items-center gap-1.5 transition-colors"
        >
          <ArrowLeft class="size-4" />
          {{ previous.name }}
        </RouterLink>
        <span v-else />
        <RouterLink to="/keywords" class="hover:underline">All keywords</RouterLink>
        <RouterLink
          v-if="next"
          :to="`/keywords/${next.slug}`"
          class="text-muted-foreground hover:text-foreground inline-flex items-center gap-1.5 transition-colors"
        >
          {{ next.name }}
          <ArrowRight class="size-4" />
        </RouterLink>
        <span v-else />
      </nav>
    </template>
  </div>
</template>
