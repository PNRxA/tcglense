<script setup lang="ts">
import { computed, onScopeDispose, ref, toRef, useId, watch } from 'vue'
import { Loader2, X } from '@lucide/vue'
import { Input } from '@/components/ui/input'
import { listCards } from '@/lib/api'
import { QUICK_ADD_MIN_CHARS, useCardNameSuggestions } from '@/composables/useQuickAdd'

// Name the commander a player brought, for the opponents you'll never have a deck for.
//
// A name box rather than a card picker on purpose: at the table you know the commander's name and
// nothing else, and choosing between eleven printings of it would be busywork — any printing
// identifies the card. So picking a name resolves to a printing behind the scenes and the field
// shows the resolved card name back.
//
// A hand-rolled combobox over the shared `useCardNameSuggestions` (the same server hints the
// quick-add box uses), for the same reason QuickAddBox hand-rolls one: full control over the
// async and keyboard behaviour, with the standard combobox/listbox roles for a screen reader.
const props = defineProps<{
  game: string
  /** The linked commander's external card id, or null. */
  modelValue: string | null
  /** The linked commander's name, so the field can show it without re-resolving. */
  name?: string | null
  label?: string
}>()

const emit = defineEmits<{ 'update:modelValue': [value: string | null]; picked: [name: string] }>()

const game = toRef(props, 'game')
const term = ref('')
const open = ref(false)
const active = ref(-1)
/** The name of whatever is currently linked — the server's on load, ours after a pick. */
const linkedName = ref<string | null>(props.name ?? null)
/**
 * The id this field resolved itself. The `name` prop comes from the seat row, which still holds
 * the *previous* commander until the seat is saved — so without this, replacing a commander would
 * show the old name against the new id.
 */
const pickedId = ref<string | null>(null)

watch(
  () => [props.modelValue, props.name] as const,
  ([id, name]) => {
    if (!id) {
      linkedName.value = null
      pickedId.value = null
      term.value = ''
      return
    }
    if (id === pickedId.value) return
    // A link that arrived from outside: show its name, or nothing — never the last one's.
    linkedName.value = name ?? null
  },
)

const { data, isFetching } = useCardNameSuggestions(game, term)
const suggestions = computed(() => data.value?.data ?? [])
const canSuggest = computed(() => term.value.trim().length >= QUICK_ADD_MIN_CHARS)

/** Resolve a chosen name to a printing, so the seat stores a real card id. */
const resolving = ref(false)
const resolveError = ref<string | null>(null)

async function pick(name: string) {
  resolving.value = true
  resolveError.value = null
  try {
    // Any printing identifies the card, so the first match is enough — and it's a cached
    // public read, so this costs nothing after the first time a name is picked.
    const page = await listCards(game.value, { q: name, pageSize: 1 })
    const card = page.data[0]
    if (!card) {
      resolveError.value = `Couldn't find a card called "${name}".`
      return
    }
    linkedName.value = card.name
    pickedId.value = card.id
    term.value = ''
    open.value = false
    active.value = -1
    emit('update:modelValue', card.id)
    emit('picked', card.name)
  } catch {
    // The name came from the server's own hints, so a failure here is the network, not the input.
    resolveError.value = "Couldn't look that card up. Please retry."
  } finally {
    resolving.value = false
  }
}

/** Typing reopens the list and drops the highlight — a named handler, because a Vue template
 * expression must be a single expression and two statements in an inline handler is a parse
 * error the type-checker doesn't catch. */
function onInput() {
  open.value = true
  active.value = -1
}

function clear() {
  linkedName.value = null
  term.value = ''
  open.value = false
  resolveError.value = null
  emit('update:modelValue', null)
}

// Stable ids for the combobox/listbox ARIA wiring — the same shape `QuickAddBox` uses, so the
// highlighted option is announced rather than only styled.
const baseId = useId()
const listboxId = `${baseId}-listbox`
function optionId(index: number): string {
  return `${baseId}-option-${index}`
}
const activeDescendant = computed(() =>
  open.value && active.value >= 0 ? optionId(active.value) : undefined,
)

let blurTimer: ReturnType<typeof setTimeout> | undefined
function onBlur() {
  // Delay closing so an option's mousedown→click lands first (the option prevents its own
  // mousedown default, so a click keeps focus, but a click elsewhere closes here). Without this
  // the list stays open over whatever is beneath it once focus moves on.
  blurTimer = setTimeout(() => {
    open.value = false
  }, 120)
}
onScopeDispose(() => clearTimeout(blurTimer))

function onKeydown(event: KeyboardEvent) {
  if (!open.value || !suggestions.value.length) return
  if (event.key === 'ArrowDown') {
    event.preventDefault()
    active.value = (active.value + 1) % suggestions.value.length
  } else if (event.key === 'ArrowUp') {
    event.preventDefault()
    active.value = active.value <= 0 ? suggestions.value.length - 1 : active.value - 1
  } else if (event.key === 'Enter') {
    const name = suggestions.value[active.value] ?? suggestions.value[0]
    if (name) {
      event.preventDefault()
      void pick(name)
    }
  } else if (event.key === 'Escape') {
    open.value = false
  }
}
</script>

<template>
  <div class="relative">
    <!-- Linked: show what it resolved to, with a way out. -->
    <div
      v-if="modelValue && linkedName"
      class="bg-muted/40 flex h-9 items-center gap-2 rounded-md border px-3"
    >
      <span class="min-w-0 flex-1 truncate text-sm">{{ linkedName }}</span>
      <button
        type="button"
        class="text-muted-foreground hover:text-foreground shrink-0"
        :aria-label="`Unlink ${linkedName}`"
        @click="clear"
      >
        <X class="size-4" aria-hidden="true" />
      </button>
    </div>

    <template v-else>
      <Input
        v-model="term"
        :aria-label="label ?? 'Commander'"
        placeholder="Commander name…"
        role="combobox"
        :aria-expanded="open && suggestions.length > 0"
        :aria-controls="listboxId"
        :aria-activedescendant="activeDescendant"
        aria-autocomplete="list"
        @focus="open = true"
        @input="onInput"
        @keydown="onKeydown"
        @blur="onBlur"
      />
      <Loader2
        v-if="isFetching || resolving"
        class="text-muted-foreground absolute top-2.5 right-3 size-4 animate-spin"
        aria-hidden="true"
      />
      <ul
        v-if="open && canSuggest && suggestions.length"
        :id="listboxId"
        role="listbox"
        :aria-label="label ?? 'Commander suggestions'"
        class="bg-popover absolute z-20 mt-1 max-h-56 w-full overflow-y-auto rounded-md border p-1 shadow-md"
      >
        <li v-for="(name, index) in suggestions" :key="name">
          <button
            :id="optionId(index)"
            type="button"
            role="option"
            tabindex="-1"
            :aria-selected="index === active"
            class="hover:bg-accent w-full truncate rounded px-2 py-1.5 text-left text-sm"
            :class="index === active ? 'bg-accent' : ''"
            @mousedown.prevent="pick(name)"
          >
            {{ name }}
          </button>
        </li>
      </ul>
      <p v-if="resolveError" class="text-destructive mt-1 text-xs">{{ resolveError }}</p>
    </template>
  </div>
</template>
