import { defineStore } from 'pinia'
import { ref } from 'vue'

// The bridge that lets a detail modal offer prev/next through the items on the page it opened
// over (issue #275). A modal is decoupled from its grids — it's mounted once in App.vue and
// driven purely off the URL (`?card=`/`?product=<id>` + the `:game` route param), so it never
// receives a list. Instead every browse grid *registers* the ordered ids it's currently showing
// here (via `makeNavList` in `composables/navList.ts`), and the modal looks the open item up to
// find its neighbours.
//
// One registry per item kind, built by the factory below (`stores/cardNav.ts`,
// `stores/productNav.ts`): cards and sealed products are separate id spaces published by
// separate grids, and a card modal must never step onto a product. The registry itself is
// identical for both, so it lives here once.

// One registered browse grid: its game plus the ordered ids of the items it's showing now.
export interface NavGrid {
  game: string
  ids: string[]
}

// An item's place within the grid that holds it: the ids to move to on prev/next (null at a
// boundary), the 0-based index (-1 when the item is on no registered grid), and the grid size.
export interface NavPosition {
  prev: string | null
  next: string | null
  index: number
  total: number
}

/** What a nav registry exposes: the three calls a grid makes to publish itself, and the lookup
 * a modal reads. Every store {@link makeNavStore} builds satisfies this, so the grid bridge and
 * the dialog shell take the registry as a plain dependency and stay kind-agnostic. */
export interface NavStoreApi {
  register(grid: NavGrid): number
  update(handle: number, grid: NavGrid): void
  unregister(handle: number): void
  locate(game: string, itemId: string): NavPosition
}

const NOT_FOUND: NavPosition = { prev: null, next: null, index: -1, total: 0 }

/** Build one nav registry as a Pinia store under `id` (unique per item kind). */
export function makeNavStore<Id extends string>(id: Id) {
  return defineStore(id, () => {
    // Registered grids in mount order, keyed by a monotonic handle. A reactive Map so the modal's
    // position re-computes whenever a grid registers, repages (its ids change), or unmounts.
    const grids = ref(new Map<number, NavGrid>())
    let nextHandle = 0

    function register(grid: NavGrid): number {
      const handle = nextHandle++
      grids.value.set(handle, grid)
      return handle
    }

    function update(handle: number, grid: NavGrid): void {
      // Only when still registered — an update racing an unmount must not resurrect the entry.
      if (grids.value.has(handle)) grids.value.set(handle, grid)
    }

    function unregister(handle: number): void {
      grids.value.delete(handle)
    }

    // The neighbours of `itemId` within the first registered grid (in mount order) for `game`
    // that contains it — strictly "left/right on the current page", so no wraparound and no
    // cross-page fetch (issue #275). Mount order makes the page's own grid win over a grid
    // nested inside the modal (the card modal's "other printings", registered later, when the
    // modal opens). `index === -1` when the item is on no current grid (a fresh deep link, or a
    // printing not on the page underneath), which the modal reads as "no nav".
    function locate(game: string, itemId: string): NavPosition {
      for (const grid of grids.value.values()) {
        if (grid.game !== game) continue
        const index = grid.ids.indexOf(itemId)
        if (index === -1) continue
        return {
          prev: index > 0 ? (grid.ids[index - 1] ?? null) : null,
          next: index < grid.ids.length - 1 ? (grid.ids[index + 1] ?? null) : null,
          index,
          total: grid.ids.length,
        }
      }
      return NOT_FOUND
    }

    return { register, update, unregister, locate }
  })
}
