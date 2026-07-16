import { onMounted, onUnmounted, toValue, watch, type MaybeRefOrGetter } from 'vue'
import { useProductNavStore } from '@/stores/productNav'

// Publish a product grid's ordered ids into the shared nav registry so the detail modal can
// step through them. The grid remains the source of truth; this mirrors its current page while
// mounted, updates on paging/filtering, and unregisters on teardown.
export function useProductNavList(
  game: MaybeRefOrGetter<string>,
  ids: MaybeRefOrGetter<string[]>,
): void {
  const nav = useProductNavStore()
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
