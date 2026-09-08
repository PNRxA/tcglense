import { computed, type ComputedRef } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { patchQuery } from '@/composables/useCardSearch'
import {
  copiesToken,
  isCopiesFilterActive,
  parseCopiesToken,
  parseFinish,
  type CopiesFilter,
} from '@/lib/holdingsFilter'

/**
 * The holdings copy-count + finish filter (issue #677), backed by the URL query — `?copies=`
 * (the `N` / `N-M` / `N-` / `-M` token grammar) and `?finish=` — so "the playsets I'm one
 * short of" is shareable, bookmarkable and survives opening a card and pressing Back, exactly
 * like {@link useCardSearch}'s `?q`/`?sort`.
 *
 * Kept beside that composable rather than inside it because the two filter through different
 * grammars: `q` is Scryfall's (parsed server-side, 422 on nonsense), while these are plain
 * numeric query params the *holdings* listings take and the public catalog endpoints do not.
 * They must never be folded into `q`.
 *
 * Every write is ONE `patchQuery` merge of both keys (an empty value drops its key, unrelated
 * keys — `view`/`related`/`from`/`ghosts` — are preserved) and restarts paging: page 3 of the
 * unfiltered list is meaningless once the list narrows. One write, not one per half: each
 * `router.replace` snapshots the route *before* the previous navigation commits, so two
 * writes in a tick would re-land the key the first had just dropped. Unlike a sort commit, a
 * filter change does NOT leave a grouped view — the bounds apply to the cards within each
 * drop / sub-type just as well.
 */
export function useCopiesFilter(): {
  copies: ComputedRef<CopiesFilter>
  active: ComputedRef<boolean>
  set: (filter: CopiesFilter) => void
  clear: () => void
} {
  const route = useRoute()
  const router = useRouter()

  const patch = (changes: Record<string, string | undefined>) => patchQuery(route, router, changes)

  const bounds = computed(() =>
    parseCopiesToken(typeof route.query.copies === 'string' ? route.query.copies : undefined),
  )
  const finish = computed(() => parseFinish(route.query.finish))

  /** The committed filter — what the query hooks key off and send as wire params. */
  const copies = computed<CopiesFilter>(() => ({ ...bounds.value, finish: finish.value }))

  const active = computed(() => isCopiesFilterActive(copies.value))

  /** Commit a whole filter in one write: the bounds as the canonical `?copies=` token (an
   * unbounded filter drops the key), the finish (its `any` default drops the key), page 1. */
  function set(filter: CopiesFilter) {
    patch({
      copies: copiesToken(filter.min, filter.max),
      finish: filter.finish === 'any' ? undefined : filter.finish,
      page: undefined,
    })
  }

  /** Drop the whole filter in one write (both keys), back to page 1. */
  function clear() {
    patch({ copies: undefined, finish: undefined, page: undefined })
  }

  return { copies, active, set, clear }
}
