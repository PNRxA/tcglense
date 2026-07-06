import { computed, reactive, ref, watch, type Ref } from 'vue'
import { getCardNames, type Card, type CollectionQuantities } from '@/lib/api'
import { useCardPrintingsByName } from '@/composables/useQuickAdd'
import { useCollectionEntryQuery, useSetCollectionEntryMutation } from '@/composables/useCollection'
import { matchPrinting } from '@/lib/scan/match'
import { cleanCardName, nameQueryCandidates, parseSetHint, type SetHint } from '@/lib/scan/ocr'
import { deconfuseDigits, namePoolPrefix, ocrSimilarity, rankNames } from '@/lib/scan/similarity'
import type { ScanCapture } from '@/composables/useCardScanner'

// Orchestrates one scanning session: turn an OCR capture into a confirmed catalog match,
// let the user tweak the printing/counts, and commit it to the collection when the *next*
// card is shown (the rapid-add rhythm). It deliberately does NOT use useOwnedCountEditor:
// that editor auto-saves 350ms after each change, but a scanner must stay tentative until
// the next card appears, and its card-switch flush would mis-commit a printing correction.
// So the write goes through useSetCollectionEntryMutation directly (which still handles all
// cache invalidation) at exactly the moment we advance.

/** How the resolved match is being edited before it's committed. */
export interface ScanMatch {
  /** Cleaned OCR title text — drives same-card detection and the "read as" hint. */
  ocrName: string
  /** Set/collector hints parsed from the bottom strip (advisory printing pre-select). */
  hint: SetHint
  /** Catalog name candidates from the autocomplete (the user can correct the pick). */
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

/** How many name candidates to pull for the confirm/correct dropdown. */
const NAME_CANDIDATE_LIMIT = 8

/** A wider pool for the fuzzy recovery query, when no exact substring matched (the server
 * caps this at 25). Ranking then picks the closest read out of the larger net. */
const FUZZY_POOL_LIMIT = 25

/** Lowest OCR-similarity we'll trust for a fuzzy-recovered name. Below this the closest
 * pool entry is more likely noise than the card, so we report it unmatched instead. */
const FUZZY_MATCH_THRESHOLD = 0.62

export type CaptureOutcome = 'matched' | 'same' | 'unmatched' | 'busy'

export function useScanSession(game: Ref<string>) {
  const mutation = useSetCollectionEntryMutation()

  // Monotonic id per committed entry (stable session-log key across each unshift).
  let nextEntryId = 0
  // Guards commitCurrent against a concurrent second invocation (an in-flight auto tick
  // racing the Stop/unmount commit), which would otherwise double-write and double-log.
  let commitInFlight = false

  const match = ref<ScanMatch | null>(null)
  const selectedName = ref('')
  const selectedId = ref('')
  const seeded = ref(false)
  const resolving = ref(false)
  const lastUnmatched = ref<string | null>(null)
  const commitError = ref(false)
  const log = ref<SessionEntry[]>([])

  // Absolute counts to write on commit: owned + the scanned copy, then user-adjustable.
  const target = reactive<CollectionQuantities>({ quantity: 0, foil_quantity: 0 })

  // Every printing of the chosen name (public read, cached — re-scans are instant).
  const printsEnabled = computed(() => selectedName.value.length > 0)
  const printsQuery = useCardPrintingsByName(game, selectedName, { enabled: printsEnabled })
  const prints = computed<Card[]>(() => printsQuery.data.value?.data ?? [])
  const printsLoading = computed(() => printsEnabled.value && printsQuery.isFetching.value)
  const selectedCard = computed<Card | null>(
    () => prints.value.find((card) => card.id === selectedId.value) ?? null,
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
    void entryQuery.refetch()
  }
  // The steppers/commit are trustworthy only once the seed has been applied off a settled
  // holding.
  const ready = computed(() => seeded.value)

  // Whether it's safe to commit the current match and advance to a new one: nothing to
  // commit, the holding has seeded, or it settled into a terminal "no printing" state.
  // Guards against a fast double-scan advancing before the previous card's holding loads —
  // its commit would be skipped (unseeded) and the card silently dropped.
  const currentSettled = computed(
    () => !match.value || ready.value || (!printsLoading.value && !selectedCard.value),
  )

  function selectId(id: string) {
    if (id === selectedId.value) return
    // A different printing must re-seed off its own holding (writes are absolute).
    seeded.value = false
    selectedId.value = id
  }

  // Auto-pick a printing once the name's printings have settled: the set/collector hint's
  // target if it resolves, else the newest (prints are newest-first). Guarded so a manual
  // pick (a now-valid selection) is never overridden, and so we never pick off a list
  // that's still loading for the new name.
  watch([selectedName, prints, printsLoading], () => {
    if (!selectedName.value || printsLoading.value || !prints.value.length) return
    if (selectedCard.value) return
    const picked = matchPrinting(prints.value, match.value?.hint ?? {}) ?? prints.value[0]
    if (picked) selectId(picked.id)
  })

  // Seed the target counts off the settled holding, once per printing.
  watch(
    [selectedId, ownedReady],
    () => {
      if (selectedId.value && ownedReady.value && !seeded.value) {
        target.quantity = owned.value.quantity + SCANNED_COPIES
        target.foil_quantity = owned.value.foil_quantity
        seeded.value = true
      }
    },
    { immediate: true },
  )

  function startMatch(next: ScanMatch) {
    match.value = next
    lastUnmatched.value = null
    selectedName.value = next.name
    selectId('')
  }

  /** Switch the resolved name to another candidate (re-resolves printings + counts). */
  function setName(name: string) {
    if (!match.value || name === match.value.name) return
    match.value = { ...match.value, name }
    selectedName.value = name
    selectId('')
  }

  function adjust(which: 'quantity' | 'foil_quantity', delta: number) {
    if (!seeded.value) return
    target[which] = Math.max(0, target[which] + delta)
  }

  function clearCurrent() {
    match.value = null
    selectedName.value = ''
    selectId('')
    seeded.value = false
    commitError.value = false
  }

  /** Drop the on-screen match without adding it (a misread you don't want). */
  function discardCurrent() {
    clearCurrent()
  }

  async function fetchNames(query: string, limit: number): Promise<string[]> {
    try {
      const { data } = await getCardNames(game.value, query, limit)
      return data
    } catch {
      // Transient failure — the caller moves on to the next (broader) query.
      return []
    }
  }

  /**
   * Resolve an OCR'd title into catalog name candidates, best match first, tolerant of a
   * slightly wrong read. Three tiers, most-confident first:
   *  1. Exact-substring queries — the raw read and its leading-word prefixes, then the same
   *     with OCR digit-for-letter swaps undone (so `Lightn1ng Bolt` still finds the card).
   *     The first that hits is a real substring match, so its pool is trusted; we only
   *     re-rank it so the closest name is the default pick.
   *  2. Recovery — nothing matched as a substring (a *letter* was misread). Pull a wider
   *     pool off a short prefix and keep it only if the closest name clears the confidence
   *     bar, so a genuine misread resolves while noise stays unmatched.
   */
  async function resolveNames(cleaned: string): Promise<string[]> {
    const queries = new Set(nameQueryCandidates(cleaned))
    const deconfused = deconfuseDigits(cleaned)
    if (deconfused !== cleaned) for (const q of nameQueryCandidates(deconfused)) queries.add(q)
    for (const query of queries) {
      const pool = await fetchNames(query, NAME_CANDIDATE_LIMIT)
      if (pool.length) return rankNames(cleaned, pool)
    }

    const prefix = namePoolPrefix(cleaned)
    if (prefix) {
      const ranked = rankNames(cleaned, await fetchNames(prefix, FUZZY_POOL_LIMIT))
      const top = ranked[0]
      if (top && ocrSimilarity(cleaned, top) >= FUZZY_MATCH_THRESHOLD) return ranked
    }
    return []
  }

  /** Write the current match's absolute counts, unless nothing changed. Logs the add with
   * its pre-add counts for undo. Returns whether a write actually happened. */
  async function commitCurrent(): Promise<boolean> {
    const card = selectedCard.value
    if (!card || !seeded.value) return false
    const previous = { ...owned.value }
    if (
      target.quantity === previous.quantity &&
      target.foil_quantity === previous.foil_quantity
    ) {
      return false
    }
    // A concurrent commit is already writing this same match (Stop/unmount racing an in-flight
    // auto tick) — don't double-write and double-log it.
    if (commitInFlight) return false
    commitInFlight = true
    try {
      await mutation.mutateAsync({
        game: game.value,
        id: card.id,
        quantity: target.quantity,
        foil_quantity: target.foil_quantity,
      })
      commitError.value = false
      log.value.unshift({
        id: nextEntryId++,
        card,
        quantity: target.quantity,
        foil_quantity: target.foil_quantity,
        previous,
      })
      return true
    } catch {
      commitError.value = true
      return false
    } finally {
      commitInFlight = false
    }
  }

  /**
   * Process a fresh capture. Resolves the OCR'd name against the catalog; if it's a
   * genuinely different card, commits the one on screen and swaps in the new match. A
   * capture of the same card (or unreadable text) leaves the current match untouched.
   */
  async function handleCapture(capture: ScanCapture): Promise<CaptureOutcome> {
    // Don't advance past a match whose holding is still loading — commit it first.
    if (!currentSettled.value) return 'busy'
    const cleaned = cleanCardName(capture.nameText)
    if (cleaned.length < 3) return 'unmatched'
    resolving.value = true
    try {
      const candidates = await resolveNames(cleaned)
      const name = candidates[0]
      if (!name) {
        lastUnmatched.value = cleaned
        return 'unmatched'
      }
      if (match.value && name === match.value.name) return 'same'
      await commitCurrent()
      // A failed save keeps the current card on screen rather than silently replacing (and
      // losing) it; the next capture retries the commit once the connection recovers.
      if (commitError.value) return 'busy'
      startMatch({ ocrName: cleaned, hint: parseSetHint(capture.setText), candidates, name })
      return 'matched'
    } finally {
      resolving.value = false
    }
  }

  /** Revert a logged add back to the counts it had before (both-zero deletes the row). */
  async function undo(index: number): Promise<void> {
    const entry = log.value[index]
    if (!entry) return
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
      commitError.value = false
    } catch {
      commitError.value = true
    }
  }

  const committing = computed(() => mutation.isPending.value)
  const addedCount = computed(() => log.value.length)

  return {
    // match state
    match,
    prints,
    printsLoading,
    selectedId,
    selectedCard,
    owned,
    target,
    ready,
    resolving,
    ownedError,
    // session
    log,
    addedCount,
    lastUnmatched,
    commitError,
    committing,
    // actions
    handleCapture,
    commitCurrent,
    discardCurrent,
    selectId,
    setName,
    adjust,
    undo,
    retryOwned,
  }
}
