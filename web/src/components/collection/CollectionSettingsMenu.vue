<script setup lang="ts">
import { computed, toRef } from 'vue'
import { Settings2 } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Switch } from '@/components/ui/switch'
import CollectionVisibilityControl from '@/components/collection/CollectionVisibilityControl.vue'
import {
  useCollectionVisibilityQuery,
  useSetCollectionVisibilityMutation,
} from '@/composables/useCollectionVisibility'

// The per-game collection landing's settings menu (issue #381): a gear button opening a
// popover that gathers the sharing control (public/private + share link) and the display
// toggles for the value-over-time chart and the biggest-movers panel. All three persist
// server-side on the same per-(user, game) row, so the choices follow the collector across
// devices. Each row's descriptive text is wired to its switch via `for`/`id` so a click
// anywhere on the row flips it; toggles are optimistic (see the mutation), so they feel
// instant. Both default to shown while the prefs load or when there's no row yet.
const props = defineProps<{ game: string }>()
const game = toRef(props, 'game')

const visibilityQuery = useCollectionVisibilityQuery(game)
const setVisibility = useSetCollectionVisibilityMutation()

const showValue = computed(() => visibilityQuery.data.value?.show_value_chart ?? true)
const showMovers = computed(() => visibilityQuery.data.value?.show_movers ?? true)

function setShowValue(next: boolean) {
  setVisibility.mutate({ game: game.value, patch: { show_value_chart: next } })
}

function setShowMovers(next: boolean) {
  setVisibility.mutate({ game: game.value, patch: { show_movers: next } })
}
</script>

<template>
  <Popover>
    <PopoverTrigger as-child>
      <Button variant="outline" size="sm" class="shrink-0">
        <Settings2 />
        Settings
      </Button>
    </PopoverTrigger>
    <PopoverContent align="end" class="w-80 space-y-4">
      <section class="space-y-2">
        <h3 class="text-muted-foreground text-xs font-medium tracking-wide uppercase">Sharing</h3>
        <CollectionVisibilityControl :game="game" />
      </section>

      <div class="border-t" />

      <section class="space-y-3">
        <h3 class="text-muted-foreground text-xs font-medium tracking-wide uppercase">Sections</h3>
        <div class="flex items-start justify-between gap-3">
          <label for="collection-show-value" class="cursor-pointer space-y-0.5">
            <span class="block text-sm font-medium">Value over time</span>
            <span class="text-muted-foreground block text-xs">
              The collection's total value chart.
            </span>
          </label>
          <!-- No disabled-while-saving: the toggle is optimistic (and rolled back on
               error), so it stays responsive and the two prefs never block each other. -->
          <Switch
            id="collection-show-value"
            :checked="showValue"
            aria-label="Value over time"
            @update:checked="setShowValue"
          />
        </div>
        <div class="flex items-start justify-between gap-3">
          <label for="collection-show-movers" class="cursor-pointer space-y-0.5">
            <span class="block text-sm font-medium">Biggest movers</span>
            <span class="text-muted-foreground block text-xs">The largest gainers and losers.</span>
          </label>
          <Switch
            id="collection-show-movers"
            :checked="showMovers"
            aria-label="Biggest movers"
            @update:checked="setShowMovers"
          />
        </div>
      </section>
    </PopoverContent>
  </Popover>
</template>
