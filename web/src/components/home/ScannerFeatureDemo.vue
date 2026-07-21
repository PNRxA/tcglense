<script setup lang="ts">
import { Camera, Check, Plus, ScanLine } from '@lucide/vue'
</script>

<template>
  <div class="grid items-center gap-4 lg:grid-cols-[minmax(0,1.1fr)_minmax(9rem,0.9fr)]">
    <div
      class="from-muted/80 to-muted/30 relative mx-auto aspect-[4/3] w-full max-w-sm overflow-hidden rounded-xl border bg-gradient-to-br"
    >
      <div
        class="bg-background/90 absolute top-3 left-3 inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-[10px] font-medium shadow-sm"
      >
        <Camera class="size-3" />
        Camera ready
      </div>

      <div
        class="border-primary/70 bg-card/70 absolute top-1/2 left-1/2 aspect-[5/7] h-[72%] -translate-x-1/2 -translate-y-1/2 overflow-hidden rounded-md border-2 shadow-sm"
      >
        <div class="from-primary/30 via-primary/10 h-[58%] bg-gradient-to-br to-transparent"></div>
        <div class="space-y-2 p-2">
          <div class="bg-foreground/20 h-1.5 w-4/5 rounded-full"></div>
          <div class="bg-foreground/10 h-1.5 w-3/5 rounded-full"></div>
          <div class="bg-foreground/10 mt-3 h-1 w-full rounded-full"></div>
          <div class="bg-foreground/10 h-1 w-5/6 rounded-full"></div>
        </div>
      </div>

      <div
        class="bg-background/90 absolute right-3 bottom-3 inline-flex items-center gap-1.5 rounded-full border border-emerald-500/30 px-2.5 py-1 text-[10px] font-medium text-emerald-700 shadow-sm dark:text-emerald-400"
      >
        <ScanLine class="size-3" />
        Card locked on
      </div>
    </div>

    <div class="bg-background rounded-xl border p-3 shadow-sm">
      <div class="flex items-center gap-1.5 text-xs font-semibold">
        <Check class="text-primary size-3.5" />
        Artwork matched
      </div>
      <div class="mt-3 flex gap-2.5">
        <div class="bg-muted aspect-[5/7] w-10 shrink-0 overflow-hidden rounded border">
          <div class="from-primary/25 via-primary/10 h-2/5 bg-gradient-to-br to-transparent"></div>
        </div>
        <div class="min-w-0 flex-1 pt-0.5">
          <div class="bg-foreground/20 h-1.5 w-full rounded-full"></div>
          <div class="bg-foreground/10 mt-2 h-1.5 w-3/4 rounded-full"></div>
          <p class="text-muted-foreground mt-2 text-[10px]">Confirm the exact printing</p>
        </div>
      </div>
      <div class="mt-3 flex items-center justify-between border-t pt-3 text-xs">
        <span class="text-muted-foreground">Regular</span>
        <span
          class="border-primary/30 bg-primary/10 text-primary inline-flex items-center gap-1 rounded-full border px-2 py-0.5 font-medium"
        >
          <Plus class="size-3" />
          1 ready
        </span>
      </div>
    </div>
  </div>
</template>
