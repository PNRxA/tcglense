<script setup lang="ts">
import { RouterLink, RouterView } from 'vue-router'
import CardDetailDialog from '@/components/cards/CardDetailDialog.vue'
import MainNav from '@/components/MainNav.vue'
import MobileNav from '@/components/MobileNav.vue'
import ThemeToggle from '@/components/ThemeToggle.vue'
import UserMenu from '@/components/UserMenu.vue'
import { useAuthCacheReset } from '@/composables/useAuthCacheReset'

// Session restore happens once in the router guard (see router/index.ts).

// Wipe the per-user query cache whenever the signed-in identity changes, so one
// account never sees another's cached collection/wish list (issue #177).
useAuthCacheReset()
</script>

<template>
  <div class="bg-background text-foreground min-h-screen overflow-x-hidden">
    <header class="border-b">
      <div class="mx-auto flex h-14 max-w-6xl items-center justify-between gap-2 px-4">
        <div class="flex min-w-0 items-center gap-1">
          <!-- Below sm the two nav dropdowns don't fit alongside the brand + theme +
               account controls, so they collapse into MobileNav's hamburger. -->
          <MobileNav class="sm:hidden" />
          <RouterLink to="/" class="truncate text-lg font-semibold tracking-tight"
            >TCGLense</RouterLink
          >
          <!-- MainNav renders its own <nav> landmark (reka NavigationMenu), so this is a div.
               Both dropdowns live under one NavigationMenu so the swipe/fade motion plays
               when moving between them. Hidden below sm in favour of MobileNav. -->
          <div class="ml-3 hidden sm:block">
            <MainNav />
          </div>
        </div>
        <div class="flex shrink-0 items-center gap-1">
          <ThemeToggle />
          <UserMenu />
        </div>
      </div>
    </header>
    <main>
      <RouterView />
    </main>
    <!-- The card-detail modal any browse grid opens via `?card=<id>` (see CardTile /
         CardDetailDialog) — mounted once here so it overlays whichever page is up. -->
    <CardDetailDialog />
  </div>
</template>
