<script setup lang="ts">
import { RouterLink, RouterView } from 'vue-router'
import CardsNav from '@/components/CardsNav.vue'
import CollectionsNav from '@/components/CollectionsNav.vue'
import ThemeToggle from '@/components/ThemeToggle.vue'
import UserMenu from '@/components/UserMenu.vue'
import { useAuthStore } from '@/stores/auth'

// Session restore happens once in the router guard (see router/index.ts).
const auth = useAuthStore()
</script>

<template>
  <div class="bg-background text-foreground min-h-screen">
    <header class="border-b">
      <div class="mx-auto flex h-14 max-w-6xl items-center justify-between px-4">
        <div class="flex items-center gap-1">
          <RouterLink to="/" class="text-lg font-semibold tracking-tight">TCGLense</RouterLink>
          <!-- CardsNav renders its own <nav> landmark (reka NavigationMenu), so this is a div. -->
          <div class="ml-3">
            <CardsNav />
          </div>
          <!-- Collections are per-account, so the nav only appears when signed in. -->
          <div v-if="auth.isAuthenticated">
            <CollectionsNav />
          </div>
        </div>
        <div class="flex items-center gap-1">
          <ThemeToggle />
          <UserMenu />
        </div>
      </div>
    </header>
    <main>
      <RouterView />
    </main>
  </div>
</template>
