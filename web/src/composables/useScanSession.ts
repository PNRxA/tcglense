import { computed, reactive, ref, watch, type Ref } from 'vue'
import type { Card, CollectionQuantities, ScanMatch as ApiScanMatch } from '@/lib/api'
import { usePrintingPicker } from '@/composables/usePrintings'
import { useCollectionEntryQuery, useSetCollectionEntryMutation } from '@/composables/useCollection'
import { useScanMutation } from '@/composables/useScan'
import { matchPrinting } from '@/lib/scan/match'
import { parseSetHint, type SetHint } from '@/lib/scan/ocr'
import type { ScanCapture } from '@/composables/useCardScanner'

// Orchestrates one scanning session: turn a captured card into a confirmed catalog match,
// let the user tweak the printing/counts, and commit it to the collection when the *next*
// card is shown (the rapid-add rhythm). It deliberately does NOT use useOwnedCountEditor:
// that editor auto-saves 350ms after each change, but a scanner must stay tentative until
// the next card appears, and its card-switch flush would mis-commit a printing correction.
// So the write goes through useSetCollectionEntryMutation directly (which still handles all
// cache invalidation) at exactly the moment we advance.

/** How the resolved match is being edited before it's committed. */
export interface ScanMatch {
  /** The visually-matched card name (the "read as" hint shown to the user). */
  ocrName: string
  /** Set/collector hints parsed from the OCR'd bottom strip (printing pre-select). */
  hint: SetHint
  /** Catalog name candidates (the distinct names of the top visual matches) the user
   * can correct the pick to. */
  candidates: string[]
  /** The chosen catalog name (drives the printings query). */
  name: string
}

/** A card committed during this session, with the counts before it for one-tap undo. */
export interface SessionEntry {
  /** Stable per-entry id — the log's v-for key, so the unshift on each new commit doesn't
   * remount every row (an index-based key would). */
  id: number
  card: Card
  quantity: number
  foil_quantity: number
  previous: CollectionQuantities
}

/** Regular copies a fresh scan proposes — you showed the camera one card. */
const SCANNED_COPIES = 1

/** How many name candidates to keep for the confirm/correct dropdown (from the distinct
 * names of the top visual matches). */
const NAME_CANDIDATE_LIMIT = 8

export type CaptureOutcome = 'matched' | 'same' | 'unmatched' | 'busy'

export function useScanSession(game: Ref<string>) {
  const mutation = useSetCollectionEntryMutation()
  const scanMutation = useScanMutation()

  // Monotonic id per committed entry (stable session-log key across each unshift).
  let nextEntryId = 0
  // A Stop/navigation finalizer can race the auto-advance commit. Share the same promise so
  // both callers wait for one write instead of either double-writing or treating the in-flight
  // commit as a failed/no-op save.
  let commitInFlight: Promise<boolean> | null = null
  let finalizeInFlight: Promise<boolean> | null = null
  let undoInFlight: Promise<boolean> | null = null

  const match = ref<ScanMatch | null>(null)
  const selectedName = ref('')
  const selectedId = ref('')
  const seeded = ref(false)
  const resolving = ref(false)
  const finalizing = ref(false)
  const undoing = ref(false)
  // True when the last capture matched no catalog card (nothing within the visual
  // confidence radius) — drives the "not recognised" nudge.
  const unrecognized = ref(false)
  const commitError = ref(false)
  const log = ref<SessionEntry[]>([])
  // The ranked visual matches from the last capture (nearest first) — shown as a
  // pickable strip so the user can correct a weak/wrong top match by tapping the right
  // card (its art, not just a name).
  const candidates = ref<ApiScanMatch[]>([])

  const actionsLocked = () => finalizing.value || resolving.value || undoing.value

  // Absolute counts to write on commit: owned + the scanned copy, then user-adjustable.
  const target = reactive<CollectionQuantities>({ quantity: 0, foil_quantity: 0 })
  // Frozen baseline that `target` was seeded from. The live owned query can refetch while the
  // user edits; absolute writes and session-log Undo must still compare against the baseline
  // behind the tentative delta. A same-printing Undo rebases both values below.
  const seedBase = reactive<CollectionQuantities>({ quantity: 0, foil_quantity: 0 })

  // Every printing of the chosen name (public read, cached — re-scans are instant).
  const printsEnabled = computed(() => selectedName.value.length > 0)
  const printsPicker = usePrintingPicker(game, selectedName, { enabled: printsEnabled })
  const prints = printsPicker.printings
  // A set-code OCR hint is resolved only after all pages are loaded. Picking against a
  // partial list could incorrectly fuzzy-match a similar set code while the exact old
  // printing sits beyond the first 200 results.
  const resolvingPrintingHint = computed(
    () =>
      !selectedId.value &&
      Boolean(match.value?.hint.setCode) &&
      !printsPicker.failed.value &&
      (printsPicker.hasNextPage.value || printsPicker.isFetchingNextPage.value),
  )
  const printsLoading = computed(
    () => printsEnabled.value && (printsPicker.isPending.value || resolvingPrintingHint.value),
  )
  const printsLoadingMore = computed(() => printsPicker.isFetchingNextPage.value)
  const selectedCard = computed<Card | null>(
    () => prints.value.find((card) => card.id === selectedId.value) ?? null,
  )
  // A failed initial/next-page request must not turn an incomplete list into a valid OCR
  // resolution. The user can retry or explicitly choose one of the loaded printings.
  const printsError = computed(
    () =>
      printsEnabled.value &&
      printsPicker.failed.value &&
      !printsPicker.isFetching.value &&
      !printsPicker.isFetchingNextPage.value,
  )

  // Authoritative owned counts for the selected printing (authed; refetched on each switch
  // so an absolute write never seeds off a stale count — see OwnedCountControl's guard).
  const ownedEnabled = computed(() => selectedId.value.length > 0)
  const entryQuery = useCollectionEntryQuery(game, selectedId, {
    enabled: ownedEnabled,
    staleTime: 0,
  })
  const owned = computed<CollectionQuantities>(
    () => entryQuery.data.value ?? { quantity: 0, foil_quantity: 0 },
  )
  const ownedReady = computed(
    () => ownedEnabled.value && entryQuery.isSuccess.value && !entryQuery.isFetching.value,
  )
  // A terminal failure fetching the current printing's holding. Surfaced (with a retry) so a
  // transient error doesn't silently soft-lock the loop: the seed watch never fires, so the
  // match can neither commit nor be advanced past until this recovers.
  const ownedError = computed(
    () => ownedEnabled.value && entryQuery.isError.value && !entryQuery.isFetching.value,
  )
  function retryOwned() {
    if (actionsLocked()) return
    void entryQuery.refetch()
  }
  function retryPrintings() {
    if (actionsLocked()) return
    if (printsPicker.hasNextPage.value) {
      void printsPicker.loadMore()
      return
    }
    void printsPicker.refetch()
  }
  // The steppers/commit are trustworthy only once the seed has been applied off a settled
  // holding.
  const ready = computed(() => seeded.value)

  // Whether it's safe to commit the current match and advance to a new one: nothing to
  // commit, the holding has seeded, or it settled into a terminal "no printing" state.
  // Guards against a fast double-scan advancing before the previous card's holding loads —
  // its commit would be skipped (unseeded) and the card silently dropped.
  const currentSettled = computed(
    () =>
      !match.value ||
      ready.value ||
      (!printsLoading.value && !printsError.value && !selectedCard.value),
  )

  function applySelectedId(id: string) {
    if (id === selectedId.value) return
    // A different printing must re-seed off its own holding (writes are absolute).
    seeded.value = false
    selectedId.value = id
  }

  function selectId(id: string) {
    if (actionsLocked()) return
    applySelectedId(id)
  }

  // Auto-pick a printing once the needed pages have settled: when OCR supplied a set code,
  // load every page before matching so an old basic-land printing is not hidden beyond 200.
  // Without a set code the newest loaded printing remains the honest default. A manual pick
  // is never overridden.
  watch([selectedName, prints, printsPicker.isPending, printsPicker.isFetchingNextPage], () => {
    if (
      !selectedName.value ||
      printsPicker.isPending.value ||
      printsPicker.isFetchingNextPage.value ||
      !prints.value.length
    ) {
      return
    }
    if (selectedCard.value) return
    if (match.value?.hint.setCode) {
      // The hinted printing may still be outside the partial result set after a failed page.
      // Never fall through to `prints[0]`; retry or a deliberate manual pick is required.
      if (printsPicker.failed.value) return
      if (printsPicker.hasNextPage.value) {
        void printsPicker.loadMore()
        return
      }
    }
    const picked = matchPrinting(prints.value, match.value?.hint ?? {}) ?? prints.value[0]
    if (picked) applySelectedId(picked.id)
  })

  // Seed the target counts off the settled holding, once per printing.
  watch(
    [selectedId, ownedReady],
    () => {
      if (selectedId.value && ownedReady.value && !seeded.value) {
        seedBase.quantity = owned.value.quantity
        seedBase.foil_quantity = owned.value.foil_quantity
        target.quantity = seedBase.quantity + SCANNED_COPIES
        target.foil_quantity = seedBase.foil_quantity
        seeded.value = true
      }
    },
    { immediate: true },
  )

  function startMatch(next: ScanMatch) {
    match.value = next
    unrecognized.value = false
    selectedName.value = next.name
    applySelectedId('')
  }

  /** Switch the resolved name to another candidate (re-resolves printings + counts). */
  function setName(name: string) {
    if (actionsLocked()) return
    if (!match.value || name === match.value.name) return
    match.value = { ...match.value, name }
    selectedName.value = name
    applySelectedId('')
  }

  /** Pick one of the ranked visual candidates (a tap on the pick strip): switch to its
   * name if different, then select that exact printing. */
  function pickCandidate(card: Card) {
    if (actionsLocked()) return
    if (!match.value) return
    if (card.name !== match.value.name) setName(card.name)
    applySelectedId(card.id)
  }

  function adjust(which: 'quantity' | 'foil_quantity', delta: number) {
    if (!seeded.value || actionsLocked()) return
    target[which] = Math.max(0, target[which] + delta)
  }

  function clearCurrent() {
    match.value = null
    selectedName.value = ''
    applySelectedId('')
    seeded.value = false
    commitError.value = false
    candidates.value = []
  }

  /** Drop the on-screen match without adding it (a misread you don't want). */
  function discardCurrent() {
    if (actionsLocked()) return
    clearCurrent()
  }

  /** Explicitly add the current match now, then clear the panel for the next card. On a
   * failed save the panel is kept (with the error) so it can be retried. */
  async function confirmCurrent(): Promise<void> {
    if (actionsLocked()) return
    if (await finalizeCurrent()) clearCurrent()
  }

  /** The distinct card names of the ranked visual matches, best first, capped — the
   * correction dropdown (if the top visual match's name is wrong, a runner-up may be
   * right). */
  function uniqueNames(matches: ApiScanMatch[]): string[] {
    const out: string[] = []
    for (const m of matches) {
      if (!out.includes(m.card.name)) out.push(m.card.name)
      if (out.length >= NAME_CANDIDATE_LIMIT) break
    }
    return out
  }

  /** Write the current match's absolute counts, unless nothing changed. Logs the add with
   * its pre-add counts for undo. Returns whether a write actually happened. */
  function commitCurrent(): Promise<boolean> {
    if (commitInFlight) return commitInFlight
    const card = selectedCard.value
    if (!card || !seeded.value) return Promise.resolve(false)
    const previous = { ...seedBase }
    if (target.quantity === previous.quantity && target.foil_quantity === previous.foil_quantity) {
      return Promise.resolve(false)
    }
    const quantity = target.quantity
    const foilQuantity = target.foil_quantity
    commitInFlight = (async () => {
      try {
        await mutation.mutateAsync({
          game: game.value,
          id: card.id,
          quantity,
          foil_quantity: foilQuantity,
        })
        commitError.value = false
        log.value.unshift({
          id: nextEntryId++,
          card,
          quantity,
          foil_quantity: foilQuantity,
          previous,
        })
        return true
      } catch {
        commitError.value = true
        return false
      } finally {
        commitInFlight = null
      }
    })()
    return commitInFlight
  }

  /** Wait for printing resolution and owned-count seeding, then save the tentative card.
   * Returns whether it is safe for Stop/navigation to discard the local panel. A printing
   * pagination error or holding error returns false and leaves the match visible for retry. */
  async function runFinalizeCurrent(): Promise<boolean> {
    const pendingUndo = undoInFlight
    if (pendingUndo && !(await pendingUndo)) return false
    if (!match.value) return true
    const printingBlocked = () => printsError.value && !selectedCard.value
    if (!currentSettled.value && !ownedError.value && !printingBlocked()) {
      await new Promise<void>((resolve) => {
        const stop = watch([currentSettled, ownedError, printsError, selectedCard, match], () => {
          // A printing error blocks auto-resolution only while no loaded printing has been
          // chosen manually. After a choice, keep waiting for that printing's owned count.
          if (currentSettled.value || ownedError.value || printingBlocked() || !match.value) {
            stop()
            resolve()
          }
        })
      })
    }
    if (!match.value) return true
    if (!ready.value) return false
    await commitCurrent()
    return !commitError.value
  }

  function finalizeCurrent(): Promise<boolean> {
    if (finalizeInFlight) return finalizeInFlight
    if (!match.value && !undoInFlight) return Promise.resolve(true)
    finalizing.value = true
    finalizeInFlight = runFinalizeCurrent().finally(() => {
      finalizing.value = false
      finalizeInFlight = null
    })
    return finalizeInFlight
  }

  /**
   * Process a fresh capture. Identifies the card **visually** (its fingerprint → the
   * match endpoint), and pins the exact printing from the OCR'd set line. If it resolves
   * to a genuinely different card, it commits the one on screen and swaps in the new
   * match; the same card (or an unrecognised one) leaves the current match untouched.
   */
  async function handleCapture(capture: ScanCapture): Promise<CaptureOutcome> {
    // Don't advance past a match whose holding is still loading — commit it first.
    if (finalizing.value || undoing.value || !currentSettled.value) return 'busy'
    resolving.value = true
    try {
      let matches: ApiScanMatch[]
      try {
        const res = await scanMutation.mutateAsync({
          game: game.value,
          fingerprints: capture.fingerprints,
          topK: NAME_CANDIDATE_LIMIT,
        })
        matches = res.data
      } catch {
        // Transient scan failure (offline, or no index on this instance yet): treat as
        // unrecognised and leave the current card untouched; the next capture retries.
        unrecognized.value = true
        return 'unmatched'
      }
      const name = matches[0]?.card.name
      if (!name) {
        unrecognized.value = true
        return 'unmatched'
      }
      // Same card re-scanned: refresh the pickable strip in place — the match is unchanged,
      // so candidates and match stay consistent.
      if (match.value && name === match.value.name) {
        candidates.value = matches
        return 'same'
      }
      await commitCurrent()
      // A failed save keeps the current card on screen rather than silently replacing (and
      // losing) it; the next capture retries the commit once the connection recovers. Leave
      // candidates untouched so the strip keeps showing the retained match, not the (not yet
      // committed) new scan — otherwise a tap on the strip would edit the wrong card.
      if (commitError.value) return 'busy'
      // Swap the strip together with the match so the candidate strip never shows a different
      // card than the match panel.
      candidates.value = matches
      // Visual match gives identity (name); the OCR'd set line pins the printing via the
      // existing hint → matchPrinting flow (see the auto-pick watch above).
      startMatch({
        ocrName: name,
        hint: parseSetHint(capture.setText),
        candidates: uniqueNames(matches),
        name,
      })
      return 'matched'
    } finally {
      resolving.value = false
    }
  }

  /** Revert a logged add back to the counts it had before (both-zero deletes the row). */
  function undo(index: number): Promise<boolean> {
    if (finalizing.value || resolving.value) return Promise.resolve(false)
    if (undoInFlight) return undoInFlight
    const entry = log.value[index]
    if (!entry) return Promise.resolve(false)
    undoing.value = true
    undoInFlight = (async () => {
      try {
        await mutation.mutateAsync({
          game: game.value,
          id: entry.card.id,
          quantity: entry.previous.quantity,
          foil_quantity: entry.previous.foil_quantity,
        })
        // Remove by identity, not the click-time index: a concurrent scan commit can unshift
        // the log during the await, so the numeric index would point at the wrong row by now.
        const at = log.value.indexOf(entry)
        if (at !== -1) log.value.splice(at, 1)
        if (selectedCard.value?.id === entry.card.id && seeded.value) {
          // Keep the tentative edit (normally +1 scanned copy) while moving its absolute
          // baseline to the count restored by Undo. Without this, 1 owned -> tentative 2 ->
          // Undo to 0 would still commit 2, making the Undo ineffective.
          const quantityDelta = target.quantity - seedBase.quantity
          const foilDelta = target.foil_quantity - seedBase.foil_quantity
          seedBase.quantity = entry.previous.quantity
          seedBase.foil_quantity = entry.previous.foil_quantity
          target.quantity = Math.max(0, seedBase.quantity + quantityDelta)
          target.foil_quantity = Math.max(0, seedBase.foil_quantity + foilDelta)
        }
        commitError.value = false
        return true
      } catch {
        commitError.value = true
        return false
      } finally {
        undoing.value = false
        undoInFlight = null
      }
    })()
    return undoInFlight
  }

  const committing = computed(() => mutation.isPending.value)
  const addedCount = computed(() => log.value.length)

  function loadMorePrintings() {
    if (actionsLocked()) return
    void printsPicker.loadMore()
  }

  return {
    // match state
    match,
    prints,
    printsLoading,
    printsLoadingMore,
    printsError,
    printsTotal: printsPicker.total,
    printsHasMore: printsPicker.hasNextPage,
    selectedId,
    selectedCard,
    owned,
    target,
    ready,
    advanceReady: currentSettled,
    resolving,
    finalizing,
    undoing,
    ownedError,
    candidates,
    // session
    log,
    addedCount,
    unrecognized,
    commitError,
    committing,
    // actions
    handleCapture,
    commitCurrent,
    finalizeCurrent,
    confirmCurrent,
    discardCurrent,
    selectId,
    setName,
    adjust,
    undo,
    retryOwned,
    retryPrintings,
    pickCandidate,
    loadMorePrintings,
  }
}
