<script setup lang="ts">
import { computed, onUnmounted, ref, toRef, watch, type ComponentPublicInstance } from 'vue'
import { useId } from 'reka-ui'
import { Loader2, Search } from '@lucide/vue'
import { Input } from '@/components/ui/input'
import QuickAddPrintDialog from '@/components/collection/QuickAddPrintDialog.vue'
import QuickAddProductDialog from '@/components/collection/QuickAddProductDialog.vue'
import type { CardListTarget } from '@/composables/useOwnedCountEditor'
import {
  QUICK_ADD_MIN_CHARS,
  useCardNameSuggestions,
  useProductSuggestions,
} from '@/composables/useQuickAdd'
import { productTypeLabel } from '@/lib/productType'
import type { Product } from '@/lib/api'

// Step one of quick-add (mounted on the collection / wish-list landing while signed
// in): a text box that suggests options as you type, picking one opens a step-two
// dialog to confirm and add.
//
// `kind` selects what's being added (fixed for the component's lifetime, read once at
// setup — same rule as the dialog's `list`; precedent CollectionControls.vue:30-32):
//   - `'card'` (default): suggests distinct card names; picking one opens the print
//     picker (QuickAddPrintDialog) to choose a printing + regular/foil and add it — to
//     the collection by default, or the wish list when `list` says so (#167).
//   - `'product'` (#364): suggests sealed products; picking one opens
//     QuickAddProductDialog to set the wanted quantity. Sealed products are wish-list
//     only, so `list` is moot in this mode.
//
// A hand-rolled combobox (rather than reka's) so the list is driven purely by the
// server's suggestions with full control over the async/keyboard behaviour — options
// are chosen by mouse or keyboard and announced via the standard combobox/listbox ARIA
// roles. Both kinds normalise to one `QuickAddOption` shape so the listbox render, the
// keyboard handling, and the reset flow stay shared.
const props = withDefaults(
  defineProps<{ game: string; list?: CardListTarget; kind?: 'card' | 'product' }>(),
  { list: 'collection', kind: 'card' },
)
const game = toRef(props, 'game')
const listName = computed(() => (props.list === 'wishlist' ? 'wish list' : 'collection'))
const isProduct = props.kind === 'product'

// One normalized suggestion shape shared by both kinds: card mode yields
// `{ key, label }` (the name); product mode adds a `sublabel` (set · type) and the
// underlying `product` (handed to the step-two dialog).
interface QuickAddOption {
  key: string
  label: string
  sublabel?: string
  product?: Product
}

// `term` is the live input; `debouncedTerm` is what actually drives the query, so we
// don't fire a request on every keystroke.
const term = ref('')
const debouncedTerm = ref('')
const open = ref(false)
const activeIndex = ref(-1)

// The chosen leaf + whether the step-two dialog is open (one shared `dialogOpen`, one
// selection ref per kind).
const selectedName = ref<string | null>(null)
const selectedProduct = ref<Product | null>(null)
const dialogOpen = ref(false)

// The text input, so focus can be returned to it when the dialog closes.
const inputRef = ref<ComponentPublicInstance | null>(null)

// Pick the suggestion source once at setup (kind is fixed, so only the active query is
// ever created — the other stays untouched, firing no requests).
const cardQuery = isProduct ? undefined : useCardNameSuggestions(game, debouncedTerm)
const productQuery = isProduct ? useProductSuggestions(game, debouncedTerm) : undefined
const activeQuery = productQuery ?? cardQuery

const suggestions = computed<QuickAddOption[]>(() => {
  if (productQuery) {
    return (productQuery.data.value?.data ?? []).map((p) => ({
      key: p.id,
      label: p.name,
      sublabel: `${p.set_name ?? p.set_code.toUpperCase()} · ${productTypeLabel(p.product_type)}`,
      product: p,
    }))
  }
  return (cardQuery?.data.value?.data ?? []).map((name) => ({ key: name, label: name }))
})

const trimmedTerm = computed(() => term.value.trim())
// A query is "in flight" while the debounce hasn't caught up to the live term, or the
// request itself is running — used to show a spinner instead of a premature "no match".
const pending = computed(
  () =>
    trimmedTerm.value.length >= QUICK_ADD_MIN_CHARS &&
    ((activeQuery?.isFetching.value ?? false) || trimmedTerm.value !== debouncedTerm.value.trim()),
)
const showDropdown = computed(() => open.value && trimmedTerm.value.length >= QUICK_ADD_MIN_CHARS)

// Strings that vary by kind, computed once (from the fixed `kind`).
const placeholder = isProduct ? 'Quick add a sealed product by name…' : 'Quick add a card by name…'
const inputAriaLabel = computed(() =>
  isProduct
    ? 'Quick add a sealed product to your wish list'
    : `Quick add a card to your ${listName.value}`,
)
const listboxLabel = isProduct ? 'Sealed product suggestions' : 'Card name suggestions'
const emptyLabel = computed(() =>
  isProduct
    ? `No sealed products match “${trimmedTerm.value}”.`
    : `No cards match “${trimmedTerm.value}”.`,
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

function pick(option: QuickAddOption) {
  if (isProduct) {
    selectedProduct.value = option.product ?? null
  } else {
    selectedName.value = option.label
  }
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
      :placeholder="placeholder"
      :aria-label="inputAriaLabel"
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

    <!-- z-40: the landings mount this box above a `sticky z-30` filter bar
         (StickySearchBar), so the list must sit one layer higher or the bar
         paints over the suggestions as they extend down the page. -->
    <div
      v-if="showDropdown"
      :id="listboxId"
      role="listbox"
      :aria-label="listboxLabel"
      class="bg-popover text-popover-foreground absolute z-40 mt-1 max-h-72 w-full overflow-auto rounded-md border p-1 shadow-md"
    >
      <button
        v-for="(option, index) in suggestions"
        :id="optionId(index)"
        :key="option.key"
        type="button"
        role="option"
        tabindex="-1"
        :aria-selected="index === activeIndex"
        class="flex w-full items-center rounded-sm px-3 py-1.5 text-left text-sm outline-none transition-colors"
        :class="index === activeIndex ? 'bg-accent text-accent-foreground' : ''"
        @mousedown.prevent
        @mouseenter="activeIndex = index"
        @click="pick(option)"
      >
        <span class="min-w-0 flex-1">
          <span class="block truncate">{{ option.label }}</span>
          <span v-if="option.sublabel" class="text-muted-foreground block truncate text-xs">{{
            option.sublabel
          }}</span>
        </span>
      </button>

      <div
        v-if="!suggestions.length"
        class="text-muted-foreground flex items-center gap-2 px-3 py-2 text-sm"
      >
        <template v-if="pending">
          <Loader2 class="size-4 animate-spin" aria-hidden="true" />
          Searching…
        </template>
        <template v-else>{{ emptyLabel }}</template>
      </div>
    </div>

    <QuickAddPrintDialog
      v-if="kind === 'card'"
      v-model:open="dialogOpen"
      :game="game"
      :name="selectedName"
      :list="list"
      @close-auto-focus="onDialogCloseAutoFocus"
    />
    <QuickAddProductDialog
      v-else
      v-model:open="dialogOpen"
      :game="game"
      :product="selectedProduct"
      @close-auto-focus="onDialogCloseAutoFocus"
    />
  </div>
</template>
