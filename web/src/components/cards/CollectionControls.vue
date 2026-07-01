<script setup lang="ts">
import { computed, toRef } from 'vue'
import { Check, Loader2, Minus, Plus } from '@lucide/vue'
import { RouterLink, useRoute } from 'vue-router'
import type { Card } from '@/lib/api'
import { Button } from '@/components/ui/button'
import { useCollectionEntryQuery } from '@/composables/useCollection'
import { useOwnedCountEditor } from '@/composables/useOwnedCountEditor'
import { useAuthStore } from '@/stores/auth'

// "How many do I own?" controls for the card-detail page. Reads the current holding
// and lets a signed-in user adjust the regular + foil counts; the route is public,
// so signed-out visitors instead get a sign-in nudge. The debounced/serialized save
// loop lives in useOwnedCountEditor (shared with the grid quick-add control).
const props = defineProps<{ game: string; card: Card }>()
const auth = useAuthStore()
const route = useRoute()

const game = toRef(props, 'game')
const cardId = computed(() => props.card.id)

const entryQuery = useCollectionEntryQuery(game, cardId)
const seed = computed(() => entryQuery.data.value)
const { regular, foil, adjust, saving, saveError } = useOwnedCountEditor(game, cardId, seed)

// Disable the steppers until the initial holding has loaded, so an early click can't
// adjust off a stale zero.
const loading = computed(() => auth.isAuthenticated && entryQuery.isLoading.value)
const owned = computed(() => regular.value + foil.value)
const loginTo = computed(() => ({ path: '/login', query: { redirect: route.fullPath } }))
</script>

<template>
  <section class="mt-6 rounded-lg border p-4">
    <div class="mb-3 flex items-center justify-between gap-2">
      <h2 class="text-sm font-semibold">Your collection</h2>
      <!-- Save status (signed-in only). -->
      <span
        v-if="auth.isAuthenticated"
        class="text-muted-foreground flex items-center gap-1 text-xs"
        aria-live="polite"
      >
        <template v-if="saveError">
          <span class="text-destructive">Couldn't save — retry</span>
        </template>
        <template v-else-if="saving">
          <Loader2 class="size-3.5 animate-spin" />
          Saving…
        </template>
        <template v-else-if="owned > 0">
          <Check class="size-3.5" />
          Saved
        </template>
      </span>
    </div>

    <!-- Signed out: nudge to sign in (the route is public). -->
    <p v-if="!auth.isAuthenticated" class="text-muted-foreground text-sm">
      <RouterLink :to="loginTo" class="text-primary font-medium hover:underline"
        >Sign in</RouterLink
      >
      to track this card in your collection.
    </p>

    <div v-else class="space-y-3">
      <div
        v-for="row in [
          { key: 'quantity' as const, label: 'Regular', value: regular },
          { key: 'foil' as const, label: 'Foil', value: foil },
        ]"
        :key="row.key"
        class="flex items-center justify-between gap-3"
      >
        <span class="text-sm">{{ row.label }}</span>
        <div class="flex items-center gap-2">
          <Button
            variant="outline"
            size="icon-sm"
            :disabled="loading || row.value <= 0"
            :aria-label="`Remove one ${row.label.toLowerCase()} copy`"
            @click="adjust(row.key, -1)"
          >
            <Minus />
          </Button>
          <span class="w-8 text-center text-sm font-medium tabular-nums">{{ row.value }}</span>
          <Button
            variant="outline"
            size="icon-sm"
            :disabled="loading"
            :aria-label="`Add one ${row.label.toLowerCase()} copy`"
            @click="adjust(row.key, 1)"
          >
            <Plus />
          </Button>
        </div>
      </div>

      <p class="text-muted-foreground text-xs">
        <template v-if="owned > 0">
          You own {{ owned }} {{ owned === 1 ? 'copy' : 'copies' }}.
        </template>
        <template v-else> Not in your collection yet. </template>
      </p>
    </div>
  </section>
</template>
