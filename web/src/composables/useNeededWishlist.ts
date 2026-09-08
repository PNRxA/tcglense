import { computed, type Ref } from 'vue'
import { useQueryClient } from '@tanstack/vue-query'
import { ApiError, setWishlistEntry, type NeededCard } from '@/lib/api'
import { invalidateWishlistData, useWishlistCounts } from '@/composables/useWishlist'
import { planWishlistTopUps, type WishlistTopUp } from '@/lib/neededCost'
import { useAuthedMutation } from '@/lib/queries'

/** What one "add all to wish list" run did. */
export interface WishlistTopUpResult {
  /** Cards whose wanted count went up. */
  added: number
  /** Cards whose write failed; the rest landed. */
  failed: number
}

/** Writes in flight at once. A shopping list is a few dozen cards at most, and the wish-list
 * write is the plain per-card upsert (there is no bulk twin), so a small fan-out keeps the
 * run short without hammering the per-user quota. */
const CONCURRENCY = 4

/**
 * "Add all to wish list" for a shopping list (issue #675): reads the wanted counts of every
 * needed card, plans the top-ups that leave the wish list holding at least the copies still
 * needed (`planWishlistTopUps` — idempotent, never lowering a count), and writes them as
 * **one** mutation, so the wish-list family is invalidated once at the end rather than once
 * per card. `toAdd` is how many cards the next run would touch — `0` reads as "already on
 * your wish list", which is what makes the button safe to show after a success.
 */
export function useNeededWishlist(game: Ref<string>, entries: Ref<NeededCard[]>) {
  const cards = computed(() => entries.value.map((entry) => entry.card))
  // Fresh counts on every open: the plan is built from absolute counts, so a stale badge
  // read could re-add copies the user just removed on the wish-list page. And the plan is
  // trusted only once that refetch has landed — `ready && !fetching`, the gate
  // `useBatchCounts` documents for every absolute-count seeder (a same-key refetch serves
  // the retained map as "ready" while the fresh one loads, and a write planned off it would
  // lower a count the user raised elsewhere since).
  const {
    ownership: wanted,
    ready: loaded,
    fetching,
  } = useWishlistCounts(game, cards, { staleTime: 0 })
  const ready = computed(() => loaded.value && !fetching.value)
  const plan = computed<WishlistTopUp[]>(() => planWishlistTopUps(entries.value, wanted.value))
  const toAdd = computed(() => plan.value.length)

  const qc = useQueryClient()
  // Built as a plain object first (typed callbacks) so TanStack's deeply-reactive option
  // types don't trip excess-property checks through the `useAuthed*` wrapper.
  const options = {
    mutationFn: async (token: string, topUps: WishlistTopUp[]): Promise<WishlistTopUpResult> => {
      let added = 0
      let failed = 0
      // Bounded fan-out; a failed card is counted, not fatal — the others still land. The
      // one exception is the per-user quota: once a write is refused as rate-limited, every
      // remaining one would be too, so the rest are counted as failed without being sent.
      for (let i = 0; i < topUps.length; i += CONCURRENCY) {
        const batch = topUps.slice(i, i + CONCURRENCY)
        const outcomes = await Promise.allSettled(
          batch.map((topUp) =>
            setWishlistEntry(token, game.value, topUp.id, {
              quantity: topUp.quantity,
              foil_quantity: topUp.foil_quantity,
            }),
          ),
        )
        let throttled = false
        for (const outcome of outcomes) {
          if (outcome.status === 'fulfilled') added += 1
          else {
            failed += 1
            if (outcome.reason instanceof ApiError && outcome.reason.status === 429)
              throttled = true
          }
        }
        if (throttled) {
          failed += topUps.length - (i + batch.length)
          break
        }
      }
      return { added, failed }
    },
    onSettled: () => invalidateWishlistData(qc, game.value),
  }
  const mutation = useAuthedMutation<WishlistTopUpResult, WishlistTopUp[]>(options)

  /** Run the plan. The outcome lands in `result` / `error` — the caller reads those rather
   * than a promise, so a refused batch is never an unhandled rejection. */
  function addAll(): void {
    mutation.mutate(plan.value)
  }

  return {
    /** The wanted counts have loaded, so `toAdd` is trustworthy. */
    ready,
    toAdd,
    pending: mutation.isPending,
    result: mutation.data,
    error: computed<ApiError | null>(() => mutation.error.value),
    addAll,
  }
}
