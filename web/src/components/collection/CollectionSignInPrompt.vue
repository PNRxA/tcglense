<script setup lang="ts">
import { computed } from 'vue'
import { Library } from '@lucide/vue'
import { RouterLink, useRoute } from 'vue-router'
import { buttonVariants } from '@/components/ui/button'
import type { CardListTarget } from '@/composables/useOwnedCountEditor'

// The signed-out prompt shown on the (public) collection and wish-list pages instead of
// bouncing to the login page. Shared by the landing and the browse grids so all preserve
// the return path: after signing in / up the user comes back to wherever they were
// (both /login and /register honour ?redirect via useAuthSubmit). The copy is
// prop-driven via `list` (collection wording by default; wish-list wording for #167).
const props = withDefaults(defineProps<{ gameName: string; list?: CardListTarget }>(), {
  list: 'collection',
})
const route = useRoute()
const loginTo = computed(() => ({ path: '/login', query: { redirect: route.fullPath } }))
const registerTo = computed(() => ({ path: '/register', query: { redirect: route.fullPath } }))

const copy = computed(() =>
  props.list === 'wishlist'
    ? {
        title: 'Sign in to view your wish list',
        body:
          `Keep a wish list of the ${props.gameName} cards you want to buy and what ` +
          `they'd cost. Sign in or create a free account to start your wish list.`,
      }
    : {
        title: 'Sign in to view your collection',
        body:
          `Track which ${props.gameName} cards you own and what they're worth. ` +
          `Sign in or create a free account to start your collection.`,
      },
)
</script>

<template>
  <div class="mx-auto max-w-md py-16 text-center">
    <div class="bg-muted mx-auto flex size-12 items-center justify-center rounded-lg">
      <Library class="size-6" aria-hidden="true" />
    </div>
    <h1 class="mt-4 text-2xl font-semibold tracking-tight">{{ copy.title }}</h1>
    <p class="text-muted-foreground mt-2">{{ copy.body }}</p>
    <div class="mt-6 flex justify-center gap-3">
      <RouterLink :to="loginTo" :class="buttonVariants({ variant: 'default' })">Sign in</RouterLink>
      <RouterLink :to="registerTo" :class="buttonVariants({ variant: 'outline' })">
        Create account
      </RouterLink>
    </div>
  </div>
</template>
