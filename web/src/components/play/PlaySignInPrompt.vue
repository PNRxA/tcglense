<script setup lang="ts">
import { computed } from 'vue'
import { Swords } from '@lucide/vue'
import { RouterLink, useRoute } from 'vue-router'
import { buttonVariants } from '@/components/ui/button'

// The signed-out prompt on the play hub — in place, like the life counter's, rather than a
// bounce to /login.
//
// What it must NOT say is "sign in to play": a guest with an invite link plays fine, and that's
// the point of the whole feature. Only *hosting* needs an account (a room belongs to someone,
// and the decks you'd load are yours), so the copy is about opening a table, and the hub keeps
// its join-by-code box visible right beside this.
defineProps<{ gameName: string }>()

const route = useRoute()
const loginTo = computed(() => ({ path: '/login', query: { redirect: route.fullPath } }))
const registerTo = computed(() => ({ path: '/register', query: { redirect: route.fullPath } }))
</script>

<template>
  <div class="bg-card rounded-xl border p-6 text-center">
    <div class="bg-muted mx-auto flex size-12 items-center justify-center rounded-lg">
      <Swords class="size-6" aria-hidden="true" />
    </div>
    <h2 class="mt-4 text-xl font-semibold tracking-tight">Sign in to open a table</h2>
    <p class="text-muted-foreground mx-auto mt-2 max-w-sm text-sm">
      A room belongs to the account that opened it, and playing one of your own {{ gameName }}
      decks needs somewhere to read it from. Joining someone else's table needs no account at all —
      if you have an invite code, use it below.
    </p>
    <div class="mt-6 flex justify-center gap-3">
      <RouterLink :to="loginTo" :class="buttonVariants({ variant: 'default' })">Sign in</RouterLink>
      <RouterLink :to="registerTo" :class="buttonVariants({ variant: 'outline' })">
        Create account
      </RouterLink>
    </div>
  </div>
</template>
