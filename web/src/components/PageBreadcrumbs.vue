<script setup lang="ts">
import { RouterLink } from 'vue-router'

// A page-level breadcrumb trail: each item with a `to` renders as a link, the
// final (or any `to`-less) item is the current-page terminal.
defineProps<{ items: { label: string; to?: string }[] }>()
</script>

<template>
  <nav aria-label="Breadcrumb" class="text-muted-foreground mb-4 text-sm">
    <template v-for="(item, index) in items" :key="index">
      <span v-if="index > 0" class="mx-1.5">/</span>
      <RouterLink v-if="item.to" :to="item.to" class="hover:underline">{{ item.label }}</RouterLink>
      <span v-else class="text-foreground">{{ item.label }}</span>
    </template>
  </nav>
</template>
