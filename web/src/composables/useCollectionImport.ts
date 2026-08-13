import { computed, ref, watch, type Ref } from 'vue'
import { useQueryClient } from '@tanstack/vue-query'
import {
  getImportJob,
  importCollection,
  importCollectionCsv,
  importCollectionText,
  ApiError,
  MAX_CSV_UPLOAD_BYTES,
  PROVIDER_LABELS,
  type CollectionProvider,
  type ImportJob,
  type ImportSummary,
  type ReconcileMode,
} from '@/lib/api'
import { invalidateCollectionData } from '@/composables/useCollection'
import { useAuthedMutation, useAuthedQuery } from '@/lib/queries'

// Import from an external collection provider (Archidekt or Moxfield). Every import is
// one-off — nothing is remembered between them.
// The low-level vue-query hooks for each import endpoint are internal plumbing; the
// module's public surface is two higher-level composables layered on them:
// `usePolledImportJob` (the shared job-poll-to-terminal plumbing) and `useCollectionImport`
// (the dialog's whole link/file/paste lifecycle). The read side of the collection stays in
// `useCollection`; this depends on it only for `invalidateCollectionData`.

/** Variables for a one-off import. */
interface ImportCollectionVars {
  game: string
  provider: CollectionProvider
  source: string
  mode: ReconcileMode
}

/**
 * Enqueue a one-off import from a provider. Resolves to a job to poll (via
 * {@link useImportJobQuery}); the collection caches are invalidated only once that job
 * completes, so nothing is invalidated here.
 */
function useImportCollectionMutation() {
  const options = {
    mutationFn: (token: string, vars: ImportCollectionVars) =>
      importCollection(token, vars.game, {
        provider: vars.provider,
        source: vars.source,
        mode: vars.mode,
      }),
  }
  return useAuthedMutation<ImportJob, ImportCollectionVars>(options)
}

/** Variables for a CSV upload import: the file and how to reconcile it. */
interface ImportCsvVars {
  game: string
  file: File
  mode: ReconcileMode
}

/**
 * Import a collection from an uploaded export file (the server detects the format from
 * the content). Resolves **synchronously** to an {@link ImportSummary} (the upload needs
 * no upstream fetch, so there's no job to poll); the caller invalidates the collection
 * caches on success.
 */
function useImportCollectionCsvMutation() {
  const options = {
    mutationFn: (token: string, vars: ImportCsvVars) =>
      importCollectionCsv(token, vars.game, vars.file, vars.mode),
  }
  return useAuthedMutation<ImportSummary, ImportCsvVars>(options)
}

/** Variables for a pasted-text import: the pasted list and how to reconcile it. */
interface ImportTextVars {
  game: string
  text: string
  mode: ReconcileMode
}

/**
 * Import a collection from pasted text (a card list, or a pasted CSV export). Same
 * synchronous {@link ImportSummary} as the upload — only how the content reaches the
 * server differs.
 */
function useImportCollectionTextMutation() {
  const options = {
    mutationFn: (token: string, vars: ImportTextVars) =>
      importCollectionText(token, vars.game, vars.text, vars.mode),
  }
  return useAuthedMutation<ImportSummary, ImportTextVars>(options)
}

/**
 * Poll a background import job until it reaches a terminal status. Enabled only while
 * `jobId` is set; refetches every 2s while `queued`/`running`, then stops.
 */
function useImportJobQuery(game: Ref<string>, jobId: Ref<number | null>) {
  const options = {
    queryKey: ['import-job', game, jobId],
    queryFn: (token: string) => getImportJob(token, game.value, jobId.value as number),
    enabled: computed(() => jobId.value != null),
    refetchInterval: (query: { state: { data?: ImportJob } }) => {
      const status = query.state.data?.status
      return status === 'queued' || status === 'running' ? 2000 : false
    },
    // A job's status is inherently fresh; don't serve a stale cached terminal state.
    staleTime: 0,
    gcTime: 0,
  }
  return useAuthedQuery<ImportJob>(options)
}

/**
 * The shared job-poll-to-terminal plumbing for a background import job: owns the polled
 * `jobId`, exposes the live `status`/`processing` flags, and fires the given
 * terminal-status handlers once (from a single guarded watcher). The caller supplies its
 * own copy for the `running`/`complete`/`error` transitions, so the watcher boilerplate
 * lives in one place.
 *
 * `start(id)` begins polling a freshly-enqueued job; `reset()` stops (before a new run).
 */
export function usePolledImportJob(
  game: Ref<string>,
  handlers: {
    onRunning?: () => void
    onComplete?: (summary: ImportSummary | null) => void
    onError?: (error: string | undefined) => void
  } = {},
) {
  const jobId = ref<number | null>(null)
  const jobQuery = useImportJobQuery(game, jobId)
  const status = computed(() => jobQuery.data.value?.status ?? null)
  const processing = computed(() => status.value === 'queued' || status.value === 'running')
  // Live fetch progress while running (rows fetched / total), for the progress bar; the
  // server only sends it in the `running` phase, so it's null when queued/terminal.
  const progress = computed(() => jobQuery.data.value?.progress ?? null)

  watch(
    () => jobQuery.data.value,
    (job) => {
      if (!job) return
      if (job.status === 'running') handlers.onRunning?.()
      else if (job.status === 'complete') handlers.onComplete?.(job.summary ?? null)
      else if (job.status === 'error') handlers.onError?.(job.error)
    },
  )

  return {
    jobId,
    status,
    processing,
    progress,
    start(id: number) {
      jobId.value = id
    },
    reset() {
      jobId.value = null
    },
  }
}

/** Human-readable form of the CSV upload size cap, for the too-large pre-check message. */
const MAX_CSV_MB = Math.round(MAX_CSV_UPLOAD_BYTES / (1024 * 1024))

/**
 * The whole import lifecycle behind the import dialog: the link/file/paste mutations, the
 * polled background job, and the busy/status/error/result state the dialog renders. The
 * dialog keeps the form refs (URL, mode, chosen file) and its own `canSubmit` (which reads
 * both those refs and `busy`); this owns everything downstream of "the user pressed
 * Import".
 *
 * `runLinkImport` enqueues a provider import and starts polling; `runCsvImport` uploads a
 * file and `runTextImport` sends pasted text (both synchronous, no job). `resetStatus`
 * clears the outcome (used by the dialog's open/tab watchers).
 */
export function useCollectionImport(game: Ref<string>) {
  const qc = useQueryClient()
  const importMutation = useImportCollectionMutation()
  const importCsvMutation = useImportCollectionCsvMutation()
  const importTextMutation = useImportCollectionTextMutation()

  const enqueuing = ref(false)
  const errorMessage = ref<string | null>(null)
  const result = ref<ImportSummary | null>(null)

  const job = usePolledImportJob(game, {
    onComplete: (summary) => {
      result.value = summary
      // The collection contents changed — refresh the grid, header, and card steppers.
      invalidateCollectionData(qc, game.value)
    },
    onError: (error) => {
      errorMessage.value = error ?? 'Import failed. Please try again.'
    },
  })

  const processing = job.processing
  const busy = computed(
    () =>
      enqueuing.value ||
      processing.value ||
      importCsvMutation.isPending.value ||
      importTextMutation.isPending.value,
  )
  // Which provider the in-flight link import targets, for the status copy (set when an
  // import is enqueued; the job itself doesn't echo the provider back).
  const activeProvider = ref<CollectionProvider>('archidekt')
  const statusMessage = computed(() => {
    switch (job.status.value) {
      case 'queued':
        return 'Queued — waiting for a free slot…'
      case 'running':
        return `Importing from ${PROVIDER_LABELS[activeProvider.value]}… this can take a couple of minutes (we throttle requests to respect their rate limit).`
      default:
        return null
    }
  })

  function resetStatus() {
    errorMessage.value = null
    result.value = null
    job.reset()
  }

  async function runLinkImport(args: {
    provider: CollectionProvider
    source: string
    mode: ReconcileMode
  }) {
    enqueuing.value = true
    activeProvider.value = args.provider
    resetStatus()
    try {
      const enqueued = await importMutation.mutateAsync({
        game: game.value,
        provider: args.provider,
        source: args.source,
        mode: args.mode,
      })
      // Start polling this job; the summary/error arrive via the job handlers above.
      job.start(enqueued.job_id)
    } catch (err) {
      errorMessage.value =
        err instanceof ApiError ? err.message : 'Import failed. Please try again.'
    } finally {
      enqueuing.value = false
    }
  }

  async function runCsvImport(args: { file: File; mode: ReconcileMode }) {
    // The server enforces the real limits; this only pre-checks the size for a friendlier
    // message than a bare 413 (and leaves any prior outcome visible if it's rejected here).
    if (args.file.size > MAX_CSV_UPLOAD_BYTES) {
      errorMessage.value =
        `That file is larger than ${MAX_CSV_MB} MB. If it came from Archidekt, re-export ` +
        'with only the Scryfall ID, Finish, and Quantity columns — that keeps it well ' +
        "under the limit. (Moxfield's standard export is already compact.)"
      return
    }
    errorMessage.value = null
    result.value = null
    try {
      const summary = await importCsvMutation.mutateAsync({
        game: game.value,
        file: args.file,
        mode: args.mode,
      })
      result.value = summary
      // The collection contents changed — refresh the grid, header, and card steppers.
      invalidateCollectionData(qc, game.value)
    } catch (err) {
      errorMessage.value =
        err instanceof ApiError ? err.message : 'Import failed. Please try again.'
    }
  }

  async function runTextImport(args: { text: string; mode: ReconcileMode }) {
    // Same server cap as an upload; measured in UTF-8 bytes, which is what's actually
    // sent (a naive `.length` would under-count non-ASCII card names).
    const bytes = new TextEncoder().encode(args.text).length
    if (bytes > MAX_CSV_UPLOAD_BYTES) {
      errorMessage.value = `That list is larger than ${MAX_CSV_MB} MB. Import it in smaller batches.`
      return
    }
    errorMessage.value = null
    result.value = null
    try {
      result.value = await importTextMutation.mutateAsync({
        game: game.value,
        text: args.text,
        mode: args.mode,
      })
      // The collection contents changed — refresh the grid, header, and card steppers.
      invalidateCollectionData(qc, game.value)
    } catch (err) {
      errorMessage.value =
        err instanceof ApiError ? err.message : 'Import failed. Please try again.'
    }
  }

  return {
    errorMessage,
    result,
    busy,
    processing,
    progress: job.progress,
    statusMessage,
    resetStatus,
    runLinkImport,
    runCsvImport,
    runTextImport,
  }
}
