<script setup lang="ts">
import { computed, onUnmounted, ref, useId, watch } from 'vue'
import { BookOpen, X } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import ArtTagBrowser from './ArtTagBrowser.vue'
import { ART_TAG_MIN_CHARS, useArtTagSuggestions } from '@/composables/useArtTags'
import { addArtTag, artTagSlug, getArtTagValues, removeArtTag } from '@/lib/searchBuilder'

// The advanced-search panel's art-tag control (issue #140). Art tags are
// multi-valued (like the `is:` flags): each picked tag is its own `art:<slug>` token,
// shown as a removable chip. New tags come from a typed term with server-backed
// suggestions, or from the A–Z tag browser behind the Browse button.
const query = defineModel<string>({ required: true })
const props = defineProps<{ game: string }>()

const chips = computed(() => getArtTagValues(query.value))

const term = ref('')
const debouncedTerm = ref('')
const listboxOpen = ref(false)
const activeIndex = ref(-1)
const browserOpen = ref(false)

const gameRef = computed(() => props.game)
const { data, isPending } = useArtTagSuggestions(gameRef, debouncedTerm)

const trimmedTerm = computed(() => term.value.trim())
// Hide tags that are already chips — suggesting them again would just no-op.
const suggestions = computed(() => {
  const picked = new Set(chips.value.map(artTagSlug))
  return (data.value?.data ?? []).filter((t) => !picked.has(t.slug))
})
const showDropdown = computed(
  () => listboxOpen.value && trimmedTerm.value.length >= ART_TAG_MIN_CHARS,
)

// Stable ids for the combobox/listbox ARIA wiring.
const baseId = useId()
const listboxId = `${baseId}-listbox`
function optionId(index: number): string {
  return `${baseId}-option-${index}`
}
const activeDescendant = computed(() =>
  showDropdown.value && activeIndex.value >= 0 ? optionId(activeIndex.value) : undefined,
)

let debounceTimer: ReturnType<typeof setTimeout> | undefined
let blurTimer: ReturnType<typeof setTimeout> | undefined

watch(term, (value) => {
  clearTimeout(debounceTimer)
  debounceTimer = setTimeout(() => {
    debouncedTerm.value = value
  }, 250)
  if (value.trim().length >= ART_TAG_MIN_CHARS) listboxOpen.value = true
})

watch(suggestions, () => {
  activeIndex.value = -1
})

onUnmounted(() => {
  clearTimeout(debounceTimer)
  clearTimeout(blurTimer)
})

function onFocus() {
  clearTimeout(blurTimer)
  if (trimmedTerm.value.length >= ART_TAG_MIN_CHARS) listboxOpen.value = true
}

function onBlur() {
  // Delay closing so an option's mousedown→click lands first.
  blurTimer = setTimeout(() => {
    listboxOpen.value = false
  }, 120)
}

function add(value: string) {
  query.value = addArtTag(query.value, value)
  clearTimeout(debounceTimer)
  term.value = ''
  debouncedTerm.value = ''
  listboxOpen.value = false
  activeIndex.value = -1
}

function remove(value: string) {
  query.value = removeArtTag(query.value, value)
}

function onKeydown(event: KeyboardEvent) {
  switch (event.key) {
    case 'ArrowDown':
      event.preventDefault()
      if (!showDropdown.value) {
        listboxOpen.value = true
        return
      }
      if (suggestions.value.length) {
        activeIndex.value = Math.min(activeIndex.value + 1, suggestions.value.length - 1)
      }
      break
    case 'ArrowUp':
      event.preventDefault()
      if (suggestions.value.length) {
        activeIndex.value = Math.max(activeIndex.value - 1, 0)
      }
      break
    case 'Enter': {
      const choice = suggestions.value[activeIndex.value]
      // A highlighted suggestion wins; otherwise the typed term is added as-is
      // (slug-normalised) — hand-typing a known slug shouldn't force a round trip.
      if (showDropdown.value && activeIndex.value >= 0 && choice) {
        event.preventDefault()
        add(choice.slug)
      } else if (trimmedTerm.value) {
        event.preventDefault()
        add(trimmedTerm.value)
      }
      break
    }
    case 'Escape':
      if (showDropdown.value) {
        event.preventDefault()
        listboxOpen.value = false
        activeIndex.value = -1
      }
      break
  }
}

const inputId = useId()

// The browser dialog has no trigger element, so reka's default close-focus would land
// on <body> — read as "focus outside" by the Filters popover hosting this control,
// dismissing the whole panel. Keep focus on the Browse button instead (the
// QuickAddBox precedent).
const browseButtonRef = ref<InstanceType<typeof Button>>()
function onBrowserCloseAutoFocus(event: Event) {
  event.preventDefault()
  ;(browseButtonRef.value?.$el as HTMLElement | undefined)?.focus()
}
</script>

<template>
  <div class="space-y-2">
    <div class="flex items-center justify-between">
      <label :for="inputId" class="text-sm leading-none font-medium">Art tags</label>
      <Button
        ref="browseButtonRef"
        variant="ghost"
        size="sm"
        class="text-muted-foreground -mr-2 h-7 gap-1.5"
        @click="browserOpen = true"
      >
        <BookOpen class="size-3.5" aria-hidden="true" />
        Browse
      </Button>
    </div>

    <!-- One removable chip per active `art:` token. -->
    <div v-if="chips.length" class="flex flex-wrap gap-1.5">
      <span
        v-for="chip in chips"
        :key="chip"
        class="bg-secondary text-secondary-foreground inline-flex items-center gap-1 rounded-md py-0.5 pr-1 pl-2 text-xs"
      >
        {{ artTagSlug(chip) }}
        <button
          type="button"
          class="hover:bg-muted rounded p-0.5"
          :aria-label="`Remove art tag ${artTagSlug(chip)}`"
          @click="remove(chip)"
        >
          <X class="size-3" aria-hidden="true" />
        </button>
      </span>
    </div>

    <div class="relative">
      <Input
        :id="inputId"
        v-model="term"
        type="text"
        class="h-8"
        placeholder="e.g. squirrel"
        role="combobox"
        aria-autocomplete="list"
        autocomplete="off"
        :aria-expanded="showDropdown"
        :aria-controls="listboxId"
        :aria-activedescendant="activeDescendant"
        @keydown="onKeydown"
        @focus="onFocus"
        @blur="onBlur"
      />
      <div
        v-if="showDropdown"
        :id="listboxId"
        role="listbox"
        aria-label="Art tag suggestions"
        class="bg-popover text-popover-foreground absolute z-40 mt-1 max-h-56 w-full overflow-auto rounded-md border p-1 shadow-md"
      >
        <button
          v-for="(tag, index) in suggestions"
          :id="optionId(index)"
          :key="tag.slug"
          type="button"
          role="option"
          tabindex="-1"
          :aria-selected="index === activeIndex"
          class="flex w-full items-center gap-2 rounded-sm px-3 py-1.5 text-left text-sm outline-none transition-colors"
          :class="index === activeIndex ? 'bg-accent text-accent-foreground' : ''"
          @mousedown.prevent
          @mouseenter="activeIndex = index"
          @click="add(tag.slug)"
        >
          <span class="min-w-0 flex-1 truncate">{{ tag.label }}</span>
          <span class="text-muted-foreground text-xs tabular-nums">{{ tag.count }}</span>
        </button>
        <div v-if="!suggestions.length" class="text-muted-foreground px-3 py-2 text-sm">
          {{ isPending ? 'Searching…' : `No tags match “${trimmedTerm}”.` }}
        </div>
      </div>
    </div>

    <ArtTagBrowser
      v-model="query"
      v-model:open="browserOpen"
      :game="game"
      @close-auto-focus="onBrowserCloseAutoFocus"
    />
  </div>
</template>
