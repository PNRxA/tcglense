<script setup lang="ts">
import { computed, onUnmounted, ref, toRef, watch, type ComponentPublicInstance } from 'vue'
import { useId } from 'reka-ui'
import { Loader2, Search } from '@lucide/vue'
import { Input } from '@/components/ui/input'
import QuickAddPrintDialog from '@/components/collection/QuickAddPrintDialog.vue'
import type { CardListTarget } from '@/composables/useOwnedCountEditor'
import { QUICK_ADD_MIN_CHARS, useCardNameSuggestions } from '@/composables/useQuickAdd'

// Step one of quick-add (mounted on the collection / wish-list landing while signed
// in): a text box that suggests distinct card names as you type. Picking a name opens
// the print picker (QuickAddPrintDialog) to choose a printing + regular/foil and add
// it — to the collection by default, or to the wish list when `list` says so (#167).
//
// A hand-rolled combobox (rather than reka's) so the list is driven purely by the
// server's already-unique name suggestions with full control over the async/keyboard
// behaviour — options are chosen by mouse or keyboard and announced via the standard
// combobox/listbox ARIA roles.
const props = withDefaults(defineProps<{ game: string; list?: CardListTarget }>(), {
  list: 'collection',
})
const game = toRef(props, 'game')
const listName = computed(() => (props.list === 'wishlist' ? 'wish list' : 'collection'))

// `term` is the live input; `debouncedTerm` is what actually drives the query, so we
// don't fire a request on every keystroke.
const term = ref('')
const debouncedTerm = ref('')
const open = ref(false)
const activeIndex = ref(-1)

// The chosen name + whether the print picker is open.
const selectedName = ref<string | null>(null)
const dialogOpen = ref(false)

// The text input, so focus can be returned to it when the print dialog closes.
const inputRef = ref<ComponentPublicInstance | null>(null)

const suggestQuery = useCardNameSuggestions(game, debouncedTerm)
const suggestions = computed(() => suggestQuery.data.value?.data ?? [])

const trimmedTerm = computed(() => term.value.trim())
// A query is "in flight" while the debounce hasn't caught up to the live term, or the
// request itself is running — used to show a spinner instead of a premature "no match".
const pending = computed(
  () =>
    trimmedTerm.value.length >= QUICK_ADD_MIN_CHARS &&
    (suggestQuery.isFetching.value || trimmedTerm.value !== debouncedTerm.value.trim()),
)
const showDropdown = computed(() => open.value && trimmedTerm.value.length >= QUICK_ADD_MIN_CHARS)

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
  // Typing (re)opens the dropdown once the term is long enough.
  if (value.trim().length >= QUICK_ADD_MIN_CHARS) open.value = true
})

// A fresh suggestion set invalidates the old highlight (its index may not exist now).
watch(suggestions, () => {
  activeIndex.value = -1
})

onUnmounted(() => {
  clearTimeout(debounceTimer)
  clearTimeout(blurTimer)
})

function onFocus() {
  clearTimeout(blurTimer)
  if (trimmedTerm.value.length >= QUICK_ADD_MIN_CHARS) open.value = true
}

function onBlur() {
  // Delay closing so an option's mousedown→click lands first (the option prevents its
  // own mousedown default, so a click keeps focus, but a click elsewhere closes here).
  blurTimer = setTimeout(() => {
    open.value = false
  }, 120)
}

function close() {
  open.value = false
  activeIndex.value = -1
}

function pick(name: string) {
  selectedName.value = name
  dialogOpen.value = true
  // Reset the box so the next quick-add starts clean, and hide the dropdown.
  clearTimeout(debounceTimer)
  term.value = ''
  debouncedTerm.value = ''
  close()
}

// The dialog is opened programmatically (no trigger), so reka would otherwise drop
// focus to <body> on close — preventing its default and refocusing the box keeps the
// keyboard flow intact (and ready for the next quick-add).
function onDialogCloseAutoFocus(event: Event) {
  event.preventDefault()
  ;(inputRef.value?.$el as HTMLElement | undefined)?.focus()
}

function onKeydown(event: KeyboardEvent) {
  switch (event.key) {
    case 'ArrowDown':
      event.preventDefault()
      if (!showDropdown.value) {
        open.value = true
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
      if (showDropdown.value && activeIndex.value >= 0 && choice) {
        event.preventDefault()
        pick(choice)
      }
      break
    }
    case 'Escape':
      if (showDropdown.value) {
        event.preventDefault()
        close()
      }
      break
  }
}
</script>

<template>
  <div class="relative">
    <Search
      class="text-muted-foreground pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2"
    />
    <Input
      ref="inputRef"
      v-model="term"
      class="pl-9"
      placeholder="Quick add a card by name…"
      :aria-label="`Quick add a card to your ${listName}`"
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
    <Loader2
      v-if="showDropdown && pending"
      class="text-muted-foreground absolute top-1/2 right-3 size-4 -translate-y-1/2 animate-spin"
      aria-hidden="true"
    />

    <div
      v-if="showDropdown"
      :id="listboxId"
      role="listbox"
      aria-label="Card name suggestions"
      class="bg-popover text-popover-foreground absolute z-30 mt-1 max-h-72 w-full overflow-auto rounded-md border p-1 shadow-md"
    >
      <button
        v-for="(name, index) in suggestions"
        :id="optionId(index)"
        :key="name"
        type="button"
        role="option"
        tabindex="-1"
        :aria-selected="index === activeIndex"
        class="flex w-full items-center rounded-sm px-3 py-1.5 text-left text-sm outline-none transition-colors"
        :class="index === activeIndex ? 'bg-accent text-accent-foreground' : ''"
        @mousedown.prevent
        @mouseenter="activeIndex = index"
        @click="pick(name)"
      >
        <span class="truncate">{{ name }}</span>
      </button>

      <div
        v-if="!suggestions.length"
        class="text-muted-foreground flex items-center gap-2 px-3 py-2 text-sm"
      >
        <template v-if="pending">
          <Loader2 class="size-4 animate-spin" aria-hidden="true" />
          Searching…
        </template>
        <template v-else> No cards match “{{ trimmedTerm }}”. </template>
      </div>
    </div>

    <QuickAddPrintDialog
      v-model:open="dialogOpen"
      :game="game"
      :name="selectedName"
      :list="list"
      @close-auto-focus="onDialogCloseAutoFocus"
    />
  </div>
</template>
