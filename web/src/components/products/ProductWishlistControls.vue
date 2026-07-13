<script setup lang="ts">
import { computed, toRef } from 'vue'
import { Check, Loader2, Minus, Plus } from '@lucide/vue'
import { RouterLink, useRoute } from 'vue-router'
import type { Product } from '@/lib/api'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { useWishlistProductEntryQuery } from '@/composables/useWishlist'
import { useOwnedCountEditor } from '@/composables/useOwnedCountEditor'
import { useAuthStore } from '@/stores/auth'

// "How many do I want?" control for the sealed-product detail page — the "regular add" of
// issue #364. Reads the current wanted count and lets a signed-in user adjust it; the route
// is public, so signed-out visitors get a sign-in nudge instead. The debounced/serialized
// save loop is the same useOwnedCountEditor shared with the collection/wish-list card
// controls, here in `kind: 'product'` mode (writes to the wish list's sealed-product
// holding). Sealed products are wishlist-only — there's no collection counterpart — so
// there's no `list` prop and just one "Quantity" row (foil stays 0).
const props = defineProps<{ game: string; product: Product }>()
const auth = useAuthStore()
const route = useRoute()

const game = toRef(props, 'game')
const productId = computed(() => props.product.id)

const entryQuery = useWishlistProductEntryQuery(game, productId)
const seed = computed(() => entryQuery.data.value)
const { regular, adjust, saving, saveError } = useOwnedCountEditor(game, productId, seed, {
  kind: 'product',
})

// Disable the steppers until the initial holding has loaded, so an early click can't
// adjust off a stale zero.
const loading = computed(() => auth.isAuthenticated && entryQuery.isLoading.value)
const loginTo = computed(() => ({ path: '/login', query: { redirect: route.fullPath } }))
</script>

<template>
  <section class="mt-6 rounded-lg border p-4">
    <div class="mb-3 flex items-center justify-between gap-2">
      <h2 class="text-sm font-semibold">Your wish list</h2>
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
        <template v-else-if="regular > 0">
          <Check class="size-3.5" />
          Saved
        </template>
      </span>
    </div>

    <!-- Session unresolved: a placeholder line stands in until we know whether to show the
         sign-in nudge or the stepper — no flash of "Sign in" at an about-to-resolve user. -->
    <Skeleton v-if="!auth.sessionResolved && !auth.isAuthenticated" class="h-5 w-64" />

    <!-- Signed out (resolved): nudge to sign in (the route is public). -->
    <p v-else-if="!auth.isAuthenticated" class="text-muted-foreground text-sm">
      <RouterLink :to="loginTo" class="text-primary font-medium hover:underline"
        >Sign in</RouterLink
      >
      to keep a wish list of sealed products you want to buy.
    </p>

    <div v-else class="space-y-3">
      <div class="flex items-center justify-between gap-3">
        <span class="text-sm">Quantity</span>
        <div class="flex items-center gap-2">
          <Button
            variant="outline"
            size="icon"
            :disabled="loading || regular <= 0"
            aria-label="Remove one from your wish list"
            @click="adjust('quantity', -1)"
          >
            <Minus />
          </Button>
          <span class="w-8 text-center text-sm font-medium tabular-nums">{{ regular }}</span>
          <Button
            variant="outline"
            size="icon"
            :disabled="loading"
            aria-label="Add one to your wish list"
            @click="adjust('quantity', 1)"
          >
            <Plus />
          </Button>
        </div>
      </div>

      <p class="text-muted-foreground text-xs">
        <template v-if="regular > 0"> You want {{ regular }}. </template>
        <template v-else> Not on your wish list yet. </template>
      </p>
    </div>
  </section>
</template>
