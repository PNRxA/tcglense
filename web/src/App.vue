<script setup lang="ts">
import { RouterLink, RouterView } from 'vue-router'
import ThemeToggle from '@/components/ThemeToggle.vue'
import UserMenu from '@/components/UserMenu.vue'

// Session restore happens once in the router guard (see router/index.ts).
</script>

<template>
  <div class="bg-background text-foreground min-h-screen">
    <header class="border-b">
      <div class="mx-auto flex h-14 max-w-5xl items-center justify-between px-4">
        <RouterLink to="/" class="text-lg font-semibold tracking-tight">TCGLense</RouterLink>
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
