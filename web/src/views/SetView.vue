<script setup lang="ts">
import { computed, toRef, watch } from 'vue'
import { keepPreviousData, useQuery } from '@tanstack/vue-query'
import { ArrowLeft, Check, ChevronDown, Layers, Loader2, Search } from '@lucide/vue'
import { RouterLink, useRoute, useRouter, type LocationQueryRaw } from 'vue-router'
import { buttonVariants } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import CardGrid from '@/components/cards/CardGrid.vue'
import CardPagination from '@/components/cards/CardPagination.vue'
import CardSizeMenu from '@/components/cards/CardSizeMenu.vue'
import CardSortMenu from '@/components/cards/CardSortMenu.vue'
import SearchSyntaxHint from '@/components/cards/SearchSyntaxHint.vue'
import { searchErrorMessage, useCardSearch } from '@/composables/useCardSearch'
import { SET_DEFAULT_SORT, SET_SORT_OPTIONS, toSortParam } from '@/lib/cardSort'
import { getSet, listSetCards, listSetDrops, listSets } from '@/lib/api'
import { findGroup, originSetCode, subSetLabel } from '@/lib/setGroups'
import { cn } from '@/lib/utils'

const props = defineProps<{ game: string; code: string }>()
const game = toRef(props, 'game')
const code = toRef(props, 'code')

const route = useRoute()
const router = useRouter()

const PAGE_SIZE = 60
// The by-drop view paginates over *drops* (each a handful of cards), so it uses
// a smaller page size than the flat card grid.
const DROP_PAGE_SIZE = 20
// Page, search and sort live in the URL query (alongside the related/from scope), so
// they survive opening a card and pressing Back. Routing to a different set lands on
// a fresh URL, so it starts clean.
const { page, searchInput, query, sort } = useCardSearch(
  SET_DEFAULT_SORT,
  SET_SORT_OPTIONS.map((option) => option.value),
)

// The full set list (shared, cached with GameView) tells us whether this set has
// related sub-sets to fold in.
const setsQuery = useQuery({
  queryKey: ['sets', game],
  queryFn: () => listSets(game.value),
  staleTime: 5 * 60 * 1000,
})
const group = computed(() => findGroup(setsQuery.data.value?.data ?? [], code.value))
const isMainSet = computed(() => group.value?.main.code === code.value)
// The count of *other* sets in the group — equal from any member's viewpoint (a
// child's siblings + the main = the main's children count), so it reads correctly
// whether you're on the main set or one of its sub-sets.
const relatedCount = computed(() => group.value?.children.length ?? 0)
const hasRelated = computed(() => relatedCount.value > 0)

// The "view related" state lives in the URL (?related=1) so it's shareable and
// survives a reload, but only takes effect when there actually are related sets.
const includeRelated = computed(() => route.query.related === '1' && hasRelated.value)

// Every set in the group — the main set first, then its sub-sets — offered in
// the "view just one set" menu so you can drop into any specific one.
const members = computed(() => (group.value ? [group.value.main, ...group.value.children] : []))
const memberOptions = computed(() =>
  members.value.map((member) => ({
    code: member.code,
    name: member.name,
    // The main set keeps its full name for context; sub-sets drop the redundant
    // parent prefix ("Bloomburrow Commander" → "Commander").
    label:
      member.code === group.value?.main.code
        ? member.name
        : subSetLabel(group.value?.main.name ?? '', member.name),
  })),
)
// The single set currently on screen, flagged as "current" in the picker (so
// re-selecting it reads as a no-op rather than a dead click). Null in the grouped
// view, where no single set is on screen and every option is a real destination.
const activeSetCode = computed(() => (includeRelated.value ? null : code.value))

// The set "View just this set" drops back to: the one the grouped view was
// entered from (?from=…), else the group's main set. This is what fixes landing
// on the parent set after a sub-set → "view all together" → "view just this set"
// round-trip — the original set is remembered, not discarded.
const fromCode = computed(() => (typeof route.query.from === 'string' ? route.query.from : null))
const originCode = computed(() =>
  group.value ? originSetCode(group.value, fromCode.value) : code.value,
)
const originName = computed(
  () => members.value.find((m) => m.code === originCode.value)?.name ?? '',
)

// Keep the search + sort controls when only the view scope toggles; paging always
// restarts (page is intentionally dropped, so it reads back as 1 — switching scope
// must never strand us on an out-of-range page).
function listState(): LocationQueryRaw {
  const next: LocationQueryRaw = {}
  if (typeof route.query.q === 'string' && route.query.q) next.q = route.query.q
  if (typeof route.query.sort === 'string' && route.query.sort) next.sort = route.query.sort
  return next
}

function setIncludeRelated(on: boolean) {
  if (on) {
    // Root the grouped view at the main set so the URL, heading and counts all
    // agree (matching SetGroup's "View all" link). Entering from a sub-set
    // navigates up to the main set, remembering where we came from (?from=…) so
    // "View just this set" can return there rather than stranding us on the parent;
    // a different set is a fresh scope, so the search/sort don't carry over.
    if (group.value && !isMainSet.value) {
      router.replace({
        path: `/cards/${game.value}/sets/${group.value.main.code}`,
        query: { related: '1', from: code.value },
      })
    } else {
      router.replace({ query: { ...listState(), related: '1' } })
    }
  } else {
    viewSingleSet(originCode.value)
  }
}

// Leave the grouped view for a single set's own page. Staying on the set already in
// the route just sheds the related/from scope (search + sort carry over); otherwise
// route to the chosen set fresh.
function viewSingleSet(target: string) {
  if (target === code.value) {
    router.replace({ query: listState() })
  } else {
    router.replace({ path: `/cards/${game.value}/sets/${target}`, query: {} })
  }
}

const setQuery = useQuery({
  queryKey: ['set', game, code],
  queryFn: () => getSet(game.value, code.value),
  staleTime: 5 * 60 * 1000,
})

const set = computed(() => setQuery.data.value)
// Whether this set is browsable broken down into Secret Lair-style "drops".
// Sourced from the (game-keyed, usually-warm) set list rather than the per-set
// metadata, so it's known up front and stays stable across set→set navigation —
// no flat-grid flash, no throwaway flat fetch. By-drop is the default for such
// sets; ?view=all opts back into the flat grid, and the related-sets view
// (?related=1) is itself a flat listing, so it suppresses by-drop too.
const hasDrops = computed(
  () => setsQuery.data.value?.data.find((s) => s.code === code.value)?.has_drops ?? false,
)
const byDrop = computed(
  () => hasDrops.value && route.query.view !== 'all' && !includeRelated.value,
)

const cardsQuery = useQuery({
  queryKey: ['set-cards', game, code, query, sort, page, includeRelated],
  queryFn: () =>
    listSetCards(game.value, code.value, {
      q: query.value || undefined,
      page: page.value,
      pageSize: PAGE_SIZE,
      includeRelated: includeRelated.value || undefined,
      ...toSortParam(sort.value, SET_DEFAULT_SORT),
    }),
  // Skip the flat list while the by-drop view is active, and wait for the set
  // list to settle first — it's what tells us whether this is a drop set (and
  // resolves the related grouping), so we never fire a throwaway flat request
  // that a cold-loaded by-drop / related link would immediately discard.
  enabled: computed(() => !byDrop.value && !setsQuery.isPending.value),
  placeholderData: keepPreviousData,
  staleTime: 5 * 60 * 1000,
})

const dropsQuery = useQuery({
  queryKey: ['set-drops', game, code, query, page],
  queryFn: () =>
    listSetDrops(game.value, code.value, {
      q: query.value || undefined,
      page: page.value,
      pageSize: DROP_PAGE_SIZE,
    }),
  enabled: byDrop,
  placeholderData: keepPreviousData,
  staleTime: 5 * 60 * 1000,
})

const cards = computed(() => cardsQuery.data.value?.data ?? [])
const total = computed(() => cardsQuery.data.value?.total ?? 0)
const dropGroups = computed(() => dropsQuery.data.value?.data ?? [])
const dropTotal = computed(() => dropsQuery.data.value?.total ?? 0)

// The list's loading / error / empty state reads from whichever query drives the
// current view. cardsQuery waits on the set list, so an as-yet-undecided drop set
// shows the active query's own pending state (no flat-grid flash), while
// keepPreviousData still carries the prior set's cards smoothly across navigation.
const listPending = computed(() =>
  byDrop.value ? dropsQuery.isPending.value : cardsQuery.isPending.value,
)
const listError = computed(() => (byDrop.value ? dropsQuery.error.value : cardsQuery.error.value))
const listIsError = computed(() =>
  byDrop.value ? dropsQuery.isError.value : cardsQuery.isError.value,
)
const isEmpty = computed(() =>
  byDrop.value ? dropGroups.value.length === 0 : cards.value.length === 0,
)

// Toggle the by-drop vs flat view of this set. Preserves the search + sort (like the
// related-scope controls) but sheds the related/from scope and restarts paging (page
// is dropped by listState) — the two views paginate over different units. ?view=all
// marks the flat mode; by-drop is the bare default.
function setView(mode: 'drops' | 'all') {
  const next = listState()
  if (mode === 'all') next.view = 'all'
  router.replace({ query: next })
}

// A shared or stale link can point past the last page (a bookmarked search whose
// results later shrank, or a hand-edited ?page). Once the real total is known, clamp
// back so we never strand the user on an empty page with no pager to escape it. The
// active view sets the unit: drops (by-drop) or printings (flat).
watch(
  () =>
    byDrop.value
      ? ([dropsQuery.isSuccess.value, dropTotal.value, DROP_PAGE_SIZE] as const)
      : ([cardsQuery.isSuccess.value, total.value, PAGE_SIZE] as const),
  ([ok, count, size]) => {
    if (!ok) return
    const lastPage = Math.max(1, Math.ceil(count / size))
    if (page.value > lastPage) page.value = lastPage
  },
  { immediate: true },
)

// When folding in related sets, the page is rooted at the group's main set.
const heading = computed(() =>
  includeRelated.value && group.value
    ? group.value.main.name
    : (set.value?.name ?? code.value.toUpperCase()),
)
const setsWord = computed(() => (relatedCount.value === 1 ? 'set' : 'sets'))
const countLabel = computed(() => {
  // By-drop mode counts drops; the flat view counts card printings.
  const [n, singular] = byDrop.value ? [dropTotal.value, 'drop'] : [total.value, 'printing']
  if (!n && !query.value) return ''
  const label = `${n.toLocaleString()} ${n === 1 ? singular : `${singular}s`}`
  return query.value ? `${label} matching “${query.value}”` : label
})
// A malformed search query comes back as 422; surface its message inline.
const searchError = computed(() => searchErrorMessage(listError.value))
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-10">
    <RouterLink
      :to="`/cards/${game}`"
      class="text-muted-foreground hover:text-foreground mb-4 inline-flex items-center gap-1.5 text-sm"
    >
      <ArrowLeft class="size-4" />
      All sets
    </RouterLink>

    <p v-if="setQuery.isError.value" class="text-destructive py-12">Set not found.</p>

    <template v-else>
      <header class="mb-6 flex flex-wrap items-center justify-between gap-4">
        <div>
          <h1 class="text-3xl font-semibold tracking-tight">{{ heading }}</h1>
          <p class="text-muted-foreground mt-1 text-sm">
            <template v-if="includeRelated">{{ relatedCount }} related {{ setsWord }}</template>
            <template v-else>
              <span class="uppercase">{{ code }}</span>
              <template v-if="set?.set_type"> · {{ set?.set_type?.replace('_', ' ') }}</template>
            </template>
            <template v-if="countLabel"> · {{ countLabel }}</template>
          </p>
        </div>
        <div class="w-full sm:w-80">
          <div class="relative">
            <Search
              class="text-muted-foreground pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2"
            />
            <Input
              v-model="searchInput"
              :placeholder="
                includeRelated
                  ? 'Search these sets — c:r, t:land…'
                  : 'Search this set — c:r, t:land…'
              "
              class="pl-9"
            />
          </div>
          <SearchSyntaxHint class="mt-1.5" />
        </div>
      </header>

      <!-- Offer folding the set's related sub-sets (tokens, promos, decks, …) into
           one listing instead of visiting each individually. Hidden in the by-drop
           view, which groups the set's own cards rather than spanning sub-sets. -->
      <div
        v-if="hasRelated && !byDrop"
        class="bg-muted/40 mb-6 flex flex-wrap items-center justify-between gap-3 rounded-lg border p-3"
      >
        <p class="text-muted-foreground text-sm">
          <template v-if="includeRelated">
            Showing {{ group?.main.name }} with its {{ relatedCount }} related {{ setsWord }}.
          </template>
          <template v-else-if="isMainSet">
            This set has {{ relatedCount }} related {{ setsWord }} — tokens, promos, decks and more.
          </template>
          <template v-else>
            Part of {{ group?.main.name }} — {{ relatedCount }} related {{ setsWord }} in this
            group.
          </template>
        </p>
        <!-- A split button in both modes. The main action toggles the grouped
             view — fold the related sub-sets in, or (when grouped) return to the
             set you came from — while the caret always opens a menu to drop
             straight into any single set in the group. -->
        <div class="flex">
          <button
            v-if="!includeRelated"
            type="button"
            :class="cn(buttonVariants({ variant: 'default', size: 'sm' }), 'rounded-r-none')"
            @click="setIncludeRelated(true)"
          >
            <Layers />
            View all together
          </button>
          <button
            v-else
            type="button"
            :class="cn(buttonVariants({ variant: 'outline', size: 'sm' }), 'rounded-r-none')"
            :title="originName ? `View just ${originName}` : undefined"
            @click="setIncludeRelated(false)"
          >
            <Layers />
            View just this set
          </button>
          <DropdownMenu>
            <DropdownMenuTrigger as-child>
              <button
                type="button"
                :class="
                  cn(
                    buttonVariants({
                      variant: includeRelated ? 'outline' : 'default',
                      size: 'icon-sm',
                    }),
                    '-ml-px rounded-l-none',
                    // The outline variant's border already divides the two halves;
                    // the filled variant has none, so add a faint seam ourselves
                    // (else the chevron reads as part of one solid block — no hover
                    // on touch to reveal it).
                    !includeRelated && 'border-l border-l-primary-foreground/20',
                  )
                "
                aria-label="Jump to a set in this group"
              >
                <ChevronDown />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" class="max-w-64">
              <DropdownMenuLabel>Jump to a set</DropdownMenuLabel>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                v-for="option in memberOptions"
                :key="option.code"
                :title="option.name"
                @select="viewSingleSet(option.code)"
              >
                <span class="min-w-0 truncate">{{ option.label }}</span>
                <Check v-if="option.code === activeSetCode" class="ml-auto shrink-0" />
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>

      <div v-if="listPending" class="text-muted-foreground flex items-center gap-2 py-12">
        <Loader2 class="size-4 animate-spin" />
        Loading cards…
      </div>
      <p v-else-if="listIsError" class="text-destructive py-12">
        {{ searchError ?? "Couldn't load cards. Please retry." }}
      </p>
      <p v-else-if="isEmpty && query" class="text-muted-foreground py-12">
        No cards match “{{ query }}”.
      </p>
      <p v-else-if="isEmpty" class="text-muted-foreground py-12">
        No cards in {{ includeRelated ? 'these sets' : 'this set' }} yet.
      </p>

      <template v-else>
        <!-- Controls: a By-drop / All-cards toggle for drop sets, a card-size
             menu, and the sort menu (flat view only — the by-drop view has a
             fixed drop order). The size menu shows in both views since the
             by-drop sections are grids too. -->
        <div class="mb-4 flex items-center justify-between gap-3">
          <div
            v-if="hasDrops"
            class="bg-muted text-muted-foreground inline-flex rounded-md p-0.5 text-sm"
          >
            <button
              type="button"
              :class="
                cn(
                  'rounded px-3 py-1 font-medium transition-colors',
                  byDrop ? 'bg-background text-foreground shadow-sm' : 'hover:text-foreground',
                )
              "
              @click="setView('drops')"
            >
              By drop
            </button>
            <button
              type="button"
              :class="
                cn(
                  'rounded px-3 py-1 font-medium transition-colors',
                  !byDrop ? 'bg-background text-foreground shadow-sm' : 'hover:text-foreground',
                )
              "
              @click="setView('all')"
            >
              All cards
            </button>
          </div>
          <span v-else />
          <div class="flex gap-2">
            <CardSizeMenu />
            <CardSortMenu v-if="!byDrop" v-model="sort" :options="SET_SORT_OPTIONS" />
          </div>
        </div>

        <!-- By-drop: one section per Secret Lair drop, paginated by drop. -->
        <template v-if="byDrop">
          <section
            v-for="drop in dropGroups"
            :id="drop.slug ?? undefined"
            :key="drop.slug ?? drop.title"
            class="mb-10 scroll-mt-20"
          >
            <div class="mb-4 flex items-baseline gap-2 border-b pb-2">
              <h2 class="text-lg font-semibold tracking-tight">{{ drop.title }}</h2>
              <span class="text-muted-foreground text-sm tabular-nums">
                {{ drop.card_count }} {{ drop.card_count === 1 ? 'card' : 'cards' }}
              </span>
            </div>
            <CardGrid :game="game" :cards="drop.cards" />
          </section>
          <div class="mt-10">
            <CardPagination v-model:page="page" :page-size="DROP_PAGE_SIZE" :total="dropTotal" />
          </div>
        </template>

        <!-- Flat: the whole set as one collector-ordered grid. -->
        <template v-else>
          <CardGrid :game="game" :cards="cards" />
          <div class="mt-10">
            <CardPagination v-model:page="page" :page-size="PAGE_SIZE" :total="total" />
          </div>
        </template>
      </template>
    </template>
  </div>
</template>
