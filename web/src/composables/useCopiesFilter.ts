import { computed, type ComputedRef, type WritableComputedRef } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { patchQuery } from '@/composables/useCardSearch'
import {
  copiesToken as formatCopiesToken,
  isCopiesFilterActive,
  parseCopiesToken,
  parseFinish,
  type CopiesFilter,
  type HoldingFinish,
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
 * Writes go through the shared `patchQuery` merge (an empty value drops its key, unrelated
 * keys — `view`/`related`/`from`/`ghosts` — are preserved) and restart paging: page 3 of the
 * unfiltered list is meaningless once the list narrows. Unlike a sort commit, a filter change
 * does NOT leave a grouped view — the bounds apply to the cards within each drop / sub-type
 * just as well as to the flat grid.
 */
export function useCopiesFilter(): {
  copies: ComputedRef<CopiesFilter>
  copiesToken: WritableComputedRef<string>
  finish: WritableComputedRef<HoldingFinish>
  active: ComputedRef<boolean>
  clear: () => void
} {
  const route = useRoute()
  const router = useRouter()

  const patch = (changes: Record<string, string | undefined>) => patchQuery(route, router, changes)

  const bounds = computed(() =>
    parseCopiesToken(typeof route.query.copies === 'string' ? route.query.copies : undefined),
  )
  const finishValue = computed(() => parseFinish(route.query.finish))

  /** The committed filter — what the query hooks key off and send as wire params. */
  const copies = computed<CopiesFilter>(() => ({ ...bounds.value, finish: finishValue.value }))

  /** The `?copies=` token as the chip's radio group sees it: `''` for "any count". A written
   * value is canonicalized through the grammar, so an unparseable one simply clears. */
  const copiesToken = computed<string>({
    get: () => formatCopiesToken(bounds.value.min, bounds.value.max) ?? '',
    set: (value) => {
      const parsed = parseCopiesToken(value)
      patch({ copies: formatCopiesToken(parsed.min, parsed.max), page: undefined })
    },
  })

  /** Which counter the bounds read. The `any` default drops the key rather than spelling it. */
  const finish = computed<HoldingFinish>({
    get: () => finishValue.value,
    set: (value) => patch({ finish: value === 'any' ? undefined : value, page: undefined }),
  })

  const active = computed(() => isCopiesFilterActive(copies.value))

  /** Drop the whole filter in one write (both keys), back to page 1. */
  function clear() {
    patch({ copies: undefined, finish: undefined, page: undefined })
  }

  return { copies, copiesToken, finish, active, clear }
}
