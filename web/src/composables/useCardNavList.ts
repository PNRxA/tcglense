import { onMounted, onUnmounted, toValue, watch, type MaybeRefOrGetter } from 'vue'
import { useCardNavStore } from '@/stores/cardNav'

// Publish a browse grid's ordered card ids into the shared nav registry so the card-detail
// modal can step prev/next through them (issue #275). Both CardGrid and CollectionGrid call
// this — one bridge, every card surface. The grid stays the source of truth; this only mirrors
// its current page of ids while mounted, keeps them in sync as the page/list changes, and
// withdraws them on unmount so a torn-down grid never offers stale navigation.
export function useCardNavList(
  game: MaybeRefOrGetter<string>,
  ids: MaybeRefOrGetter<string[]>,
): void {
  const nav = useCardNavStore()
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
