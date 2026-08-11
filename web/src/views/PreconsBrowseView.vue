<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { RouterLink, useRoute, useRouter, type LocationQueryRaw } from 'vue-router'
import { Layers, LayoutGrid } from '@lucide/vue'
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
import RadioSelectMenu from '@/components/cards/RadioSelectMenu.vue'
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
  PRECON_GROUP_PAGE_SIZE,
  PRECON_PAGE_SIZE,
  usePreconFacetsQuery,
  usePreconGroupsQuery,
  usePreconsQuery,
} from '@/composables/usePrecons'
import type { PreconGrouping } from '@/lib/api'
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

// Span the set's whole catalog group — its root plus every related sub-set — the precon mirror
// of the card set page's own `?related=1`, and where the landing's grouped "All decks" lands.
// Only meaningful on a set-scoped route: the all-decks view already spans every set.
const includeRelated = computed(() => scoped.value && route.query.related === '1')
// One set actually on screen. Not the same as `scoped`: a spanned group is a set-scoped route
// showing several sets, so it behaves like the all-decks view for grouping purposes.
const singleSet = computed(() => scoped.value && !includeRelated.value)

// Page, name search and sort live in the URL, shared with every other browse view. The two
// sorts are the API's own vocabulary — newest first (the default: a precon browser is mostly
// "what came out recently") or by name.
const SORT_OPTIONS = ['released', 'name']
const { page, searchInput, query, sort } = useCardSearch('released', SORT_OPTIONS)

// How the decks are laid out, as a URL mode (`?view=`) so it's shareable and survives a
// reload — the card set view's own treatment of by-drop.
//
// The **defaults differ by route**, because the useful answer does. On a set's own page the
// decks are already one set, and the thing that makes 70 of them readable is the *type* split
// (Marvel ships 51 Jumpstart themes beside 12 Box Sets and 5 Welcome Decks), so it opens
// grouped by type. The all-decks view opens grouped **by set**: 2,986 decks flat is a wall
// with no landmarks, and the set that published them is how people remember a precon.
//
// Switching restarts paging: a page of groups and a page of decks don't mean the same thing,
// so carrying the number across would land you nowhere near where you were.
type PreconView = 'sets' | 'types' | 'all'
const DEFAULT_VIEW = computed<PreconView>(() => (singleSet.value ? 'types' : 'sets'))
const view = computed<PreconView>(() => {
  const raw = route.query.view
  // A *single* set page has no by-set view to offer — every deck on it is in the one set. A
  // spanned group does: splitting by set is exactly what tells the sub-sets apart.
  const allowed: PreconView[] = singleSet.value ? ['types', 'all'] : ['sets', 'types', 'all']
  return allowed.find((mode) => mode === raw) ?? DEFAULT_VIEW.value
})
const grouped = computed(() => view.value !== 'all')
const grouping = computed<PreconGrouping>(() => (view.value === 'sets' ? 'set' : 'type'))

const VIEW_OPTIONS: { value: PreconView; label: string }[] = [
  { value: 'sets', label: 'By set' },
  { value: 'types', label: 'By deck type' },
  { value: 'all', label: 'No grouping' },
]
const viewOptions = computed(() =>
  VIEW_OPTIONS.filter((option) => !(singleSet.value && option.value === 'sets')),
)
const viewModel = computed({
  get: () => view.value,
  set: (next: string) => {
    const query: LocationQueryRaw = { ...route.query }
    // The route's own default is the absent state, so a shared URL carries only a deliberate
    // choice and the default can change without breaking old links.
    if (next === DEFAULT_VIEW.value) delete query.view
    else query.view = next
    delete query.page
    router.replace({ query })
  },
})
const viewLabel = computed(
  () => VIEW_OPTIONS.find((option) => option.value === view.value)?.label ?? 'Grouping',
)

const facetsQuery = usePreconFacetsQuery(game)
const typeOptions = computed(() => facetsQuery.data.value?.data.types ?? [])
const setOptions = computed(() => facetsQuery.data.value?.data.sets ?? [])
const scopedSetRef = computed(() => setOptions.value.find((s) => s.code === props.code))
const setName = computed(
  () => scopedSetRef.value?.name ?? props.code?.toUpperCase() ?? props.code ?? '',
)
const heading = computed(() =>
  scoped.value
    ? includeRelated.value
      ? `${setName.value} & related sets`
      : setName.value
    : 'All decks',
)

const listOptions = {
  page,
  query,
  set: effectiveSet,
  includeRelated,
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
const groupsQuery = usePreconGroupsQuery(game, { ...listOptions, enabled: grouped }, grouping)
const activeQuery = computed(() => (grouped.value ? groupsQuery : preconsQuery))

const precons = computed(() => preconsQuery.data.value?.data ?? [])
const groups = computed(() => groupsQuery.data.value?.data ?? [])
const total = computed(() =>
  grouped.value ? (groupsQuery.data.value?.total ?? 0) : (preconsQuery.data.value?.total ?? 0),
)
// The count line names what a page actually holds: groups in a grouped view (in the noun that
// grouping counts), decks in the flat one.
const totalLabel = computed(() => {
  if (!grouped.value)
    return `${total.value.toLocaleString()} ${total.value === 1 ? 'deck' : 'decks'}`
  const noun = grouping.value === 'set' ? 'set' : 'deck type'
  const heads = `${total.value.toLocaleString()} ${total.value === 1 ? noun : `${noun}s`}`
  const decks = groups.value.reduce((sum, group) => sum + group.deck_count, 0)
  return decks
    ? `${heads} · ${decks.toLocaleString()} ${decks === 1 ? 'deck' : 'decks'} on this page`
    : heads
})
const resultsTop = ref<HTMLElement | null>(null)

useClampPage(page, () => ({
  ready: activeQuery.value.isSuccess.value,
  total: total.value,
  pageSize: grouped.value ? PRECON_GROUP_PAGE_SIZE : PRECON_PAGE_SIZE,
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
        `Planeswalker and Challenger decks, Jumpstart themes and intro packs — with full ` +
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
        <!-- The way back out of a spanned group. The landing is the only way in, so the
             single-set page needs no matching "span the group" link of its own. -->
        <RouterLink
          v-if="includeRelated"
          :to="`/decks/${game}/precons/sets/${code}`"
          class="text-muted-foreground hover:text-foreground mt-1 inline-block text-sm underline underline-offset-4"
        >
          View just {{ setName }}
        </RouterLink>
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
      <!-- How the decks are laid out. A menu rather than the by-drop view's two-button
           toggle, because there are three answers here (and two on a set page). -->
      <RadioSelectMenu
        v-model="viewModel"
        :options="viewOptions"
        label="Grouping"
        :trigger-icon="LayoutGrid"
        :trigger-label="viewLabel"
        content-class="w-44"
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
        <!-- Grouped: one heading per group — a set (linking to its own page) or a deck type —
             then its decks in the same tile grid the flat view uses. -->
        <div v-if="grouped" class="space-y-10">
          <section v-for="group in groups" :id="group.slug" :key="group.slug">
            <div class="mb-3 flex flex-wrap items-baseline gap-2 border-b pb-1.5">
              <RouterLink
                v-if="group.set_code"
                :to="`/decks/${game}/precons/sets/${group.set_code}`"
                class="text-xl font-semibold tracking-tight hover:underline"
              >
                {{ group.title }}
              </RouterLink>
              <h2 v-else class="text-xl font-semibold tracking-tight">{{ group.title }}</h2>
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
          :page-size="grouped ? PRECON_GROUP_PAGE_SIZE : PRECON_PAGE_SIZE"
          :total="total"
          :loading="activeQuery.isPlaceholderData.value"
          :scroll-target="resultsTop"
        />
      </div>
    </template>
  </div>
</template>
