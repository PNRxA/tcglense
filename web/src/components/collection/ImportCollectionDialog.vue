<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
import { Download, LoaderCircle, TriangleAlert } from '@lucide/vue'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import { Button, buttonVariants } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import CsvImportFields from '@/components/collection/CsvImportFields.vue'
import PasteImportFields from '@/components/collection/PasteImportFields.vue'
import ImportProgressBar from '@/components/collection/ImportProgressBar.vue'
import { useCollectionImport } from '@/composables/useCollectionImport'
import { formatImportSummaryLines } from '@/lib/importSummary'
import type { CollectionProvider, ReconcileMode } from '@/lib/api'

// The single management surface for importing a collection from an external provider: a
// one-off import with a chosen reconcile mode, from a link, an uploaded file, or pasted
// text. The reka dialog gives us a focus trap, Escape-to-close, and click-outside
// dismissal for free. Signed-in only — the parent view mounts this only when the visitor
// is authenticated.
const props = defineProps<{ game: string }>()

// One entry per provider. Moxfield's link import is temporarily disabled — its API only
// serves clients with a User-Agent it has approved, which we don't have yet — so it's shown
// but not selectable here; its CSV export still imports via the "Upload a file" tab. Drop
// `disabled` to re-enable once an approved MOXFIELD_USER_AGENT is configured server-side.
const PROVIDERS: { value: CollectionProvider; label: string; disabled?: boolean }[] = [
  { value: 'archidekt', label: 'Archidekt' },
  { value: 'moxfield', label: 'Moxfield — use the upload or paste tab for now', disabled: true },
]

// An example collection URL per provider, as the source input's placeholder. Partial:
// only providers listed in PROVIDERS above can be selected here, and a paste-only one
// (Mythic Tools) has no collection URL to show.
const PLACEHOLDERS: Partial<Record<CollectionProvider, string>> = {
  archidekt: 'https://archidekt.com/collection/v2/1042487',
  moxfield: 'https://moxfield.com/collection/4xUdq-66IEKK6X53bhUS8Q',
}

const MODES: { value: ReconcileMode; label: string; hint: string }[] = [
  {
    value: 'overwrite',
    label: 'Update matched cards',
    hint: 'Set counts for cards in the list; leave your other cards untouched.',
  },
  {
    value: 'merge',
    label: 'Add to my collection',
    hint: 'Add the imported counts on top of what you already own.',
  },
  {
    value: 'replace',
    label: 'Replace my collection',
    hint: 'Mirror the list exactly — this removes owned cards that aren’t in it.',
  },
]

// Three ways in: paste a public collection link (fetched server-side, async), upload an
// exported file, or paste the export's text straight in. All three are one-off — nothing
// about the source is remembered.
//
// The paste tab exists because not every service has an API or a browser-friendly export:
// Mythic Tools (issue #572) is a phone app, and pasting what you copied out of it is much
// less friction than saving a file and finding it in a file picker.
type SourceType = 'link' | 'csv' | 'text'

const open = ref(false)
const sourceType = ref<SourceType>('link')
const provider = ref<CollectionProvider>('archidekt')
const sourceInput = ref('')
const mode = ref<ReconcileMode>('overwrite')
const csvFile = ref<File | null>(null)
const pastedText = ref('')

// The import lifecycle (mutations, the polled background job, busy/status/error/result)
// lives in the composable; this component owns only the form inputs and `canSubmit`.
const gameRef = toRef(props, 'game')
const {
  errorMessage,
  result,
  busy,
  statusMessage,
  progress,
  resetStatus,
  runLinkImport,
  runCsvImport,
  runTextImport,
} = useCollectionImport(gameRef)

// Reset the form each time the dialog opens, clearing any leftover status from a previous
// session (the component instance persists across opens and across game switches).
watch(open, (isOpen) => {
  if (!isOpen) return
  sourceType.value = 'link'
  provider.value = 'archidekt'
  sourceInput.value = ''
  mode.value = 'overwrite'
  csvFile.value = null
  pastedText.value = ''
  resetStatus()
})

// Switching tabs clears the previous tab's outcome/error so stale feedback never lingers.
// Also drop any chosen file: the file tab's input is remounted on the way back (v-if), so
// it renders empty — clearing csvFile keeps Import's enabled state honest (no silently
// staged, no-longer-visible upload). Pasted text survives the switch: it's still visible
// when you come back, so it can't become a hidden staged import.
watch(sourceType, () => {
  resetStatus()
  csvFile.value = null
})

function onCsvFile(file: File | null) {
  csvFile.value = file
  resetStatus()
}

const providerLabel = computed(
  () => PROVIDERS.find((p) => p.value === provider.value)?.label ?? provider.value,
)
const canSubmit = computed(() => {
  if (busy.value) return false
  if (sourceType.value === 'csv') return csvFile.value != null
  if (sourceType.value === 'text') return pastedText.value.trim().length > 0
  return sourceInput.value.trim().length > 0
})

async function runImport() {
  if (!canSubmit.value) return
  if (sourceType.value === 'csv') {
    if (csvFile.value) await runCsvImport({ file: csvFile.value, mode: mode.value })
    return
  }
  if (sourceType.value === 'text') {
    await runTextImport({ text: pastedText.value, mode: mode.value })
    return
  }
  await runLinkImport({
    provider: provider.value,
    source: sourceInput.value.trim(),
    mode: mode.value,
  })
}

// Human-readable summary lines for the result panel (shared, testable formatter).
const resultLines = computed(() => (result.value ? formatImportSummaryLines(result.value) : []))

const selectClass =
  'border-input dark:bg-input/30 flex h-9 w-full rounded-md border bg-transparent px-3 text-sm ' +
  'shadow-xs outline-none focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px]'
</script>

<template>
  <Dialog v-model:open="open">
    <DialogTrigger :class="buttonVariants({ variant: 'outline', size: 'sm' })">
      <Download />
      Import collection
    </DialogTrigger>

    <DialogContent
      class="bg-background max-h-[90vh] w-[min(92vw,32rem)] overflow-y-auto rounded-xl border p-6 shadow-xl"
    >
      <DialogTitle class="text-lg font-semibold">Import a collection</DialogTitle>
      <DialogDescription class="text-muted-foreground mt-1 text-sm">
        <template v-if="sourceType === 'link'">
          Paste a public {{ providerLabel }} collection URL (or id) and choose how to reconcile it
          with your collection. We fetch it server-side — nothing is uploaded from your device.
        </template>
        <template v-else-if="sourceType === 'csv'">
          Upload a collection export from Mythic Tools, Archidekt or Moxfield and choose how to
          reconcile it with your collection. We detect the format automatically.
        </template>
        <template v-else>
          Paste your collection as text — a card list, or a whole CSV export from Mythic Tools,
          Archidekt or Moxfield — and choose how to reconcile it. We detect the format
          automatically.
        </template>
      </DialogDescription>

      <div class="mt-5 space-y-5">
        <!-- Source: paste a link, upload a file, or paste the export's text. -->
        <div class="bg-muted grid grid-cols-3 gap-1 rounded-lg p-1" role="tablist">
          <button
            type="button"
            role="tab"
            :aria-selected="sourceType === 'link'"
            class="rounded-md px-3 py-1.5 text-sm font-medium transition-colors"
            :class="
              sourceType === 'link'
                ? 'bg-background shadow-sm'
                : 'text-muted-foreground hover:text-foreground'
            "
            @click="sourceType = 'link'"
          >
            Paste a link
          </button>
          <button
            type="button"
            role="tab"
            :aria-selected="sourceType === 'csv'"
            class="rounded-md px-3 py-1.5 text-sm font-medium transition-colors"
            :class="
              sourceType === 'csv'
                ? 'bg-background shadow-sm'
                : 'text-muted-foreground hover:text-foreground'
            "
            @click="sourceType = 'csv'"
          >
            Upload a file
          </button>
          <button
            type="button"
            role="tab"
            :aria-selected="sourceType === 'text'"
            class="rounded-md px-3 py-1.5 text-sm font-medium transition-colors"
            :class="
              sourceType === 'text'
                ? 'bg-background shadow-sm'
                : 'text-muted-foreground hover:text-foreground'
            "
            @click="sourceType = 'text'"
          >
            Paste a list
          </button>
        </div>

        <!-- Link tab: provider + collection URL/id. -->
        <template v-if="sourceType === 'link'">
          <div class="space-y-1.5">
            <Label for="import-provider">Provider</Label>
            <select id="import-provider" v-model="provider" :class="selectClass">
              <option v-for="p in PROVIDERS" :key="p.value" :value="p.value" :disabled="p.disabled">
                {{ p.label }}
              </option>
            </select>
          </div>

          <div class="space-y-1.5">
            <Label for="import-source">Collection URL or id</Label>
            <Input id="import-source" v-model="sourceInput" :placeholder="PLACEHOLDERS[provider]" />
          </div>
        </template>

        <!-- Upload tab: file picker + how to export from each supported service. -->
        <CsvImportFields v-else-if="sourceType === 'csv'" @file-change="onCsvFile" />

        <!-- Paste tab: one plain-text box, sniffed server-side (issue #572). -->
        <PasteImportFields v-else v-model="pastedText" />

        <!-- Reconcile mode -->
        <fieldset class="space-y-2">
          <legend class="mb-1 text-sm font-medium">How should we reconcile it?</legend>
          <label
            v-for="m in MODES"
            :key="m.value"
            class="flex cursor-pointer gap-3 rounded-md border p-3 transition-colors"
            :class="mode === m.value ? 'border-ring bg-accent/40' : 'hover:bg-accent/30'"
          >
            <input v-model="mode" type="radio" name="import-mode" :value="m.value" class="mt-1" />
            <span>
              <span class="block text-sm font-medium">{{ m.label }}</span>
              <span class="text-muted-foreground block text-xs">{{ m.hint }}</span>
            </span>
          </label>
        </fieldset>

        <!-- In-progress status (queued / running) + a live fetch-progress bar once running. -->
        <div v-if="statusMessage" class="space-y-2">
          <p class="text-muted-foreground flex items-start gap-2 text-sm" aria-live="polite">
            <LoaderCircle class="mt-0.5 size-4 shrink-0 animate-spin" />
            <span>{{ statusMessage }}</span>
          </p>
          <ImportProgressBar v-if="progress" :progress="progress" />
        </div>

        <!-- Error -->
        <p
          v-if="errorMessage"
          class="text-destructive flex items-start gap-2 text-sm"
          aria-live="polite"
        >
          <TriangleAlert class="mt-0.5 size-4 shrink-0" />
          <span>{{ errorMessage }}</span>
        </p>

        <!-- Result -->
        <div
          v-if="resultLines.length"
          class="bg-muted space-y-1 rounded-md p-3 text-sm"
          aria-live="polite"
        >
          <p v-for="(line, i) in resultLines" :key="i">{{ line }}</p>
        </div>
      </div>

      <div class="mt-6 flex justify-end gap-2">
        <DialogClose :class="buttonVariants({ variant: 'outline' })">Close</DialogClose>
        <Button :disabled="!canSubmit" @click="runImport">
          {{ busy ? 'Working…' : 'Import' }}
        </Button>
      </div>
    </DialogContent>
  </Dialog>
</template>
