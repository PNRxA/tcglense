<script setup lang="ts">
import { computed } from 'vue'
import { HeartPulse } from '@lucide/vue'
import { RouterLink, useRoute } from 'vue-router'
import { buttonVariants } from '@/components/ui/button'

// The signed-out prompt on the life-counter pages, in place of bouncing to the login page.
//
// The counter is server-backed on purpose — that's what lets a game survive a dropped phone, be
// picked up on a tablet, and feed a deck's win record — so it genuinely needs an account. Saying
// that, and preserving the return path (both /login and /register honour `?redirect`), beats a
// redirect that loses where you were going. Mirrors CollectionSignInPrompt's treatment.
defineProps<{ gameName: string }>()

const route = useRoute()
const loginTo = computed(() => ({ path: '/login', query: { redirect: route.fullPath } }))
const registerTo = computed(() => ({ path: '/register', query: { redirect: route.fullPath } }))
</script>

<template>
  <div class="mx-auto max-w-md py-16 text-center">
    <div class="bg-muted mx-auto flex size-12 items-center justify-center rounded-lg">
      <HeartPulse class="size-6" aria-hidden="true" />
    </div>
    <h1 class="mt-4 text-2xl font-semibold tracking-tight">Sign in to use the life counter</h1>
    <p class="text-muted-foreground mt-2">
      Games are tracked on your account, so a {{ gameName }} game survives a closed tab and can be
      picked up on another device — and linking a player to one of your decks builds its win record.
      Sign in or create a free account to start counting.
    </p>
    <div class="mt-6 flex justify-center gap-3">
      <RouterLink :to="loginTo" :class="buttonVariants({ variant: 'default' })">Sign in</RouterLink>
      <RouterLink :to="registerTo" :class="buttonVariants({ variant: 'outline' })">
        Create account
      </RouterLink>
    </div>
  </div>
</template>
