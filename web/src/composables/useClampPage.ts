import { watch, type Ref } from 'vue'

/** The current list's readiness + size, read reactively so the clamp re-runs when
 * the active view (e.g. by-drop vs flat) or its total changes. */
interface PageBounds {
  /** The query the count comes from has succeeded (so `total` is real). */
  ready: boolean
  total: number
  pageSize: number
}

/**
 * Clamp `page` back into range once the real total is known. A shared or stale link
 * can point past the last page (a bookmarked search whose results later shrank, or a
 * hand-edited ?page); without this the user would be stranded on an empty page with
 * no pager to escape it. `bounds` is a getter so a view with multiple paginated modes
 * can feed whichever one is active (its total + page size).
 */
export function useClampPage(page: Ref<number>, bounds: () => PageBounds) {
  watch(
    bounds,
    ({ ready, total, pageSize }) => {
      if (!ready) return
      const lastPage = Math.max(1, Math.ceil(total / pageSize))
      if (page.value > lastPage) page.value = lastPage
    },
    { immediate: true },
  )
}
