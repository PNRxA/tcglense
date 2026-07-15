<script setup lang="ts">
import { Construction } from '@lucide/vue'
</script>

<template>
  <main
    class="mx-auto flex min-h-screen max-w-xl flex-col items-center justify-center px-6 py-16 text-center"
    role="status"
    aria-live="polite"
  >
    <div class="bg-primary/10 text-primary flex size-16 items-center justify-center rounded-2xl">
      <Construction class="size-8" aria-hidden="true" />
    </div>
    <p class="text-primary mt-6 text-sm font-semibold tracking-wide uppercase">TCGLense</p>
    <h1 class="mt-2 text-3xl font-bold tracking-tight sm:text-4xl">Maintenance in progress</h1>
    <p class="text-muted-foreground mt-4 text-balance">
      We're applying an update and will be back shortly. Your collection and account data are safe.
    </p>
    <p class="text-muted-foreground mt-2 text-sm">Please check back in a few minutes.</p>
  </main>
</template>
