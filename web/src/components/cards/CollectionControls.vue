<script setup lang="ts">
import { computed, onBeforeUnmount, ref, toRef, watch } from 'vue'
import { Check, Loader2, Minus, Plus } from '@lucide/vue'
import { RouterLink, useRoute } from 'vue-router'
import type { Card } from '@/lib/api'
import { Button } from '@/components/ui/button'
import { useCollectionEntryQuery, useSetCollectionEntryMutation } from '@/composables/useCollection'
import { useAuthStore } from '@/stores/auth'

// "How many do I own?" controls for the card-detail page. Reads the current holding
// and lets a signed-in user adjust the regular + foil counts; the route is public,
// so signed-out visitors instead get a sign-in nudge.
const props = defineProps<{ game: string; card: Card }>()
const auth = useAuthStore()
const route = useRoute()

const game = toRef(props, 'game')
const cardId = computed(() => props.card.id)

const entryQuery = useCollectionEntryQuery(game, cardId)
const mutation = useSetCollectionEntryMutation()

// Local, instantly-updated counts. `dirty` marks a local edit not yet reflected by
// the server, so a background refetch never clobbers an in-progress change.
const quantity = ref(0)
const foil = ref(0)
const dirty = ref(false)
const saveError = ref(false)

// Seed from the server holding whenever it (re)loads, unless a local edit is pending.
watch(
  () => entryQuery.data.value,
  (entry) => {
    if (entry && !dirty.value) {
      quantity.value = entry.quantity
      foil.value = entry.foil_quantity
    }
  },
  { immediate: true },
)

// Switching to a different card starts fresh.
watch(cardId, () => {
  dirty.value = false
  saveError.value = false
})

// Serialize + debounce saves: local state updates instantly, a trailing save fires
// after a short pause, and saves never overlap (each waits for the previous). An
// edit-generation counter keeps a late refetch from overwriting a newer local edit.
let timer: ReturnType<typeof setTimeout> | null = null
let inFlight: Promise<unknown> = Promise.resolve()
let editGen = 0

function runSave() {
  const gen = editGen
  return mutation
    .mutateAsync({
      game: game.value,
      id: cardId.value,
      quantity: quantity.value,
      foil_quantity: foil.value,
    })
    .then(() => {
      saveError.value = false
    })
    .catch(() => {
      saveError.value = true
    })
    .finally(() => {
      // Only clear the dirty flag if no further edit happened while this save ran,
      // so the pending edit's own save (and reseed) stays authoritative.
      if (gen === editGen) dirty.value = false
    })
}

function save() {
  inFlight = inFlight.then(runSave)
}

function scheduleSave() {
  dirty.value = true
  editGen += 1
  if (timer) clearTimeout(timer)
  timer = setTimeout(() => {
    timer = null
    save()
  }, 350)
}

onBeforeUnmount(() => {
  // Flush a pending edit so a quick navigation away doesn't drop the last change.
  if (timer) {
    clearTimeout(timer)
    timer = null
    save()
  }
})

function adjust(which: 'quantity' | 'foil', delta: number) {
  if (which === 'quantity') quantity.value = Math.max(0, quantity.value + delta)
  else foil.value = Math.max(0, foil.value + delta)
  scheduleSave()
}

// Disable the steppers until the initial holding has loaded, so an early click can't
// adjust off a stale zero.
const loading = computed(() => auth.isAuthenticated && entryQuery.isLoading.value)
const saving = computed(() => mutation.isPending.value || dirty.value)
const owned = computed(() => quantity.value + foil.value)
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
          { key: 'quantity' as const, label: 'Regular', value: quantity },
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
