<script setup lang="ts">
import { computed } from 'vue'
import { Library } from '@lucide/vue'
import { RouterLink, useRoute } from 'vue-router'
import { buttonVariants } from '@/components/ui/button'

// The signed-out prompt shown on the (public) collection pages instead of bouncing to
// the login page. Shared by the collection landing and the browse grids so both preserve
// the return path: after signing in / up the user comes back to wherever they were
// (both /login and /register honour ?redirect via useAuthSubmit).
const props = defineProps<{ gameName: string }>()
const route = useRoute()
const loginTo = computed(() => ({ path: '/login', query: { redirect: route.fullPath } }))
const registerTo = computed(() => ({ path: '/register', query: { redirect: route.fullPath } }))
</script>

<template>
  <div class="mx-auto max-w-md py-16 text-center">
    <div class="bg-muted mx-auto flex size-12 items-center justify-center rounded-lg">
      <Library class="size-6" aria-hidden="true" />
    </div>
    <h1 class="mt-4 text-2xl font-semibold tracking-tight">Sign in to view your collection</h1>
    <p class="text-muted-foreground mt-2">
      Track which {{ props.gameName }} cards you own and what they're worth. Sign in or create a
      free account to start your collection.
    </p>
    <div class="mt-6 flex justify-center gap-3">
      <RouterLink :to="loginTo" :class="buttonVariants({ variant: 'default' })">Sign in</RouterLink>
      <RouterLink :to="registerTo" :class="buttonVariants({ variant: 'outline' })">
        Create account
      </RouterLink>
    </div>
  </div>
</template>
