<script setup lang="ts">
import { computed, toRef, watch } from 'vue'
import { keepPreviousData, useQuery } from '@tanstack/vue-query'
import { ArrowLeft, ChevronDown, Layers, Loader2, Search } from '@lucide/vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
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
import SearchSyntaxHint from '@/components/cards/SearchSyntaxHint.vue'
import { searchErrorMessage, useCardSearch } from '@/composables/useCardSearch'
import { getSet, listSetCards, listSets } from '@/lib/api'
import { findGroup, originSetCode, subSetLabel } from '@/lib/setGroups'
import { cn } from '@/lib/utils'

const props = defineProps<{ game: string; code: string }>()
const game = toRef(props, 'game')
const code = toRef(props, 'code')

const route = useRoute()
const router = useRouter()

const PAGE_SIZE = 60
// Navigating to a different set starts fresh (search + page).
const { page, searchInput, query } = useCardSearch(code)

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

function setIncludeRelated(on: boolean) {
  if (on) {
    // Root the grouped view at the main set so the URL, heading and counts all
    // agree (matching SetGroup's "View all" link). Entering from a sub-set
    // navigates up to the main set, remembering where we came from (?from=…) so
    // "View just this set" can return there rather than stranding us on the parent.
    if (group.value && !isMainSet.value) {
      router.replace({
        path: `/cards/${game.value}/sets/${group.value.main.code}`,
        query: { related: '1', from: code.value },
      })
    } else {
      router.replace({ query: { related: '1' } })
    }
  } else {
    viewSingleSet(originCode.value)
  }
}

// Leave the grouped view for a single set's own page. Clearing the query
// (related/from) is enough when it's the set already in the route; otherwise
// route to the chosen set.
function viewSingleSet(target: string) {
  if (target === code.value) {
    router.replace({ query: {} })
  } else {
    router.replace({ path: `/cards/${game.value}/sets/${target}`, query: {} })
  }
}
// Switching scope restarts pagination so we never land on an out-of-range page.
watch(includeRelated, () => {
  page.value = 1
})

const setQuery = useQuery({
  queryKey: ['set', game, code],
  queryFn: () => getSet(game.value, code.value),
  staleTime: 5 * 60 * 1000,
})

const cardsQuery = useQuery({
  queryKey: ['set-cards', game, code, query, page, includeRelated],
  queryFn: () =>
    listSetCards(game.value, code.value, {
      q: query.value || undefined,
      page: page.value,
      pageSize: PAGE_SIZE,
      includeRelated: includeRelated.value || undefined,
    }),
  // When the URL requests the related view, wait for the set list to settle before
  // fetching, so a cold-loaded (bookmarked/reloaded) link doesn't fire a throwaway
  // single-set request and flash the wrong heading before the group resolves.
  enabled: computed(() => route.query.related !== '1' || !setsQuery.isPending.value),
  placeholderData: keepPreviousData,
  staleTime: 5 * 60 * 1000,
})

const set = computed(() => setQuery.data.value)
const cards = computed(() => cardsQuery.data.value?.data ?? [])
const total = computed(() => cardsQuery.data.value?.total ?? 0)

// When folding in related sets, the page is rooted at the group's main set.
const heading = computed(() =>
  includeRelated.value && group.value
    ? group.value.main.name
    : (set.value?.name ?? code.value.toUpperCase()),
)
const setsWord = computed(() => (relatedCount.value === 1 ? 'set' : 'sets'))
const printingsLabel = computed(() => {
  if (!total.value && !query.value) return ''
  const printings = `${total.value.toLocaleString()} ${total.value === 1 ? 'printing' : 'printings'}`
  return query.value ? `${printings} matching “${query.value}”` : printings
})
// A malformed search query comes back as 422; surface its message inline.
const searchError = computed(() => searchErrorMessage(cardsQuery.error.value))
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
            <template v-if="printingsLabel"> · {{ printingsLabel }}</template>
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
           one listing instead of visiting each individually. -->
      <div
        v-if="hasRelated"
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
        <!-- Single set: one button to fold the related sub-sets in. -->
        <button
          v-if="!includeRelated"
          type="button"
          :class="buttonVariants({ variant: 'default', size: 'sm' })"
          @click="setIncludeRelated(true)"
        >
          <Layers />
          View all together
        </button>

        <!-- Grouped: a split button. The main action returns to the set you came
             from; the caret opens a menu to drop into any one set in the group. -->
        <div v-else class="flex">
          <button
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
                    buttonVariants({ variant: 'outline', size: 'icon-sm' }),
                    '-ml-px rounded-l-none',
                  )
                "
                aria-label="View just one set in this group"
              >
                <ChevronDown />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" class="max-w-64">
              <DropdownMenuLabel>View just one set</DropdownMenuLabel>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                v-for="option in memberOptions"
                :key="option.code"
                :title="option.name"
                @select="viewSingleSet(option.code)"
              >
                <span class="truncate">{{ option.label }}</span>
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>

      <div
        v-if="cardsQuery.isPending.value"
        class="text-muted-foreground flex items-center gap-2 py-12"
      >
        <Loader2 class="size-4 animate-spin" />
        Loading cards…
      </div>
      <p v-else-if="cardsQuery.isError.value" class="text-destructive py-12">
        {{ searchError ?? "Couldn't load cards. Please retry." }}
      </p>
      <p v-else-if="!cards.length && query" class="text-muted-foreground py-12">
        No cards match “{{ query }}”.
      </p>
      <p v-else-if="!cards.length" class="text-muted-foreground py-12">
        No cards in {{ includeRelated ? 'these sets' : 'this set' }} yet.
      </p>

      <template v-else>
        <CardGrid :game="game" :cards="cards" />
        <div class="mt-10">
          <CardPagination v-model:page="page" :page-size="PAGE_SIZE" :total="total" />
        </div>
      </template>
    </template>
  </div>
</template>
