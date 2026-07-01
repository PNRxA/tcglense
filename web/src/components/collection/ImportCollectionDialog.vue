<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
import { Download, LoaderCircle, Trash2, TriangleAlert } from '@lucide/vue'
import { useQueryClient } from '@tanstack/vue-query'
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
import {
  invalidateCollectionData,
  useDeleteCollectionSourceMutation,
  useImportCollectionCsvMutation,
  useImportCollectionMutation,
  useImportJobQuery,
  useSaveCollectionSourceMutation,
} from '@/composables/useCollection'
import { ApiError, MAX_CSV_UPLOAD_BYTES } from '@/lib/api'
import type { CollectionProvider, CollectionSource, ImportSummary, ReconcileMode } from '@/lib/api'

// The single management surface for importing a collection from an external provider:
// a one-off import (with a chosen reconcile mode), optionally saving the link for
// one-click re-syncing, and removing a saved link. The reka dialog gives us a focus
// trap, Escape-to-close, and click-outside dismissal for free. Signed-in only — the
// parent view mounts this only when the visitor is authenticated.
const props = defineProps<{ game: string; source: CollectionSource | null }>()

// One entry per supported provider — structured so Moxfield slots in later.
const PROVIDERS: { value: CollectionProvider; label: string }[] = [
  { value: 'archidekt', label: 'Archidekt' },
]

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

// Two ways in: paste a public collection link (fetched server-side, async) or upload an
// exported CSV (parsed server-side, synchronous). A CSV is inherently one-off — there's
// no location to re-sync from — so the "save link" affordance only applies to the link tab.
type SourceType = 'link' | 'csv'

const open = ref(false)
const sourceType = ref<SourceType>('link')
const provider = ref<CollectionProvider>('archidekt')
const sourceInput = ref(props.source?.url ?? '')
const mode = ref<ReconcileMode>('overwrite')
const saveLink = ref(props.source != null)
const csvFile = ref<File | null>(null)

const gameRef = toRef(props, 'game')
const qc = useQueryClient()
const importMutation = useImportCollectionMutation()
const importCsvMutation = useImportCollectionCsvMutation()
const saveMutation = useSaveCollectionSourceMutation()
const deleteMutation = useDeleteCollectionSourceMutation()

const enqueuing = ref(false)
const errorMessage = ref<string | null>(null)
const result = ref<ImportSummary | null>(null)

// The in-flight import job we're polling (null when none). The import runs in the
// background (throttled by the provider rate limit); we poll it to completion.
const jobId = ref<number | null>(null)
const jobQuery = useImportJobQuery(gameRef, jobId)

// Reset the form each time the dialog opens: seed the URL/checkbox from the current
// saved link and clear any leftover status from a previous session (the component
// instance persists across opens and across game switches).
watch(open, (isOpen) => {
  if (!isOpen) return
  sourceType.value = 'link'
  provider.value = 'archidekt'
  sourceInput.value = props.source?.url ?? ''
  mode.value = 'overwrite'
  saveLink.value = props.source != null
  csvFile.value = null
  errorMessage.value = null
  result.value = null
  jobId.value = null
  enqueuing.value = false
})

// Switching tabs clears the previous tab's outcome/error so stale feedback never lingers.
// Also drop any chosen file: the CSV tab's file input is remounted on the way back (v-if),
// so it renders empty — clearing csvFile keeps Import's enabled state honest (no silently
// staged, no-longer-visible upload).
watch(sourceType, () => {
  errorMessage.value = null
  result.value = null
  jobId.value = null
  csvFile.value = null
})

function onCsvFileChange(event: Event) {
  const input = event.target as HTMLInputElement
  csvFile.value = input.files?.[0] ?? null
  errorMessage.value = null
  result.value = null
}

// React to the polled job reaching a terminal status.
watch(
  () => jobQuery.data.value,
  (job) => {
    if (!job) return
    if (job.status === 'complete') {
      result.value = job.summary ?? null
      // The collection contents changed — refresh the grid, header, and card steppers.
      invalidateCollectionData(qc, props.game)
    } else if (job.status === 'error') {
      errorMessage.value = job.error ?? 'Import failed. Please try again.'
    }
  },
)

const providerLabel = computed(
  () => PROVIDERS.find((p) => p.value === provider.value)?.label ?? provider.value,
)
const jobStatus = computed(() => jobQuery.data.value?.status ?? null)
const processing = computed(() => jobStatus.value === 'queued' || jobStatus.value === 'running')
const busy = computed(
  () => enqueuing.value || processing.value || importCsvMutation.isPending.value,
)
const canSubmit = computed(() => {
  if (busy.value) return false
  return sourceType.value === 'csv' ? csvFile.value != null : sourceInput.value.trim().length > 0
})
const statusMessage = computed(() => {
  switch (jobStatus.value) {
    case 'queued':
      return 'Queued — waiting for a free slot…'
    case 'running':
      return 'Importing from Archidekt… this can take a couple of minutes (we throttle requests to respect their rate limit).'
    default:
      return null
  }
})

// Human-readable form of the upload size cap, for the too-large message.
const maxCsvMb = Math.round(MAX_CSV_UPLOAD_BYTES / (1024 * 1024))

async function runImport() {
  if (!canSubmit.value) return
  if (sourceType.value === 'csv') {
    await runCsvImport()
    return
  }
  enqueuing.value = true
  errorMessage.value = null
  result.value = null
  jobId.value = null
  const trimmed = sourceInput.value.trim()
  try {
    const job = await importMutation.mutateAsync({
      game: props.game,
      provider: provider.value,
      source: trimmed,
      mode: mode.value,
    })
    // Start polling this job; the summary/error arrive via the watcher above.
    jobId.value = job.job_id
    // Saving the link doesn't touch the provider, so do it now (optional). A save
    // failure is a non-blocking warning — the import still runs.
    if (saveLink.value) {
      try {
        await saveMutation.mutateAsync({
          game: props.game,
          provider: provider.value,
          source: trimmed,
        })
      } catch (err) {
        errorMessage.value =
          err instanceof ApiError
            ? `Couldn't save the link: ${err.message}`
            : "Couldn't save the link for re-syncing."
      }
    }
  } catch (err) {
    errorMessage.value = err instanceof ApiError ? err.message : 'Import failed. Please try again.'
  } finally {
    enqueuing.value = false
  }
}

// Upload + reconcile a CSV. Synchronous server-side (no upstream fetch), so the summary
// comes straight back — no job to poll. The server enforces the real limits; this only
// pre-checks the size for a friendlier message than a bare 413.
async function runCsvImport() {
  const file = csvFile.value
  if (!file) return
  if (file.size > MAX_CSV_UPLOAD_BYTES) {
    errorMessage.value =
      `That file is larger than ${maxCsvMb} MB. Re-export from Archidekt with only the ` +
      'Scryfall ID, Finish, and Quantity columns — that keeps it well under the limit.'
    return
  }
  errorMessage.value = null
  result.value = null
  try {
    const summary = await importCsvMutation.mutateAsync({
      game: props.game,
      file,
      mode: mode.value,
    })
    result.value = summary
    // The collection contents changed — refresh the grid, header, and card steppers.
    invalidateCollectionData(qc, props.game)
  } catch (err) {
    errorMessage.value = err instanceof ApiError ? err.message : 'Import failed. Please try again.'
  }
}

async function removeLink() {
  errorMessage.value = null
  try {
    await deleteMutation.mutateAsync({ game: props.game })
    saveLink.value = false
  } catch (err) {
    errorMessage.value = err instanceof ApiError ? err.message : 'Could not remove the saved link.'
  }
}

// Human-readable summary lines for the result panel.
const resultLines = computed(() => {
  const s = result.value
  if (!s) return []
  const lines: string[] = []
  const copies = s.regular_copies + s.foil_copies
  lines.push(
    `Imported ${s.matched_cards.toLocaleString()} card${s.matched_cards === 1 ? '' : 's'} ` +
      `(${copies.toLocaleString()} cop${copies === 1 ? 'y' : 'ies'}).`,
  )
  if (s.unmatched_cards > 0) {
    lines.push(
      `${s.unmatched_cards.toLocaleString()} card${s.unmatched_cards === 1 ? '' : 's'} ` +
        'weren’t in our catalog and were skipped.',
    )
  }
  if (s.removed_cards > 0) {
    lines.push(
      `${s.removed_cards.toLocaleString()} card${s.removed_cards === 1 ? '' : 's'} ` +
        'removed to mirror the list.',
    )
  }
  return lines
})

const selectClass =
  'border-input dark:bg-input/30 flex h-9 w-full rounded-md border bg-transparent px-3 text-sm ' +
  'shadow-xs outline-none focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px]'
</script>

<template>
  <Dialog v-model:open="open">
    <DialogTrigger :class="buttonVariants({ variant: 'outline', size: 'sm' })">
      <Download />
      Import from {{ providerLabel }}
    </DialogTrigger>

    <DialogContent
      class="bg-background max-h-[90vh] w-[min(92vw,32rem)] overflow-y-auto rounded-xl border p-6 shadow-xl"
    >
      <DialogTitle class="text-lg font-semibold">Import from {{ providerLabel }}</DialogTitle>
      <DialogDescription class="text-muted-foreground mt-1 text-sm">
        <template v-if="sourceType === 'link'">
          Paste a public collection URL (or id) and choose how to reconcile it with your collection.
          We fetch it server-side — nothing is uploaded from your device.
        </template>
        <template v-else>
          Upload your exported {{ providerLabel }} collection CSV and choose how to reconcile it
          with your collection.
        </template>
      </DialogDescription>

      <div class="mt-5 space-y-5">
        <!-- Source: paste a link vs upload a CSV file. -->
        <div class="bg-muted grid grid-cols-2 gap-1 rounded-lg p-1" role="tablist">
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
            Upload a CSV
          </button>
        </div>

        <!-- Link tab: provider + collection URL/id. -->
        <template v-if="sourceType === 'link'">
          <div class="space-y-1.5">
            <Label for="import-provider">Provider</Label>
            <select id="import-provider" v-model="provider" :class="selectClass">
              <option v-for="p in PROVIDERS" :key="p.value" :value="p.value">{{ p.label }}</option>
            </select>
            <p class="text-muted-foreground text-xs">More providers (e.g. Moxfield) are coming.</p>
          </div>

          <div class="space-y-1.5">
            <Label for="import-source">Collection URL or id</Label>
            <Input
              id="import-source"
              v-model="sourceInput"
              placeholder="https://archidekt.com/collection/v2/1042487"
            />
          </div>
        </template>

        <!-- CSV tab: file picker + which columns to export. -->
        <template v-else>
          <div class="space-y-2">
            <Label for="import-csv">Collection CSV file</Label>
            <input
              id="import-csv"
              type="file"
              accept=".csv,text/csv"
              class="border-input dark:bg-input/30 file:bg-muted file:text-foreground block w-full cursor-pointer rounded-md border bg-transparent text-sm file:mr-3 file:cursor-pointer file:border-0 file:px-3 file:py-2 file:text-sm file:font-medium focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px] focus-visible:outline-none"
              @change="onCsvFileChange"
            />
            <div class="bg-muted/60 text-muted-foreground rounded-md p-3 text-xs">
              <p class="text-foreground font-medium">How to export from {{ providerLabel }}</p>
              <p class="mt-1">
                In {{ providerLabel }}, open your collection and choose Export → CSV. You only need
                these three columns — you can leave the rest unchecked:
              </p>
              <ul class="mt-1.5 flex flex-wrap gap-1.5">
                <li class="bg-background rounded border px-1.5 py-0.5 font-medium">Scryfall ID</li>
                <li class="bg-background rounded border px-1.5 py-0.5 font-medium">Finish</li>
                <li class="bg-background rounded border px-1.5 py-0.5 font-medium">Quantity</li>
              </ul>
            </div>
          </div>
        </template>

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

        <!-- Save the link (link tab only — an uploaded CSV has nothing to re-sync from). -->
        <div v-if="sourceType === 'link'">
          <label class="flex cursor-pointer items-center gap-2 text-sm">
            <input v-model="saveLink" type="checkbox" />
            Remember this link for one-click re-syncing
          </label>
          <p v-if="saveLink" class="text-muted-foreground mt-1 text-xs">
            Saved links re-sync by mirroring (replace).
          </p>
        </div>

        <!-- In-progress status (queued / running) -->
        <p
          v-if="statusMessage"
          class="text-muted-foreground flex items-start gap-2 text-sm"
          aria-live="polite"
        >
          <LoaderCircle class="mt-0.5 size-4 shrink-0 animate-spin" />
          <span>{{ statusMessage }}</span>
        </p>

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

      <div class="mt-6 flex items-center justify-between gap-3">
        <Button
          v-if="props.source"
          variant="ghost"
          size="sm"
          class="text-destructive hover:text-destructive"
          :disabled="deleteMutation.isPending.value"
          @click="removeLink"
        >
          <Trash2 />
          Remove saved link
        </Button>
        <span v-else />

        <div class="flex gap-2">
          <DialogClose :class="buttonVariants({ variant: 'outline' })">Close</DialogClose>
          <Button :disabled="!canSubmit" @click="runImport">
            {{ busy ? 'Working…' : 'Import' }}
          </Button>
        </div>
      </div>
    </DialogContent>
  </Dialog>
</template>
