<script setup lang="ts">
import { RouterLink } from 'vue-router'

defineProps<{ title: string; lastUpdated: string; siblingLabel: string; siblingTo: string }>()
</script>

<template>
  <article class="mx-auto max-w-3xl px-4 py-12">
    <header>
      <h1 class="text-3xl font-semibold tracking-tight">{{ title }}</h1>
      <p class="text-muted-foreground mt-2 text-sm">Last updated: {{ lastUpdated }}</p>
    </header>
    <div class="mt-8 space-y-10">
      <slot />
    </div>
    <nav
      class="mt-12 flex flex-wrap gap-x-6 gap-y-2 border-t pt-6 text-sm"
      aria-label="Legal pages"
    >
      <RouterLink to="/" class="text-primary underline-offset-4 hover:underline">
        Back to TCGLense
      </RouterLink>
      <RouterLink :to="siblingTo" class="text-primary underline-offset-4 hover:underline">
        {{ siblingLabel }}
      </RouterLink>
    </nav>
  </article>
</template>
