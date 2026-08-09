import { computed, ref, watch, type ComputedRef, type Ref } from 'vue'

/**
 * Hold a list's ORDER still for as long as the view stays mounted, while its items keep
 * updating in place.
 *
 * Recency-sorted lists (`updated_at DESC` — the deck list, the holdings grids) have a nasty
 * property: the thing you just touched jumps to the front, so a refetch doesn't refresh the
 * list, it *rearranges* it. Every row below the mover shifts by a full row, and a tap
 * already on its way to one row lands on its neighbour. It reads as "my touch is off" — the
 * tap was fine, the target moved.
 *
 * The worst case isn't even a background refetch, it's arriving at the list. vue-query paints
 * cached data immediately and refetches behind it when the key is stale, and editing a deck
 * marks the deck list stale — so navigating back from a deck paints the OLD order, then swaps
 * in a new one a moment later, every single time.
 *
 * So: remember the order of the first list this mount painted, and keep it. A refetch still
 * updates every item's own contents (name, counts, colours) because items are re-read from the
 * source by key — only their positions are pinned. Items that disappear upstream drop out;
 * items that appear go to the front, matching the newest-first sort they arrived in. The true
 * order lands on the next mount, which is the same "settles on the next navigation" contract
 * the wish list's frozen browse list uses.
 *
 * Only for lists whose order is decided upstream and *not* re-pickable on screen: a list with
 * its own sort control would appear to ignore the control, since a re-sort keeps every key and
 * would therefore be pinned away. (The holdings grids are the counter-example — they paginate
 * and carry a sort menu, so they hold their grid still a different way; see
 * `composables/holdingListFreeze.ts`.)
 *
 * @param source getter for the upstream list — a `computed` over the query data
 * @param keyOf  stable identity per item (a row id, never an array index)
 */
export function useStableOrder<T, K>(source: () => T[], keyOf: (item: T) => K): ComputedRef<T[]> {
  const order = ref<K[]>([]) as Ref<K[]>

  watch(
    source,
    (items) => {
      const keys = items.map(keyOf)
      const known = new Set(order.value)
      const live = new Set(keys)
      // Arrivals lead (upstream sorts newest first, and a new row is the user's own doing);
      // everything already placed keeps its slot; anything gone upstream drops out. On the
      // first run `order` is empty, so this is just the upstream order.
      order.value = [
        ...keys.filter((key) => !known.has(key)),
        ...order.value.filter((key) => live.has(key)),
      ]
    },
    { immediate: true },
  )

  return computed(() => {
    const byKey = new Map(source().map((item) => [keyOf(item), item]))
    return order.value.map((key) => byKey.get(key)).filter((item): item is T => item !== undefined)
  })
}
