<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
import { Download, LoaderCircle, Trash2, TriangleAlert } from '@lucide/vue'
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
import type { CollectionProvider, CollectionSource, ReconcileMode } from '@/lib/api'

// The single management surface for importing a collection from an external provider:
// a one-off import (with a chosen reconcile mode), optionally saving the link for
// one-click re-syncing, and removing a saved link. The reka dialog gives us a focus
// trap, Escape-to-close, and click-outside dismissal for free. Signed-in only — the
// parent view mounts this only when the visitor is authenticated.
const props = defineProps<{ game: string; source: CollectionSource | null }>()

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

/** The saved source's provider, when it's one we can still import from (a stored id is a
 *  plain string). A disabled provider (e.g. a legacy Moxfield link) falls through so the
 *  picker defaults to an enabled provider instead of pre-selecting an unselectable option. */
function savedProvider(source: CollectionSource | null): CollectionProvider | null {
  const known = PROVIDERS.find((p) => p.value === source?.provider && !p.disabled)
  return known?.value ?? null
}

/** The URL to prefill the source field with. Only when the saved link's provider is still
 *  selectable here — otherwise the provider falls back (e.g. a legacy Moxfield link drops to
 *  Archidekt), and prefilling that provider's field with a Moxfield URL would be a confusing
 *  mismatch, so we start blank to match the provider fallback. */
function savedSourceUrl(source: CollectionSource | null): string {
  return savedProvider(source) ? (source?.url ?? '') : ''
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
  {
    value: 'smart',
    label: 'Smart sync',
    hint: 'Only update recently-changed cards (fast). Won’t remove cards deleted on the provider.',
  },
]

// Three ways in: paste a public collection link (fetched server-side, async), upload an
// exported file, or paste the export's text straight in. The last two are inherently
// one-off — there's no location to re-sync from — so the "save link" affordance only
// applies to the link tab.
//
// The paste tab exists because not every service has an API or a browser-friendly export:
// Mythic Tools (issue #572) is a phone app, and pasting what you copied out of it is much
// less friction than saving a file and finding it in a file picker.
type SourceType = 'link' | 'csv' | 'text'

const open = ref(false)
const sourceType = ref<SourceType>('link')
const provider = ref<CollectionProvider>(savedProvider(props.source) ?? 'archidekt')
const sourceInput = ref(savedSourceUrl(props.source))
const mode = ref<ReconcileMode>('overwrite')
const saveLink = ref(props.source != null)
// Whether a saved link re-syncs with smart sync. Kept separate from the one-off `mode`
// so re-importing a smart-saved link with a different mode doesn't silently downgrade it.
const smartResync = ref(props.source?.smart ?? false)
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
  deletePending,
  resetStatus,
  runLinkImport,
  runCsvImport,
  runTextImport,
  removeLink: removeSavedLink,
} = useCollectionImport(gameRef)

// Reset the form each time the dialog opens: seed the provider/URL/checkbox from the
// current saved link and clear any leftover status from a previous session (the
// component instance persists across opens and across game switches).
watch(open, (isOpen) => {
  if (!isOpen) return
  sourceType.value = 'link'
  provider.value = savedProvider(props.source) ?? 'archidekt'
  sourceInput.value = savedSourceUrl(props.source)
  mode.value = 'overwrite'
  saveLink.value = props.source != null
  // Seed the saved-link re-sync preference from the existing link (defaults off).
  smartResync.value = props.source?.smart ?? false
  csvFile.value = null
  pastedText.value = ''
  resetStatus()
})

// Choosing the smart one-off mode implies wanting smart re-syncs too; picking another
// mode never forces it off (it stays whatever the saved link had / the user set).
watch(mode, (m) => {
  if (m === 'smart') smartResync.value = true
})

// Switching tabs clears the previous tab's outcome/error so stale feedback never lingers.
// Also drop any chosen file: the file tab's input is remounted on the way back (v-if), so
// it renders empty — clearing csvFile keeps Import's enabled state honest (no silently
// staged, no-longer-visible upload). Pasted text survives the switch: it's still visible
// when you come back, so it can't become a hidden staged import.
watch(sourceType, (type) => {
  resetStatus()
  csvFile.value = null
  // Smart isn't offered for an upload or a paste (there's no fetch to order / stop), so
  // drop back to a valid mode when leaving the link tab.
  if (type !== 'link' && mode.value === 'smart') mode.value = 'overwrite'
})

function onCsvFile(file: File | null) {
  csvFile.value = file
  resetStatus()
}

// Smart is a link-only mode (it needs the newest-first fetch to stop early); an upload or
// a paste has no fetch, so it's hidden on those tabs.
const visibleModes = computed(() =>
  sourceType.value === 'link' ? MODES : MODES.filter((m) => m.value !== 'smart'),
)

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
    save: saveLink.value,
    smart: smartResync.value,
  })
}

async function removeLink() {
  if (await removeSavedLink()) saveLink.value = false
}

// Human-readable summary lines for the result panel (shared, testable formatter).
const resultLines = computed(() => (result.value ? formatImportSummaryLines(result.value) : []))

// The re-sync behaviour the saved link will use, tailored to the smart-resync toggle.
const savedResyncHint = computed(() =>
  smartResync.value
    ? 'Re-syncs update recently-changed cards only (won’t remove deleted cards).'
    : 'Re-syncs mirror the list exactly (removes cards no longer in it).',
)

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
            v-for="m in visibleModes"
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
        <div v-if="sourceType === 'link'" class="space-y-2">
          <label class="flex cursor-pointer items-center gap-2 text-sm">
            <input v-model="saveLink" type="checkbox" />
            Remember this link for one-click re-syncing
          </label>
          <template v-if="saveLink">
            <label class="flex cursor-pointer items-center gap-2 pl-6 text-sm">
              <input v-model="smartResync" type="checkbox" />
              Re-sync with smart sync
            </label>
            <p class="text-muted-foreground pl-6 text-xs">{{ savedResyncHint }}</p>
          </template>
        </div>

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

      <div class="mt-6 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <Button
          v-if="props.source"
          variant="ghost"
          size="sm"
          class="text-destructive hover:text-destructive"
          :disabled="deletePending"
          @click="removeLink"
        >
          <Trash2 />
          Remove saved link
        </Button>
        <span v-else class="hidden sm:block" />

        <div class="flex justify-end gap-2">
          <DialogClose :class="buttonVariants({ variant: 'outline' })">Close</DialogClose>
          <Button :disabled="!canSubmit" @click="runImport">
            {{ busy ? 'Working…' : 'Import' }}
          </Button>
        </div>
      </div>
    </DialogContent>
  </Dialog>
</template>
