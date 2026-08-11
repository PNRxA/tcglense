<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { RouterLink, useRoute, useRouter, type LocationQueryRaw } from 'vue-router'
import { Layers } from '@lucide/vue'
import { buttonVariants } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import CardPagination from '@/components/cards/CardPagination.vue'
import CardSearchBox from '@/components/cards/CardSearchBox.vue'
import StickySearchBar from '@/components/cards/StickySearchBar.vue'
import UpdatingCue from '@/components/cards/UpdatingCue.vue'
import UpdatingOverlay from '@/components/cards/UpdatingOverlay.vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import PreconTile from '@/components/decks/PreconTile.vue'
import { useCardSearch } from '@/composables/useCardSearch'
import { useGameName } from '@/composables/useCatalog'
import { useClampPage } from '@/composables/useClampPage'
import { PRECON_PAGE_SIZE, usePreconFacetsQuery, usePreconsQuery } from '@/composables/usePrecons'
import { usePageMeta } from '@/lib/seo'

// Preconstructed decks: the lists a publisher shipped, browsable beside your own decks.
// Public catalog data (no auth) — a visitor can browse and read every decklist; signing in
// only adds "Copy to my decks" on the detail page.
const props = defineProps<{ game: string }>()
const game = toRef(props, 'game')
const gameName = useGameName(game)

// The set + type filters live in the URL. reka's Select reserves '' for "no selection", so
// an `all` sentinel means "no filter"; writes merge into the query and reset paging.
const route = useRoute()
const router = useRouter()
const ALL = 'all'
function patchFilter(key: 'set' | 'type', value: string) {
  const next: LocationQueryRaw = { ...route.query }
  if (value === ALL) delete next[key]
  else next[key] = value
  delete next.page
  router.replace({ query: next })
}
function readFilter(key: 'set' | 'type'): string {
  const raw = route.query[key]
  return typeof raw === 'string' && raw ? raw : ''
}
const setFilter = computed(() => readFilter('set'))
const typeFilter = computed(() => readFilter('type'))
const setSelect = computed({
  get: () => setFilter.value || ALL,
  set: (value: string) => patchFilter('set', value),
})
const typeSelect = computed({
  get: () => typeFilter.value || ALL,
  set: (value: string) => patchFilter('type', value),
})

// Page, name search and sort live in the URL, shared with every other browse view. The two
// sorts are the API's own vocabulary — newest first (the default: a precon browser is mostly
// "what came out recently") or by name.
const SORT_OPTIONS = ['released', 'name']
const { page, searchInput, query, sort } = useCardSearch('released', SORT_OPTIONS)

const facetsQuery = usePreconFacetsQuery(game)
const typeOptions = computed(() => facetsQuery.data.value?.data.types ?? [])
const setOptions = computed(() => facetsQuery.data.value?.data.sets ?? [])

const preconsQuery = usePreconsQuery(game, {
  page,
  query,
  set: setFilter,
  type: typeFilter,
  sort,
})
const precons = computed(() => preconsQuery.data.value?.data ?? [])
const total = computed(() => preconsQuery.data.value?.total ?? 0)
const resultsTop = ref<HTMLElement | null>(null)

useClampPage(page, () => ({
  ready: preconsQuery.isSuccess.value,
  total: total.value,
  pageSize: PRECON_PAGE_SIZE,
}))

usePageMeta({
  title: () => `${gameName.value} preconstructed decks`,
  description: () =>
    `Browse every preconstructed ${gameName.value} deck — Commander decks, Planeswalker and ` +
    `Challenger decks, Jumpstart themes and Secret Lair drops — with full decklists, prices ` +
    `and one-click copying into your own decks on TCGLense.`,
  canonicalPath: () => `/decks/${game.value}/precons`,
})
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-10">
    <PageBreadcrumbs
      :items="[
        { label: 'Decks', to: '/decks' },
        { label: gameName, to: `/decks/${game}` },
        { label: 'Preconstructed' },
      ]"
    />

    <header class="mb-4 flex flex-wrap items-start justify-between gap-3">
      <div>
        <h1 class="text-3xl font-semibold tracking-tight">Preconstructed decks</h1>
        <p class="text-muted-foreground mt-1 text-sm">
          The decklists {{ gameName }} shipped — Commander decks, Planeswalker and Challenger decks,
          Jumpstart themes and Secret Lair drops. Open one to see the full list, or copy it into
          your own decks.
        </p>
      </div>
      <RouterLink :class="buttonVariants({ variant: 'outline' })" :to="`/decks/${game}`">
        <Layers class="size-4" aria-hidden="true" /> Your decks
      </RouterLink>
    </header>

    <StickySearchBar>
      <div class="flex flex-wrap items-center gap-2">
        <CardSearchBox
          v-model="searchInput"
          placeholder="Search preconstructed decks…"
          aria-label="Search preconstructed decks"
          class="min-w-48 flex-1"
        />
        <Select v-model="typeSelect">
          <SelectTrigger size="sm" class="w-48" aria-label="Filter by deck type">
            <SelectValue placeholder="All types" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem :value="ALL">All types</SelectItem>
            <SelectItem v-for="t in typeOptions" :key="t.type" :value="t.type">
              {{ t.type }} ({{ t.count }})
            </SelectItem>
          </SelectContent>
        </Select>
        <Select v-model="setSelect">
          <SelectTrigger size="sm" class="w-44" aria-label="Filter by set">
            <SelectValue placeholder="All sets" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem :value="ALL">All sets</SelectItem>
            <SelectItem v-for="s in setOptions" :key="s.code" :value="s.code">
              {{ s.name ?? s.code.toUpperCase() }} ({{ s.count }})
            </SelectItem>
          </SelectContent>
        </Select>
        <Select v-model="sort">
          <SelectTrigger size="sm" class="w-36" aria-label="Sort decks">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="released">Newest first</SelectItem>
            <SelectItem value="name">Name (A–Z)</SelectItem>
          </SelectContent>
        </Select>
      </div>
    </StickySearchBar>

    <p class="text-muted-foreground mt-4 mb-6 text-sm">
      <template v-if="preconsQuery.isFetching.value && !precons.length">Searching…</template>
      <template v-else-if="preconsQuery.isFetching.value && preconsQuery.isPlaceholderData.value">
        <UpdatingCue />
      </template>
      <template v-else>{{ total.toLocaleString() }} {{ total === 1 ? 'deck' : 'decks' }}</template>
      <template v-if="query"> matching “{{ query }}”</template>
    </p>

    <LoadingRow v-if="preconsQuery.isPending.value" label="Loading preconstructed decks…" />
    <p v-else-if="preconsQuery.isError.value" class="text-destructive py-12">
      Couldn't load preconstructed decks. Please retry.
    </p>
    <p v-else-if="!precons.length" class="text-muted-foreground py-12">
      No preconstructed decks found.
    </p>

    <template v-else>
      <div ref="resultsTop" class="scroll-mt-40 sm:scroll-mt-24" />
      <UpdatingOverlay :loading="preconsQuery.isPlaceholderData.value">
        <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
          <PreconTile v-for="precon in precons" :key="precon.slug" :precon="precon" :game="game" />
        </div>
      </UpdatingOverlay>
      <div class="mt-10">
        <CardPagination
          v-model:page="page"
          :page-size="PRECON_PAGE_SIZE"
          :total="total"
          :loading="preconsQuery.isPlaceholderData.value"
          :scroll-target="resultsTop"
        />
      </div>
    </template>
  </div>
</template>
