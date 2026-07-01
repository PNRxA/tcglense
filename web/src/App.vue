<script setup lang="ts">
import { RouterLink, RouterView } from 'vue-router'
import MainNav from '@/components/MainNav.vue'
import ThemeToggle from '@/components/ThemeToggle.vue'
import UserMenu from '@/components/UserMenu.vue'

// Session restore happens once in the router guard (see router/index.ts).
</script>

<template>
  <div class="bg-background text-foreground min-h-screen">
    <header class="border-b">
      <div class="mx-auto flex h-14 max-w-6xl items-center justify-between px-4">
        <div class="flex items-center gap-1">
          <RouterLink to="/" class="text-lg font-semibold tracking-tight">TCGLense</RouterLink>
          <!-- MainNav renders its own <nav> landmark (reka NavigationMenu), so this is a div.
               Both dropdowns live under one NavigationMenu so the swipe/fade motion plays
               when moving between them. -->
          <div class="ml-3">
            <MainNav />
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
