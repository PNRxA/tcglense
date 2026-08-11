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
import GroupViewToggle from '@/components/cards/GroupViewToggle.vue'
import StickySearchBar from '@/components/cards/StickySearchBar.vue'
import UpdatingCue from '@/components/cards/UpdatingCue.vue'
import UpdatingOverlay from '@/components/cards/UpdatingOverlay.vue'
import LoadingRow from '@/components/cards/LoadingRow.vue'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import PreconTile from '@/components/decks/PreconTile.vue'
import { useCardSearch } from '@/composables/useCardSearch'
import { useGameName } from '@/composables/useCatalog'
import { useClampPage } from '@/composables/useClampPage'
import {
  PRECON_PAGE_SIZE,
  PRECON_SET_PAGE_SIZE,
  usePreconFacetsQuery,
  usePreconSetsQuery,
  usePreconsQuery,
} from '@/composables/usePrecons'
import { usePageMeta } from '@/lib/seo'

// The precon *browse*, serving two routes the way `SealedBrowseView` serves its two:
//
//   /decks/:game/precons/all            — every precon, flat (newest first) or grouped by set
//   /decks/:game/precons/sets/:code     — one set's precons, the landing's click-through
//
// `code` (undefined = the all-decks view) is the only per-route difference: it pins the set
// filter, hides the set `<Select>`, and hides the by-set toggle (a single set grouped by set is
// one group — the flat grid is what you want). Public, like the rest of the catalog.
const props = defineProps<{ game: string; code?: string }>()
const game = toRef(props, 'game')
const gameName = useGameName(game)
const scoped = computed(() => !!props.code)

// The set + type filters live in the URL. reka's Select reserves '' for "no selection", so an
// `all` sentinel means "no filter"; writes merge into the query and reset paging.
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

// The active set: the scoped route's `code`, else the in-URL set filter (any `?set=` is ignored
// in scoped mode, where the set `<Select>` is hidden).
const effectiveSet = computed(() => (scoped.value ? (props.code ?? '') : setFilter.value))

// Page, name search and sort live in the URL, shared with every other browse view. The two
// sorts are the API's own vocabulary — newest first (the default: a precon browser is mostly
// "what came out recently") or by name.
const SORT_OPTIONS = ['released', 'name']
const { page, searchInput, query, sort } = useCardSearch('released', SORT_OPTIONS)

// The by-set view is a URL mode (`?view=sets`), like the card set view's by-drop grouping, so
// it's shareable and survives a reload. Switching restarts paging: a page of *sets* and a page
// of decks don't mean the same thing, so carrying the number across would land you nowhere near
// where you were.
const grouped = computed(() => !scoped.value && route.query.view === 'sets')
function selectView(next: 'grouped' | 'all') {
  const query: LocationQueryRaw = { ...route.query }
  if (next === 'grouped') query.view = 'sets'
  else delete query.view
  delete query.page
  router.replace({ query })
}

const facetsQuery = usePreconFacetsQuery(game)
const typeOptions = computed(() => facetsQuery.data.value?.data.types ?? [])
const setOptions = computed(() => facetsQuery.data.value?.data.sets ?? [])
const scopedSetRef = computed(() => setOptions.value.find((s) => s.code === props.code))
const heading = computed(() =>
  scoped.value ? (scopedSetRef.value?.name ?? props.code?.toUpperCase() ?? '') : 'All decks',
)

const listOptions = {
  page,
  query,
  set: effectiveSet,
  type: typeFilter,
  sort,
}
// Both views are mounted, but only the one on screen fetches: TanStack keeps the other's last
// data cached, so toggling back paints instantly, while `enabled` stops the hidden view from
// firing a second request for every load and every filter change.
const preconsQuery = usePreconsQuery(game, {
  ...listOptions,
  enabled: computed(() => !grouped.value),
})
const setsQuery = usePreconSetsQuery(game, { ...listOptions, enabled: grouped })
const activeQuery = computed(() => (grouped.value ? setsQuery : preconsQuery))

const precons = computed(() => preconsQuery.data.value?.data ?? [])
const setGroups = computed(() => setsQuery.data.value?.data ?? [])
const total = computed(() =>
  grouped.value ? (setsQuery.data.value?.total ?? 0) : (preconsQuery.data.value?.total ?? 0),
)
// The count line names what a page holds: sets in the grouped view, decks in the flat one.
const totalLabel = computed(() => {
  if (!grouped.value)
    return `${total.value.toLocaleString()} ${total.value === 1 ? 'deck' : 'decks'}`
  const decks = setGroups.value.reduce((sum, group) => sum + group.deck_count, 0)
  const sets = `${total.value.toLocaleString()} ${total.value === 1 ? 'set' : 'sets'}`
  return decks
    ? `${sets} · ${decks.toLocaleString()} ${decks === 1 ? 'deck' : 'decks'} on this page`
    : sets
})
const resultsTop = ref<HTMLElement | null>(null)

useClampPage(page, () => ({
  ready: activeQuery.value.isSuccess.value,
  total: total.value,
  pageSize: grouped.value ? PRECON_SET_PAGE_SIZE : PRECON_PAGE_SIZE,
}))

usePageMeta({
  title: () =>
    scoped.value
      ? `${heading.value} preconstructed decks — ${gameName.value}`
      : `All ${gameName.value} preconstructed decks`,
  description: () =>
    scoped.value
      ? `Every preconstructed ${gameName.value} deck from ${heading.value} — full decklists, ` +
        `prices, and one-click copying into your own decks on TCGLense.`
      : `Browse and filter every preconstructed ${gameName.value} deck — Commander decks, ` +
        `Planeswalker and Challenger decks, Jumpstart themes and Secret Lair drops — with full ` +
        `decklists and prices on TCGLense.`,
  canonicalPath: () =>
    scoped.value
      ? `/decks/${game.value}/precons/sets/${props.code}`
      : `/decks/${game.value}/precons/all`,
})
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-10">
    <PageBreadcrumbs
      :items="[
        { label: 'Decks', to: '/decks' },
        { label: gameName, to: `/decks/${game}` },
        { label: 'Preconstructed', to: `/decks/${game}/precons` },
        { label: heading },
      ]"
    />

    <header class="mb-4 flex flex-wrap items-start justify-between gap-3">
      <div>
        <h1 class="text-3xl font-semibold tracking-tight">{{ heading }}</h1>
        <p class="text-muted-foreground mt-1 text-sm">
          Open one to see the full list, or copy it into your own decks.
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
        <!-- The set filter is an in-page filter on the all-decks view; the set-scoped view is
             pinned to its set, so it hides the select entirely. -->
        <Select v-if="!scoped" v-model="setSelect">
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

    <div class="mt-4 mb-6 flex flex-wrap items-center justify-between gap-3">
      <p class="text-muted-foreground text-sm">
        <template v-if="activeQuery.isFetching.value && !total">Searching…</template>
        <template v-else-if="activeQuery.isFetching.value && activeQuery.isPlaceholderData.value">
          <UpdatingCue />
        </template>
        <template v-else>{{ totalLabel }}</template>
        <template v-if="query"> matching “{{ query }}”</template>
      </p>
      <!-- By set / all decks, the by-drop view's own control. Hidden in scoped mode, where
           every deck is already in the one set. -->
      <GroupViewToggle
        v-if="!scoped"
        :grouped="grouped"
        label="By set"
        all-label="All decks"
        @select="selectView"
      />
    </div>

    <LoadingRow v-if="activeQuery.isPending.value" label="Loading preconstructed decks…" />
    <p v-else-if="activeQuery.isError.value" class="text-destructive py-12">
      Couldn't load preconstructed decks. Please retry.
    </p>
    <p v-else-if="!total" class="text-muted-foreground py-12">No preconstructed decks found.</p>

    <template v-else>
      <div ref="resultsTop" class="scroll-mt-40 sm:scroll-mt-24" />
      <UpdatingOverlay :loading="activeQuery.isPlaceholderData.value">
        <!-- Grouped by set: one heading per set (linking to that set's own page), then its
             decks in the same tile grid the flat view uses. -->
        <div v-if="grouped" class="space-y-10">
          <section v-for="group in setGroups" :id="group.code" :key="group.code">
            <div class="mb-3 flex flex-wrap items-baseline gap-2 border-b pb-1.5">
              <RouterLink
                :to="`/decks/${game}/precons/sets/${group.code}`"
                class="text-xl font-semibold tracking-tight hover:underline"
              >
                {{ group.name ?? group.code.toUpperCase() }}
              </RouterLink>
              <span class="text-muted-foreground text-sm tabular-nums">
                {{ group.deck_count }} {{ group.deck_count === 1 ? 'deck' : 'decks' }}
                <template v-if="group.released_at"> · {{ group.released_at }}</template>
              </span>
            </div>
            <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
              <PreconTile
                v-for="precon in group.decks"
                :key="precon.slug"
                :precon="precon"
                :game="game"
              />
            </div>
          </section>
        </div>

        <div v-else class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
          <PreconTile v-for="precon in precons" :key="precon.slug" :precon="precon" :game="game" />
        </div>
      </UpdatingOverlay>
      <div class="mt-10">
        <CardPagination
          v-model:page="page"
          :page-size="grouped ? PRECON_SET_PAGE_SIZE : PRECON_PAGE_SIZE"
          :total="total"
          :loading="activeQuery.isPlaceholderData.value"
          :scroll-target="resultsTop"
        />
      </div>
    </template>
  </div>
</template>
