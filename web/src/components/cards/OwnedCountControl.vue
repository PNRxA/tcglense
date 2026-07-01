<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { Check, Loader2, Minus, Plus, Sparkles } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import OwnedCountBadge from '@/components/cards/OwnedCountBadge.vue'
import { useCollectionEntryQuery } from '@/composables/useCollection'
import { useOwnedCountEditor, type OwnedCountSeed } from '@/composables/useOwnedCountEditor'

// Quick-add control overlaid on a card tile (issue #95): a corner trigger showing the
// owned count (or a "+" on a card you don't own yet) that opens a small popover with
// regular/foil steppers, so a signed-in user can add to or adjust their collection
// without leaving the grid. Rendered by CardGrid / CollectionGrid only while signed in.
//
// `quantity`/`foilQuantity` are the *display* counts from the grid's ownership source —
// good enough for the resting badge, but they can lag (a browse grid loads them async).
// Because the save writes ABSOLUTE counts, the editor is instead seeded from the
// authoritative single-card holding fetched when the popover opens, and the steppers stay
// disabled until it resolves — so an early click can never clobber the real count with a
// stale zero.
const props = defineProps<{
  game: string
  cardId: string
  name: string
  quantity: number
  foilQuantity: number
}>()

const open = ref(false)
const game = toRef(props, 'game')
const cardId = toRef(props, 'cardId')

// Fetch the authoritative holding only once the popover is open (a big grid must not fire
// one request per tile). `staleTime: 0` forces a re-fetch every time it re-opens, and
// `ready` waits for that fetch to settle (not just any prior success) — otherwise a reopen
// could seed the steppers off a stale cached count and an absolute-count save would clobber
// the true value. Until ready, seed the display from the grid counts so an owned card
// doesn't flash "0"; the steppers stay disabled, so acting on the fallback is impossible.
const entryQuery = useCollectionEntryQuery(game, cardId, { enabled: open, staleTime: 0 })
const seed = computed<OwnedCountSeed>(
  () => entryQuery.data.value ?? { quantity: props.quantity, foil_quantity: props.foilQuantity },
)
const ready = computed(() => entryQuery.isSuccess.value && !entryQuery.isFetching.value)

const { regular, foil, adjust, saving, saveError } = useOwnedCountEditor(game, cardId, seed)

// Resting trigger reflects the grid counts; the live edited counts show inside the panel.
const displayTotal = computed(() => props.quantity + props.foilQuantity)
const owned = computed(() => displayTotal.value > 0)
const editorTotal = computed(() => regular.value + foil.value)

const rows = computed(() => [
  { key: 'quantity' as const, label: 'Regular', value: regular.value, icon: null },
  { key: 'foil' as const, label: 'Foil', value: foil.value, icon: Sparkles },
])
</script>

<template>
  <Popover v-model:open="open">
    <PopoverTrigger as-child>
      <!-- Sibling of CardTile's stretched nav link (not nested inside it), so clicking it
        opens the popover instead of navigating; `.stop` is belt-and-braces. Anchored
        bottom-left to match the owned-count badge placement (issue #100). On a card you
        already own the count chip is always shown; on an unowned card the "+" is revealed
        on hover/focus (and always on touch) to keep a dense grid clean. -->
      <button
        type="button"
        class="absolute bottom-1.5 left-1.5 z-20 inline-flex items-center rounded-md outline-none transition focus-visible:ring-2 focus-visible:ring-ring"
        :class="
          owned
            ? ''
            : 'opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 focus-visible:opacity-100 [@media(hover:none)]:opacity-100'
        "
        :aria-label="
          owned ? `Edit copies of ${name} in your collection` : `Add ${name} to your collection`
        "
        @click.stop
      >
        <OwnedCountBadge
          v-if="owned"
          :quantity="quantity"
          :foil-quantity="foilQuantity"
          :tooltip="false"
        />
        <span
          v-else
          class="bg-primary/90 text-primary-foreground inline-flex items-center justify-center rounded-md p-1 shadow"
        >
          <Plus class="size-3.5" aria-hidden="true" />
        </span>
      </button>
    </PopoverTrigger>

    <!-- Opens above the bottom-left trigger (over the card art) so it doesn't cover the
      name/price below; reka flips it if there isn't room above. -->
    <PopoverContent side="top" align="start" :side-offset="6" class="w-56 p-3">
      <div class="mb-3 flex items-center justify-between gap-2">
        <p class="truncate text-sm font-medium" :title="name">{{ name }}</p>
        <span
          class="text-muted-foreground flex shrink-0 items-center gap-1 text-xs"
          aria-live="polite"
        >
          <template v-if="saveError">
            <span class="text-destructive">Retry</span>
          </template>
          <template v-else-if="saving">
            <Loader2 class="size-3.5 animate-spin" aria-hidden="true" />
            Saving…
          </template>
          <template v-else-if="editorTotal > 0">
            <Check class="size-3.5" aria-hidden="true" />
            Saved
          </template>
        </span>
      </div>

      <div class="space-y-2">
        <div v-for="row in rows" :key="row.key" class="flex items-center justify-between gap-3">
          <span class="flex items-center gap-1.5 text-sm">
            <component :is="row.icon" v-if="row.icon" class="size-3.5" aria-hidden="true" />
            {{ row.label }}
          </span>
          <div class="flex items-center gap-2">
            <!-- At 0 the minus is inert but stays focusable (aria-disabled + a click that
              no-ops), not natively `disabled` — so decrementing the last copy while
              keyboard-focused here doesn't drop focus out of the non-modal popover. -->
            <Button
              variant="outline"
              size="icon-sm"
              :disabled="!ready"
              :aria-disabled="row.value <= 0"
              :class="{ 'pointer-events-none opacity-50': row.value <= 0 }"
              :aria-label="`Remove one ${row.label.toLowerCase()} copy of ${name}`"
              @click="adjust(row.key, -1)"
            >
              <Minus />
            </Button>
            <span
              class="w-8 text-center text-sm font-medium tabular-nums"
              aria-live="polite"
              aria-atomic="true"
              :aria-label="`${row.label}: ${row.value}`"
              >{{ row.value }}</span
            >
            <Button
              variant="outline"
              size="icon-sm"
              :disabled="!ready"
              :aria-label="`Add one ${row.label.toLowerCase()} copy of ${name}`"
              @click="adjust(row.key, 1)"
            >
              <Plus />
            </Button>
          </div>
        </div>
      </div>
    </PopoverContent>
  </Popover>
</template>
