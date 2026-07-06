<script setup lang="ts">
import { Skeleton } from '@/components/ui/skeleton'

// Placeholder for SetGroupGrid while a set list loads. Reproduces the real grid's
// static column mechanism (sm:grid-cols-2 lg:grid-cols-3) and SetTile's bordered tile
// shape — icon square + two text lines + the footer-row spacer that keeps childless
// tiles the same height as collapsible ones — so the incoming layout is reserved.
withDefaults(defineProps<{ count?: number }>(), { count: 9 })
</script>

<template>
  <div class="grid items-start gap-3 sm:grid-cols-2 lg:grid-cols-3">
    <div v-for="i in count" :key="i" class="rounded-xl border p-3">
      <div class="flex items-center gap-3">
        <Skeleton class="size-10 shrink-0 rounded-lg" />
        <div class="min-w-0 flex-1">
          <Skeleton class="h-4 w-3/4" />
          <Skeleton class="mt-1.5 h-3 w-1/2" />
        </div>
      </div>
      <!-- Matches SetTile's hidden footer spacer so a skeleton row is the same height. -->
      <div class="hidden h-11 sm:block" />
    </div>
  </div>
</template>
