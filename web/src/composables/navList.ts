import { onMounted, onUnmounted, toValue, watch, type MaybeRefOrGetter } from 'vue'
import type { NavStoreApi } from '@/stores/nav'

/**
 * Build the grid → nav registry bridge for one item kind (see `useCardNavList` /
 * `useProductNavList`, the two thin wrappers over this). The returned composable publishes a
 * browse grid's ordered ids into `useNavStore`'s registry so the matching detail modal can step
 * prev/next through them (issue #275).
 *
 * The grid stays the source of truth: this only mirrors its current page of ids while mounted,
 * keeps them in sync as the page/list changes, and withdraws them on unmount so a torn-down grid
 * never offers stale navigation.
 */
export function makeNavList(useNavStore: () => NavStoreApi) {
  return function useNavList(
    game: MaybeRefOrGetter<string>,
    ids: MaybeRefOrGetter<string[]>,
  ): void {
    const nav = useNavStore()
    let handle: number | null = null

    onMounted(() => {
      handle = nav.register({ game: toValue(game), ids: toValue(ids) })
    })

    watch(
      () => [toValue(game), toValue(ids)] as const,
      ([currentGame, currentIds]) => {
        if (handle !== null) nav.update(handle, { game: currentGame, ids: currentIds })
      },
    )

    onUnmounted(() => {
      if (handle !== null) nav.unregister(handle)
      handle = null
    })
  }
}
