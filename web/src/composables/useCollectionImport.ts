import { computed, ref, watch, type Ref } from 'vue'
import { useQueryClient } from '@tanstack/vue-query'
import {
  deleteCollectionSource,
  getCollectionSource,
  getImportJob,
  importCollection,
  importCollectionCsv,
  saveCollectionSource,
  syncCollectionSource,
  ApiError,
  MAX_CSV_UPLOAD_BYTES,
  PROVIDER_LABELS,
  type CollectionProvider,
  type CollectionSource,
  type ImportJob,
  type ImportSummary,
  type ReconcileMode,
} from '@/lib/api'
import { invalidateCollectionData } from '@/composables/useCollection'
import { useAuthedMutation, useAuthedQuery } from '@/lib/queries'

// Import / sync from an external collection provider (Archidekt or Moxfield).
// The low-level vue-query hooks for each import/sync endpoint are internal plumbing; the
// module's public surface is `useCollectionSourceQuery` + `useSyncCollectionSourceMutation`
// (the saved-link read/re-sync the collection landing uses) plus two higher-level
// composables layered on the internal hooks: `usePolledImportJob` (the shared
// job-poll-to-terminal plumbing both the import dialog and the collection landing use) and
// `useCollectionImport` (the dialog's whole link/CSV/save lifecycle). The read side of the
// collection stays in `useCollection`; this depends on it only for `invalidateCollectionData`.

/**
 * The user's saved external collection link for a game (or null). Drives the
 * "Re-sync" affordance and prefills the import dialog. Disabled while signed out.
 */
export function useCollectionSourceQuery(game: Ref<string>) {
  const options = {
    queryKey: ['collection-source', game],
    queryFn: (token: string) => getCollectionSource(token, game.value),
  }
  return useAuthedQuery<CollectionSource | null>(options)
}

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
 * Import a collection from an uploaded CSV export (Archidekt or Moxfield — the server
 * detects which from the header row). Resolves **synchronously**
 * to an {@link ImportSummary} (the CSV needs no upstream fetch, so there's no job to
 * poll); the caller invalidates the collection caches on success.
 */
function useImportCollectionCsvMutation() {
  const options = {
    mutationFn: (token: string, vars: ImportCsvVars) =>
      importCollectionCsv(token, vars.game, vars.file, vars.mode),
  }
  return useAuthedMutation<ImportSummary, ImportCsvVars>(options)
}

/**
 * Poll a background import/sync job until it reaches a terminal status. Enabled only
 * while `jobId` is set; refetches every 2s while `queued`/`running`, then stops.
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

/** Variables for saving a collection link. */
interface SaveSourceVars {
  game: string
  provider: CollectionProvider
  source: string
  /** Whether saved re-syncs should use smart (incremental) sync. */
  smart?: boolean
}

/** Save (upsert) the collection link; invalidates the saved-source query. */
function useSaveCollectionSourceMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: SaveSourceVars) =>
      saveCollectionSource(token, vars.game, {
        provider: vars.provider,
        source: vars.source,
        smart: vars.smart,
      }),
    onSettled: (
      _data: CollectionSource | undefined,
      _error: ApiError | null,
      vars: SaveSourceVars,
    ) => {
      qc.invalidateQueries({ queryKey: ['collection-source', vars.game] })
    },
  }
  return useAuthedMutation<CollectionSource, SaveSourceVars>(options)
}

/** Forget the saved collection link; invalidates the saved-source query. */
function useDeleteCollectionSourceMutation() {
  const qc = useQueryClient()
  const options = {
    mutationFn: (token: string, vars: { game: string }) => deleteCollectionSource(token, vars.game),
    onSettled: (_data: void | undefined, _error: ApiError | null, vars: { game: string }) => {
      qc.invalidateQueries({ queryKey: ['collection-source', vars.game] })
    },
  }
  return useAuthedMutation<void, { game: string }>(options)
}

/**
 * Enqueue a re-sync from the saved link (mirror/replace). Resolves to a job to poll; the
 * collection + saved-source caches are invalidated once that job completes (the caller
 * does this on completion, via {@link invalidateCollectionData} + the source query).
 */
export function useSyncCollectionSourceMutation() {
  const options = {
    mutationFn: (token: string, vars: { game: string }) => syncCollectionSource(token, vars.game),
  }
  return useAuthedMutation<ImportJob, { game: string }>(options)
}

/**
 * The shared job-poll-to-terminal plumbing for a background import/sync job: owns the
 * polled `jobId`, exposes the live `status`/`processing` flags, and fires the given
 * terminal-status handlers once (from a single guarded watcher). Both the import dialog
 * and the collection landing's re-sync build on it — each supplies its own copy for the
 * `running`/`complete`/`error` transitions (the summary/error shapes differ), so the
 * watcher boilerplate lives in one place.
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
 * The whole import lifecycle behind the import dialog: the link/CSV/save mutations, the
 * polled background job, and the busy/status/error/result state the dialog renders. The
 * dialog keeps the form refs (URL, mode, save toggles, chosen file) and its own
 * `canSubmit` (which reads both those refs and `busy`); this owns everything downstream of
 * "the user pressed Import".
 *
 * `runLinkImport` enqueues a provider import (optionally saving the link) and starts
 * polling; `runCsvImport` uploads a CSV (synchronous, no job). `removeLink` deletes the
 * saved link and returns whether it succeeded (so the dialog can un-check "save"), and
 * `resetStatus` clears the outcome (used by the dialog's open/tab watchers).
 */
export function useCollectionImport(game: Ref<string>) {
  const qc = useQueryClient()
  const importMutation = useImportCollectionMutation()
  const importCsvMutation = useImportCollectionCsvMutation()
  const saveMutation = useSaveCollectionSourceMutation()
  const deleteMutation = useDeleteCollectionSourceMutation()

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
    () => enqueuing.value || processing.value || importCsvMutation.isPending.value,
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
    save: boolean
    smart: boolean
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
      // Saving the link doesn't touch the provider, so do it now (optional). A save
      // failure is a non-blocking warning — the import still runs.
      if (args.save) {
        try {
          await saveMutation.mutateAsync({
            game: game.value,
            provider: args.provider,
            source: args.source,
            smart: args.smart,
          })
        } catch (err) {
          errorMessage.value =
            err instanceof ApiError
              ? `Couldn't save the link: ${err.message}`
              : "Couldn't save the link for re-syncing."
        }
      }
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

  async function removeLink(): Promise<boolean> {
    errorMessage.value = null
    try {
      await deleteMutation.mutateAsync({ game: game.value })
      return true
    } catch (err) {
      errorMessage.value =
        err instanceof ApiError ? err.message : 'Could not remove the saved link.'
      return false
    }
  }

  return {
    errorMessage,
    result,
    busy,
    processing,
    progress: job.progress,
    statusMessage,
    deletePending: deleteMutation.isPending,
    resetStatus,
    runLinkImport,
    runCsvImport,
    removeLink,
  }
}
