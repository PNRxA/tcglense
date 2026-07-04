<script setup lang="ts">
import { computed } from 'vue'
import { Contrast, Ghost, Palette, SlidersHorizontal } from '@lucide/vue'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import type { CardListTarget } from '@/composables/useOwnedCountEditor'
import { isGhostStyle } from '@/lib/ghostDisplay'
import { useGhostDisplayStore } from '@/stores/ghostDisplay'
import { cn } from '@/lib/utils'

// The show-ghosts toggle in the collection / wish-list browse toolbars (issue #112), plus a
// display-settings dropdown attached to it (issue #213). Extracted from the two browse views
// (which used to each inline the button) so the new dropdown lives in one place. `list` picks
// the wording and, for the wish list, adds the "Show owned (in collection)" option; both
// settings come from the shared, persisted ghostDisplay store.
const props = withDefaults(defineProps<{ showGhosts: boolean; list?: CardListTarget }>(), {
  list: 'collection',
})
const emit = defineEmits<{ toggle: [value: boolean] }>()

const ghost = useGhostDisplayStore()

const toggleTitle = computed(() =>
  props.list === 'wishlist'
    ? 'Also show cards not on your wish list, dimmed, to see what you could add'
    : "Also show cards you don't own, dimmed, to see the gaps",
)

// The settings caret opens the display dropdown. It's shown whenever a setting applies: the
// ghost-colour choice (only meaningful while ghosts are shown) or, on the wish list, the
// always-relevant "Show owned" toggle — whose markers render in every wish-list browse mode,
// so its control must stay reachable even with ghosts off (else it's an undiscoverable
// dead-end). So on the wish list the caret is always present; on the collection it appears
// only with ghosts (the colour choice is its only setting).
const showOwnedOption = computed(() => props.list === 'wishlist')
const caretVisible = computed(() => props.showGhosts || showOwnedOption.value)

function onSelectStyle(value: string | undefined) {
  if (isGhostStyle(value)) ghost.setStyle(value)
}
</script>

<template>
  <!-- A split control: the "Show ghosts" toggle on the left; a settings caret on the right
       (whenever a display setting applies) opens the dropdown. The border + active fill wrap
       both so they read as one pill (matching the original standalone button when the caret
       is hidden). -->
  <div
    :class="
      cn(
        'inline-flex items-center rounded-md border text-sm font-medium transition-colors',
        showGhosts ? 'border-primary bg-primary/10 text-foreground' : 'text-muted-foreground',
      )
    "
  >
    <button
      type="button"
      :class="
        cn(
          'inline-flex items-center gap-1.5 px-3 py-1.5 transition-colors',
          caretVisible ? 'rounded-l-md' : 'rounded-md',
          showGhosts ? '' : 'hover:text-foreground',
        )
      "
      :aria-pressed="showGhosts"
      :title="toggleTitle"
      @click="emit('toggle', !showGhosts)"
    >
      <Ghost class="size-4" aria-hidden="true" />
      Show ghosts
    </button>

    <template v-if="caretVisible">
      <span
        :class="cn('h-5 w-px', showGhosts ? 'bg-primary/30' : 'bg-border')"
        aria-hidden="true"
      />
      <DropdownMenu>
        <DropdownMenuTrigger as-child>
          <button
            type="button"
            :class="
              cn(
                'focus-visible:ring-ring inline-flex items-center rounded-r-md px-2 py-1.5 outline-none transition-colors focus-visible:ring-2',
                showGhosts ? 'hover:bg-primary/10' : 'hover:text-foreground',
              )
            "
            title="Display settings"
          >
            <SlidersHorizontal class="size-4" aria-hidden="true" />
            <span class="sr-only">Display settings</span>
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" class="w-56">
          <DropdownMenuLabel>Display</DropdownMenuLabel>
          <DropdownMenuSeparator />

          <!-- Ghost colour: only meaningful while ghosts are shown. -->
          <DropdownMenuRadioGroup
            v-if="showGhosts"
            :model-value="ghost.style"
            @update:model-value="onSelectStyle"
          >
            <DropdownMenuRadioItem value="grayscale">
              <Contrast aria-hidden="true" />
              Grayscale
            </DropdownMenuRadioItem>
            <DropdownMenuRadioItem value="color">
              <Palette aria-hidden="true" />
              Full colour
            </DropdownMenuRadioItem>
          </DropdownMenuRadioGroup>

          <!-- Wish list only: flag cards already in the collection while shopping the list.
               Always available here (its markers act in every browse mode), so the caret
               stays reachable with ghosts off. `@select.prevent` keeps the menu open so the
               toggle can be flipped in place. -->
          <template v-if="showOwnedOption">
            <DropdownMenuSeparator v-if="showGhosts" />
            <DropdownMenuCheckboxItem
              :model-value="ghost.showOwned"
              @update:model-value="ghost.setShowOwned"
              @select="(event: Event) => event.preventDefault()"
            >
              Show owned (in collection)
            </DropdownMenuCheckboxItem>
          </template>
        </DropdownMenuContent>
      </DropdownMenu>
    </template>
  </div>
</template>
