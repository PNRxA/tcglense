<script setup lang="ts">
import { computed, toRef } from 'vue'
import { Check, Loader2, Minus, Plus } from '@lucide/vue'
import { RouterLink, useRoute } from 'vue-router'
import type { Product } from '@/lib/api'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { useCollectionProductEntryQuery } from '@/composables/useCollection'
import { useWishlistProductEntryQuery } from '@/composables/useWishlist'
import { useOwnedCountEditor, type CardListTarget } from '@/composables/useOwnedCountEditor'
import { useAuthStore } from '@/stores/auth'

// One list-targeted sealed-product control. The detail page instantiates it for both the
// collection and wish list; both use the shared product holding editor/query engines.
const props = withDefaults(
  defineProps<{ game: string; product: Product; list?: CardListTarget }>(),
  { list: 'wishlist' },
)
const auth = useAuthStore()
const route = useRoute()

const game = toRef(props, 'game')
const productId = computed(() => props.product.id)

const entryQuery =
  props.list === 'wishlist'
    ? useWishlistProductEntryQuery(game, productId)
    : useCollectionProductEntryQuery(game, productId)
const seed = computed(() => entryQuery.data.value)
const { regular, adjust, saving, saveError } = useOwnedCountEditor(game, productId, seed, {
  kind: 'product',
  list: props.list,
})

// Disable the steppers until the initial holding has loaded, so an early click can't
// adjust off a stale zero.
const loading = computed(() => auth.isAuthenticated && entryQuery.isLoading.value)
const loginTo = computed(() => ({ path: '/login', query: { redirect: route.fullPath } }))
const listName = computed(() => (props.list === 'wishlist' ? 'wish list' : 'collection'))
const verb = computed(() => (props.list === 'wishlist' ? 'want' : 'own'))
</script>

<template>
  <section class="bg-card rounded-xl border p-4 shadow-sm">
    <div class="mb-3 flex items-center justify-between gap-2">
      <h2 class="text-sm font-semibold">Your {{ listName }}</h2>
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
      to track sealed products in your {{ listName }}.
    </p>

    <div v-else class="space-y-3">
      <div class="flex items-center justify-between gap-3">
        <span class="text-sm">Quantity</span>
        <div class="flex items-center gap-2">
          <Button
            variant="outline"
            size="icon"
            :disabled="loading || regular <= 0"
            :aria-label="`Remove one from your ${listName}`"
            @click="adjust('quantity', -1)"
          >
            <Minus />
          </Button>
          <span class="w-8 text-center text-sm font-medium tabular-nums">{{ regular }}</span>
          <Button
            variant="outline"
            size="icon"
            :disabled="loading"
            :aria-label="`Add one to your ${listName}`"
            @click="adjust('quantity', 1)"
          >
            <Plus />
          </Button>
        </div>
      </div>

      <p class="text-muted-foreground text-xs">
        <template v-if="regular > 0"> You {{ verb }} {{ regular }}. </template>
        <template v-else> Not in your {{ listName }} yet. </template>
      </p>
    </div>
  </section>
</template>
