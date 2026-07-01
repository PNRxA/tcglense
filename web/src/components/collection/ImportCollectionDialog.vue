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
  useImportCollectionMutation,
  useImportJobQuery,
  useSaveCollectionSourceMutation,
} from '@/composables/useCollection'
import { ApiError } from '@/lib/api'
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

const open = ref(false)
const provider = ref<CollectionProvider>('archidekt')
const sourceInput = ref(props.source?.url ?? '')
const mode = ref<ReconcileMode>('overwrite')
const saveLink = ref(props.source != null)

const gameRef = toRef(props, 'game')
const qc = useQueryClient()
const importMutation = useImportCollectionMutation()
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
  provider.value = 'archidekt'
  sourceInput.value = props.source?.url ?? ''
  mode.value = 'overwrite'
  saveLink.value = props.source != null
  errorMessage.value = null
  result.value = null
  jobId.value = null
  enqueuing.value = false
})

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
const busy = computed(() => enqueuing.value || processing.value)
const canSubmit = computed(() => sourceInput.value.trim().length > 0 && !busy.value)
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

async function runImport() {
  if (!canSubmit.value) return
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
        Paste a public collection URL (or id) and choose how to reconcile it with your collection.
        We fetch it server-side — nothing is uploaded from your device.
      </DialogDescription>

      <div class="mt-5 space-y-5">
        <!-- Provider -->
        <div class="space-y-1.5">
          <Label for="import-provider">Provider</Label>
          <select id="import-provider" v-model="provider" :class="selectClass">
            <option v-for="p in PROVIDERS" :key="p.value" :value="p.value">{{ p.label }}</option>
          </select>
          <p class="text-muted-foreground text-xs">More providers (e.g. Moxfield) are coming.</p>
        </div>

        <!-- Source URL / id -->
        <div class="space-y-1.5">
          <Label for="import-source">Collection URL or id</Label>
          <Input
            id="import-source"
            v-model="sourceInput"
            placeholder="https://archidekt.com/collection/v2/1042487"
          />
        </div>

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

        <!-- Save the link -->
        <div>
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
