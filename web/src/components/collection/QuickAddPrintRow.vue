<script setup lang="ts">
import { computed, toRef } from 'vue'
import { Check, Loader2, Minus, Plus, Sparkles } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import CardImage from '@/components/cards/CardImage.vue'
import { useOwnedCountEditor, type OwnedCountSeed } from '@/composables/useOwnedCountEditor'
import { displayUsdPrice } from '@/lib/cardPrice'
import type { Card } from '@/lib/api'

// One printing in the quick-add print picker: a thumbnail + set/number/price, and
// regular/foil steppers to add it to (or adjust it in) the collection. Reuses the
// same `useOwnedCountEditor` the card-detail and grid controls use, so the tricky
// bits (debounced + serialized absolute-count saves, dirty guarding) are shared.
//
// Writes are ABSOLUTE, so the editor must be seeded from the authoritative holding
// before it can be trusted: the parent dialog fetches every printing's owned counts
// fresh on open and passes each row its `seed` (once `ready`). Until then the
// steppers stay disabled, so a click can never save an adjustment off a stale zero.
const props = defineProps<{
  game: string
  card: Card
  /** Authoritative owned counts for this printing, or `undefined` until they load. */
  seed: OwnedCountSeed | undefined
  /** Whether `seed` reflects the current server holding (gates the steppers). */
  ready: boolean
}>()

const game = toRef(props, 'game')
const cardId = computed(() => props.card.id)
const seed = toRef(props, 'seed')

const { regular, foil, adjust, saving, saveError } = useOwnedCountEditor(game, cardId, seed)

const price = computed(() => displayUsdPrice(props.card.prices))
const owned = computed(() => regular.value + foil.value > 0)

const rows = computed(() => [
  { key: 'quantity' as const, label: 'Regular', value: regular.value, icon: null },
  { key: 'foil' as const, label: 'Foil', value: foil.value, icon: Sparkles },
])
</script>

<template>
  <div class="flex flex-wrap items-center gap-x-4 gap-y-3">
    <!-- Thumbnail + printing identity: the art is the quickest way to tell printings
      apart when several share a name. -->
    <div class="flex min-w-0 flex-1 items-center gap-3">
      <CardImage
        :game="game"
        :id="card.id"
        :name="card.name"
        :has-image="card.has_image"
        size="small"
        class="w-10 shrink-0 rounded-sm"
      />
      <div class="min-w-0">
        <p class="truncate text-sm font-medium" :title="card.set_name">
          {{ card.set_name }}
        </p>
        <p class="text-muted-foreground flex items-center gap-1.5 text-xs">
          <span>{{ card.set_code.toUpperCase() }} · #{{ card.collector_number }}</span>
          <span v-if="card.rarity" class="capitalize">· {{ card.rarity }}</span>
          <span v-if="price" class="tabular-nums"
            >· ${{ price.amount
            }}<span v-if="price.foil" class="ml-0.5 uppercase opacity-70">foil</span></span
          >
        </p>
      </div>
    </div>

    <!-- Regular / foil steppers. Disabled until the authoritative seed loads. -->
    <div class="flex items-center gap-4">
      <div v-for="row in rows" :key="row.key" class="flex items-center gap-1.5">
        <span class="text-muted-foreground flex items-center gap-1 text-xs">
          <component :is="row.icon" v-if="row.icon" class="size-3" aria-hidden="true" />
          {{ row.label }}
        </span>
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

      <!-- Save status, mirroring the grid quick-add control. -->
      <span
        class="text-muted-foreground flex w-14 shrink-0 items-center gap-1 text-xs"
        aria-live="polite"
      >
        <template v-if="saveError">
          <span class="text-destructive">Retry</span>
        </template>
        <template v-else-if="saving">
          <Loader2 class="size-3.5 animate-spin" aria-hidden="true" />
          Saving…
        </template>
        <template v-else-if="owned">
          <Check class="size-3.5" aria-hidden="true" />
          Saved
        </template>
      </span>
    </div>
  </div>
</template>
