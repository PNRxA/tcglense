<script setup lang="ts">
// The price stat tiles both detail pages show in their left rail: the primary market
// price as a full-width hero tile, the remaining quotes (foil, Cardmarket EUR, MTGO
// tix, MSRP) as half-width tiles beneath it. Purely presentational — each caller
// builds its own `rows` (cards and sealed products expose different quote sets) and
// filters out the blanks before handing them down.
defineProps<{ rows: { label: string; value: string }[] }>()
</script>

<template>
  <dl v-if="rows.length" class="grid grid-cols-2 gap-2">
    <div
      v-for="(row, index) in rows"
      :key="row.label"
      class="bg-muted/50 rounded-lg border p-3"
      :class="index === 0 ? 'col-span-2' : ''"
    >
      <dt class="text-muted-foreground text-xs">{{ row.label }}</dt>
      <dd
        class="mt-0.5 font-semibold tabular-nums"
        :class="index === 0 ? 'text-2xl tracking-tight' : 'text-sm'"
      >
        {{ row.value }}
      </dd>
    </div>
  </dl>
</template>
