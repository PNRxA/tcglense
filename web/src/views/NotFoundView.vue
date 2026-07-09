<script setup lang="ts">
import { RouterLink } from 'vue-router'
import { buttonVariants } from '@/components/ui/button'
import { usePageMeta } from '@/lib/seo'

// The catch-all view for any path that matches no route (see the router's
// `/:pathMatch(.*)*` entry). A 404 has no content of its own to index, and — because
// TCGLense is a client-rendered SPA served with an index.html fallback — an unknown
// path returns HTTP 200 (a "soft 404"), so `noindex, nofollow` (set here via
// usePageMeta) is the SPA-level signal that keeps dead URLs out of the search index.
// No canonicalPath: the page maps to no real URL, so a self-canonical would be
// meaningless on a noindex page.
usePageMeta({ title: 'Page not found', noindex: true })
</script>

<template>
  <div
    class="mx-auto flex min-h-[60vh] max-w-xl flex-col items-center justify-center px-4 py-16 text-center"
  >
    <p class="text-primary text-6xl font-bold tracking-tight sm:text-7xl">404</p>
    <h1 class="mt-4 text-2xl font-semibold tracking-tight">Page not found</h1>
    <p class="text-muted-foreground mt-3">
      The page you're looking for doesn't exist or may have moved.
    </p>
    <div class="mt-8 flex flex-wrap items-center justify-center gap-3">
      <RouterLink to="/" :class="buttonVariants({ size: 'lg' })">Go home</RouterLink>
      <RouterLink to="/cards" :class="buttonVariants({ variant: 'outline', size: 'lg' })">
        Browse cards
      </RouterLink>
      <RouterLink to="/sealed" :class="buttonVariants({ variant: 'outline', size: 'lg' })">
        Browse sealed
      </RouterLink>
    </div>
  </div>
</template>
