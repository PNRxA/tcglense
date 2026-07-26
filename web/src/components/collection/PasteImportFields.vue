<script setup lang="ts">
import { computed } from 'vue'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'

// The paste tab's field: one plain-text box the user drops their collection into. Added
// for Mythic Tools (issue #572) — it's a phone app, so copying its export out beats
// saving a file and hunting for it in a browser's file picker — but the server sniffs the
// content, so a pasted CSV from any supported service works here too.
//
// The parent owns the pasted text (v-model) and the reconcile mode; this is presentation
// plus the format hint.
const text = defineModel<string>({ required: true })

const PLACEHOLDER = `2 Sol Ring (C21) 263
1 Aang, Air Nomad (TLE) 146 *F*
4 Counterspell`

/** Non-blank pasted lines, for the "we'll read N lines" reassurance under the box. */
const lineCount = computed(() => text.value.split('\n').filter((line) => line.trim()).length)
</script>

<template>
  <div class="space-y-2">
    <Label for="import-text">Paste your collection</Label>
    <Textarea
      id="import-text"
      v-model="text"
      class="min-h-40 font-mono text-xs"
      :placeholder="PLACEHOLDER"
      spellcheck="false"
    />
    <p v-if="lineCount" class="text-muted-foreground text-xs">
      {{ lineCount }} {{ lineCount === 1 ? 'line' : 'lines' }} pasted.
    </p>
    <div class="bg-muted/60 text-muted-foreground space-y-2 rounded-md p-3 text-xs">
      <div>
        <p class="text-foreground font-medium">Exporting from Mythic Tools</p>
        <p class="mt-1">
          Open the box, binder, or list you want, choose Export, and pick either the TXT or the CSV
          format — then paste the whole thing here. Both work.
        </p>
      </div>
      <div>
        <p class="text-foreground font-medium">Any card list works</p>
        <p class="mt-1">
          One card per line:
          <span class="text-foreground font-mono">2 Sol Ring (C21) 263</span>. Add
          <span class="text-foreground font-mono">*F*</span> for foils and
          <span class="text-foreground font-mono">*E*</span> for etched. That's the same format
          Moxfield, Archidekt and MTG Arena copy out, and you can paste a CSV export here instead.
        </p>
      </div>
      <p>
        A line with no set code — just
        <span class="text-foreground font-mono">4 Counterspell</span> — matches the most recent
        printing of that card.
      </p>
    </div>
  </div>
</template>
