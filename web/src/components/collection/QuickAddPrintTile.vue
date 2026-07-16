<script setup lang="ts">
import { computed, toRef } from 'vue'
import { Check, Loader2, Minus, Plus, Sparkles } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import PrintingTile from '@/components/printings/PrintingTile.vue'
import {
  useOwnedCountEditor,
  type CardListTarget,
  type OwnedCountSeed,
} from '@/composables/useOwnedCountEditor'
import type { Card } from '@/lib/api'

// One printing in the quick-add print picker, rendered as a tall tile so the artwork
// is large enough to actually tell printings apart: full-size card image on top, then
// set/number/rarity/price, then regular/foil steppers. Reuses the same
// `useOwnedCountEditor` the card-detail and grid controls use, so the tricky bits
// (debounced + serialized absolute-count saves, dirty guarding) are shared. `list`
// retargets the saves at the wish list (issue #167).
//
// Writes are ABSOLUTE, so the editor must be seeded from the authoritative holding
// before it can be trusted: the parent dialog fetches counts for every loaded printing
// fresh on open and passes each tile its `seed` (once `ready`). Until then the steppers
// stay disabled, so a click can never save an adjustment off a stale zero.
const props = withDefaults(
  defineProps<{
    game: string
    card: Card
    /** Authoritative counts for this printing, or `undefined` until they load. */
    seed: OwnedCountSeed | undefined
    /** Whether `seed` reflects the current server holding (gates the steppers). */
    ready: boolean
    list?: CardListTarget
  }>(),
  { list: 'collection' },
)

const game = toRef(props, 'game')
const cardId = computed(() => props.card.id)
const seed = toRef(props, 'seed')

const { regular, foil, adjust, saving, saveError } = useOwnedCountEditor(game, cardId, seed, {
  list: props.list,
})

const owned = computed(() => regular.value + foil.value > 0)

const rows = computed(() => [
  { key: 'quantity' as const, label: 'Regular', value: regular.value, icon: null },
  { key: 'foil' as const, label: 'Foil', value: foil.value, icon: Sparkles },
])
</script>

<template>
  <PrintingTile :game="game" :card="card">
    <template #actions>
      <!-- Regular / foil steppers stay in this collection/wish-list adapter. Writes are
        disabled until the authoritative seed for the loaded printing set has settled. -->
      <div class="mt-0.5 space-y-1.5 px-0.5 pb-0.5">
        <div
          v-for="row in rows"
          :key="row.key"
          class="flex flex-wrap items-center justify-between gap-x-2 gap-y-1"
        >
          <span class="text-muted-foreground flex items-center gap-1 text-xs">
            <component :is="row.icon" v-if="row.icon" class="size-3" aria-hidden="true" />
            {{ row.label }}
          </span>
          <div class="flex items-center gap-1.5">
            <Button
              variant="outline"
              size="icon"
              class="size-7"
              :disabled="!ready"
              :aria-disabled="row.value <= 0"
              :class="{ 'pointer-events-none opacity-50': row.value <= 0 }"
              :aria-label="`Remove one ${row.label.toLowerCase()} copy of ${card.name} (${card.set_name})`"
              @click="adjust(row.key, -1)"
            >
              <Minus />
            </Button>
            <span
              class="w-6 text-center text-sm font-medium tabular-nums"
              aria-live="polite"
              aria-atomic="true"
              :aria-label="`${row.label}: ${row.value}`"
              >{{ row.value }}</span
            >
            <Button
              variant="outline"
              size="icon"
              class="size-7"
              :disabled="!ready"
              :aria-label="`Add one ${row.label.toLowerCase()} copy of ${card.name} (${card.set_name})`"
              @click="adjust(row.key, 1)"
            >
              <Plus />
            </Button>
          </div>
        </div>

        <!-- Save status (fixed height so it never shifts the tile as it changes). -->
        <div class="text-muted-foreground flex h-4 items-center gap-1 text-xs" aria-live="polite">
          <template v-if="saveError">
            <span class="text-destructive">Retry — not saved</span>
          </template>
          <template v-else-if="saving">
            <Loader2 class="size-3.5 animate-spin" aria-hidden="true" />
            Saving…
          </template>
          <template v-else-if="owned">
            <Check class="size-3.5" aria-hidden="true" />
            Saved
          </template>
        </div>
      </div>
    </template>
  </PrintingTile>
</template>
