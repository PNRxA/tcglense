<script setup lang="ts">
import { computed, ref, watch, type ComponentPublicInstance } from 'vue'
import { RouterLink, useRouter } from 'vue-router'
import { useId } from 'reka-ui'
import {
  BookCopy,
  BookOpen,
  Boxes,
  ChevronRight,
  Layers,
  Loader2,
  Package,
  Search,
} from '@lucide/vue'
import CardImage from '@/components/cards/CardImage.vue'
import ProductImage from '@/components/products/ProductImage.vue'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useUniversalSearch } from '@/composables/useUniversalSearch'
import type { Game } from '@/lib/api'
import { prefetchRouteChunks } from '@/lib/prefetch'
import { cardSearchLocation, type SearchGroupView, type SearchOption } from '@/lib/universalSearch'

// The homepage's universal search: one box that answers across cards, sealed products,
// preconstructed decks, the keyword glossary and — signed in — your own decks, as a grouped
// dropdown of the top matches, each a link to the thing itself. Enter with nothing
// highlighted (or the closing row) hands off to the full card search, where the whole
// Scryfall grammar applies.
//
// A hand-rolled combobox in QuickAddBox's idiom rather than reka's: the list is driven
// purely by the server's groups with full control over the async/keyboard behaviour, and
// every row is a real link (RouterLink), so a middle-click or a long-press opens it in a
// new tab like any other link. Everything reactive lives in `useUniversalSearch`; this file
// is the markup and the ARIA wiring (combobox → listbox of labelled groups of options).
//
// Games arrive from the registry; the box searches the first one and only shows a picker
// when there are several, so a single-game deployment sees just a search box.
const props = defineProps<{ games: Game[] }>()

const selectedGame = ref('')
watch(
  () => props.games,
  (games) => {
    if (!games.some((game) => game.id === selectedGame.value))
      selectedGame.value = games[0]?.id ?? ''
  },
  { immediate: true },
)
const gameName = computed(
  () => props.games.find((game) => game.id === selectedGame.value)?.name ?? 'the catalog',
)

const {
  term,
  searchedTerm,
  showDropdown,
  groups,
  footer,
  activeOption,
  pending,
  status,
  onFocus,
  onBlur,
  onKeydown,
  highlight,
  close,
} = useUniversalSearch(selectedGame)

const inputRef = ref<ComponentPublicInstance | null>(null)

// Stable ids for the combobox/listbox ARIA wiring. Option keys are `kind:id`, which is
// valid in an id and unique across the list.
const baseId = useId()
const listboxId = `${baseId}-listbox`
const optionId = (option: SearchOption) => `${baseId}-${option.key}`
const groupId = (group: SearchGroupView) => `${baseId}-group-${group.id}`
const activeDescendant = computed(() =>
  showDropdown.value && activeOption.value ? optionId(activeOption.value) : undefined,
)

// Keep the highlighted row in view as the arrow keys move it past the scroll edge. The rows
// highlight on `mousemove`, not `mouseenter`, so the content this scrolls under a parked
// pointer can't steal the keyboard's highlight back (the reka/Radix rule).
watch(activeDescendant, (id) => {
  if (id) document.getElementById(id)?.scrollIntoView?.({ block: 'nearest' })
})

// Enter and the closing row both land on the card listing: warm its chunk as soon as the
// box is focused so the hand-off paints from cache (chunks only, never data).
const router = useRouter()
function onInputFocus() {
  onFocus()
  if (selectedGame.value) prefetchRouteChunks(router, cardSearchLocation(selectedGame.value, ''))
}

const GROUP_ICONS = {
  card: Layers,
  deck: BookCopy,
  product: Package,
  precon: Boxes,
  keyword: BookOpen,
} as const

const placeholder = computed(
  () => `Search ${gameName.value} cards, sealed products, precons, keywords…`,
)
</script>

<template>
  <div class="relative">
    <div class="flex gap-2">
      <div class="relative min-w-0 flex-1">
        <Search
          class="text-muted-foreground pointer-events-none absolute top-1/2 left-3.5 size-5 -translate-y-1/2"
          aria-hidden="true"
        />
        <Input
          ref="inputRef"
          v-model="term"
          type="search"
          class="h-12 rounded-xl pr-10 pl-11 text-base shadow-sm md:text-base"
          :placeholder="placeholder"
          aria-label="Search cards, sealed products, preconstructed decks, keywords, and your decks"
          role="combobox"
          aria-autocomplete="list"
          autocomplete="off"
          spellcheck="false"
          :aria-expanded="showDropdown"
          :aria-controls="listboxId"
          :aria-activedescendant="activeDescendant"
          @keydown="onKeydown"
          @focus="onInputFocus"
          @blur="onBlur"
        />
        <Loader2
          v-if="showDropdown && pending"
          class="text-muted-foreground absolute top-1/2 right-3.5 size-4 -translate-y-1/2 animate-spin"
          aria-hidden="true"
        />
      </div>
      <!-- Only a multi-game deployment gets a picker; today's single game needs none. -->
      <Select v-if="games.length > 1" v-model="selectedGame">
        <SelectTrigger class="h-12 shrink-0 rounded-xl" aria-label="Game to search">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem v-for="game in games" :key="game.id" :value="game.id">
            {{ game.name }}
          </SelectItem>
        </SelectContent>
      </Select>
    </div>

    <!-- z-40 keeps the panel above the page's sticky chrome, as QuickAddBox's list does. -->
    <div
      v-if="showDropdown"
      :id="listboxId"
      role="listbox"
      aria-label="Search results"
      class="bg-popover text-popover-foreground absolute z-40 mt-2 max-h-[70vh] w-full overflow-auto rounded-xl border p-1.5 shadow-lg"
    >
      <div
        v-for="group in groups"
        :key="group.id"
        role="group"
        :aria-labelledby="groupId(group)"
        class="py-1"
      >
        <div
          :id="groupId(group)"
          class="text-muted-foreground flex items-center gap-1.5 px-3 pt-1.5 pb-1 text-xs font-medium tracking-wide uppercase"
        >
          <component :is="GROUP_ICONS[group.id]" class="size-3.5" aria-hidden="true" />
          {{ group.label }}
        </div>
        <RouterLink
          v-for="option in group.options"
          :id="optionId(option)"
          :key="option.key"
          :to="option.to"
          role="option"
          tabindex="-1"
          :aria-selected="option.key === activeOption?.key"
          class="flex w-full items-center gap-3 rounded-lg px-3 py-1.5 text-left text-sm outline-none transition-colors"
          :class="[
            option.key === activeOption?.key ? 'bg-accent text-accent-foreground' : '',
            option.kind === 'more' ? 'text-primary font-medium' : '',
          ]"
          @mousedown.prevent
          @mousemove="highlight(option.key)"
          @click="close()"
        >
          <!-- The row's text already names the thing, so its thumbnail is decorative. -->
          <template v-if="option.kind === 'more'">
            <ChevronRight class="text-muted-foreground size-4 shrink-0" aria-hidden="true" />
          </template>
          <template v-else-if="option.thumbnail?.kind === 'card'">
            <CardImage
              :game="selectedGame"
              :id="option.thumbnail.id"
              :name="option.thumbnail.name"
              :has-image="option.thumbnail.hasImage"
              size="small"
              class="w-10 shrink-0"
              aria-hidden="true"
            />
          </template>
          <template v-else-if="option.thumbnail?.kind === 'product'">
            <ProductImage
              :game="selectedGame"
              :id="option.thumbnail.id"
              :name="option.thumbnail.name"
              :has-image="option.thumbnail.hasImage"
              size="small"
              class="w-10 shrink-0"
              aria-hidden="true"
            />
          </template>
          <span
            v-else
            class="bg-muted text-muted-foreground flex size-10 shrink-0 items-center justify-center rounded-md"
            aria-hidden="true"
          >
            <component :is="GROUP_ICONS[group.id]" class="size-4" />
          </span>
          <span class="min-w-0 flex-1">
            <span class="block truncate">{{ option.label }}</span>
            <span v-if="option.sublabel" class="text-muted-foreground block truncate text-xs">
              {{ option.sublabel }}
            </span>
          </span>
        </RouterLink>
      </div>

      <div
        v-if="status === 'pending' || status === 'error' || status === 'empty'"
        class="text-muted-foreground flex items-center gap-2 px-3 py-2.5 text-sm"
        :class="status === 'error' ? 'text-destructive' : ''"
        role="status"
      >
        <template v-if="status === 'pending'">
          <Loader2 class="size-4 animate-spin" aria-hidden="true" />
          Searching…
        </template>
        <template v-else-if="status === 'error'">Search is unavailable right now.</template>
        <template v-else
          >No cards, sealed products, decks, or keywords match “{{ searchedTerm }}”.</template
        >
      </div>

      <!-- The closing row: always offered, so the full grammar is one Enter away even when
           nothing matched by name. Fenced from the groups above by a rule. -->
      <RouterLink
        v-if="footer"
        :id="optionId(footer)"
        :to="footer.to"
        role="option"
        tabindex="-1"
        :aria-selected="footer.key === activeOption?.key"
        class="mt-1 flex w-full items-center gap-3 rounded-lg border-t px-3 py-2 text-left text-sm outline-none transition-colors"
        :class="footer.key === activeOption?.key ? 'bg-accent text-accent-foreground' : ''"
        @mousedown.prevent
        @mousemove="highlight(footer.key)"
        @click="close()"
      >
        <span
          class="bg-primary/10 text-primary flex size-10 shrink-0 items-center justify-center rounded-md"
          aria-hidden="true"
        >
          <Search class="size-4" />
        </span>
        <span class="min-w-0 flex-1">
          <span class="block truncate font-medium">{{ footer.label }}</span>
          <span class="text-muted-foreground block truncate text-xs">{{ footer.sublabel }}</span>
        </span>
        <kbd
          class="bg-muted text-muted-foreground hidden rounded border px-1.5 py-0.5 font-mono text-[10px] sm:inline"
          aria-hidden="true"
        >
          Enter
        </kbd>
      </RouterLink>
    </div>
  </div>
</template>
