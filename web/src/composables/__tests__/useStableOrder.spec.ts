import { describe, it, expect } from 'vitest'
import { effectScope, nextTick, ref } from 'vue'
import { useStableOrder } from '@/composables/useStableOrder'

// The bug: the deck list comes back `updated_at DESC`, and every deck write marks it stale.
// Arriving from a deck you just edited paints the cached (old) order and then swaps in a
// reordered one, pushing every tile below the mover down a full row — under a thumb already
// on its way to one of them, which is how "select a deck" opens the neighbour.

interface Row {
  id: number
  name: string
}
const row = (id: number, name = `deck ${id}`): Row => ({ id, name })

/** Drive the composable inside a scope, since it registers a watcher. */
function driveOrder(initial: Row[]) {
  const source = ref<Row[]>(initial)
  const scope = effectScope()
  const stable = scope.run(() =>
    useStableOrder(
      () => source.value,
      (item) => item.id,
    ),
  )!
  const ids = () => stable.value.map((item) => item.id)
  const set = async (next: Row[]) => {
    source.value = next
    await nextTick()
  }
  return { ids, names: () => stable.value.map((item) => item.name), set, stop: () => scope.stop() }
}

describe('useStableOrder', () => {
  it('takes the upstream order on the first list', () => {
    const { ids, stop } = driveOrder([row(3), row(1), row(2)])
    expect(ids()).toEqual([3, 1, 2])
    stop()
  })

  it('keeps every row where it is when a refetch reorders them', async () => {
    const { ids, set, stop } = driveOrder([row(3), row(1), row(2)])
    // The refetch that lands after editing deck 2: upstream now leads with it.
    await set([row(2), row(3), row(1)])
    // Nothing moved under the user — reverting the pin gives [2, 3, 1] here, and that shift
    // is exactly what lands a tap on the neighbouring deck.
    expect(ids()).toEqual([3, 1, 2])
    stop()
  })

  it('still refreshes each row in place while its position is pinned', async () => {
    const { names, set, stop } = driveOrder([row(1, 'old'), row(2, 'two')])
    await set([row(2, 'two'), row(1, 'renamed')])
    // Position pinned, contents live: the point is to freeze ORDER, not data. A tile whose
    // card count or name went stale until the next navigation would be its own bug.
    expect(names()).toEqual(['renamed', 'two'])
    stop()
  })

  it('drops rows that disappear upstream and leads with new arrivals', async () => {
    const { ids, set, stop } = driveOrder([row(3), row(1), row(2)])
    // Deck 1 deleted, deck 9 created.
    await set([row(9), row(3), row(2)])
    // 9 leads (upstream sorts newest first and creating it was the user's own doing), 1 is
    // gone, and 3/2 keep the slots they already had.
    expect(ids()).toEqual([9, 3, 2])
    stop()
  })

  it('adopts the current order on a fresh mount', async () => {
    const first = driveOrder([row(3), row(1), row(2)])
    await first.set([row(2), row(3), row(1)])
    expect(first.ids()).toEqual([3, 1, 2])
    first.stop()

    // Navigating away and back re-runs the composable, so the pin is per-visit: the recency
    // order the user was denied mid-visit is what greets them next time.
    const second = driveOrder([row(2), row(3), row(1)])
    expect(second.ids()).toEqual([2, 3, 1])
    second.stop()
  })

  it('survives an empty list without losing the rows that come back', async () => {
    const { ids, set, stop } = driveOrder([])
    await set([row(5), row(4)])
    expect(ids()).toEqual([5, 4])
    stop()
  })
})
