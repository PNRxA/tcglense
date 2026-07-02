<script setup lang="ts">
import { Label } from '@/components/ui/label'

// The CSV-upload tab's fields: the file picker plus the "how to export" hint for each
// supported service (the server sniffs which shape an upload is from its header row).
// The parent owns the chosen file and the reconcile mode; this just emits the picked
// file (null when cleared). Rendered under v-if so the native file input remounts empty
// when the user leaves and returns to this tab.
const emit = defineEmits<{ fileChange: [File | null] }>()

function onChange(event: Event) {
  const input = event.target as HTMLInputElement
  emit('fileChange', input.files?.[0] ?? null)
}
</script>

<template>
  <div class="space-y-2">
    <Label for="import-csv">Collection CSV file</Label>
    <input
      id="import-csv"
      type="file"
      accept=".csv,text/csv"
      class="border-input dark:bg-input/30 file:bg-muted file:text-foreground block w-full cursor-pointer rounded-md border bg-transparent text-sm file:mr-3 file:cursor-pointer file:border-0 file:px-3 file:py-2 file:text-sm file:font-medium focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px] focus-visible:outline-none"
      @change="onChange"
    />
    <div class="bg-muted/60 text-muted-foreground space-y-2 rounded-md p-3 text-xs">
      <div>
        <p class="text-foreground font-medium">Exporting from Archidekt</p>
        <p class="mt-1">
          Open your collection and choose Export → CSV. You only need these three columns — you can
          leave the rest unchecked:
        </p>
        <ul class="mt-1.5 flex flex-wrap gap-1.5">
          <li class="bg-background rounded border px-1.5 py-0.5 font-medium">Scryfall ID</li>
          <li class="bg-background rounded border px-1.5 py-0.5 font-medium">Finish</li>
          <li class="bg-background rounded border px-1.5 py-0.5 font-medium">Quantity</li>
        </ul>
      </div>
      <div>
        <p class="text-foreground font-medium">Exporting from Moxfield</p>
        <p class="mt-1">
          Open your collection and choose Export — the standard export already includes everything
          we need (Count, Edition, Collector Number, Foil).
        </p>
      </div>
    </div>
  </div>
</template>
