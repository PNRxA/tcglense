import { defineStore } from 'pinia'
import { ref } from 'vue'

// The bridge that lets the card-detail modal offer prev/next through the cards on the page
// it opened over (issue #275). The modal is decoupled from its grids — it's mounted once in
// App.vue and driven purely off the URL (`?card=<id>` + the `:game` route param), so it never
// receives a list. Instead every browse grid *registers* the ordered card ids it's currently
// showing here (via `useCardNavList`, called by CardGrid + CollectionGrid — one bridge feeding
// the catalog, sets, collection, wish list, sealed-product sections, and the modal's own
// "other printings"), and the modal looks the current card up to find its neighbours.

// One registered browse grid: its game plus the ordered ids of the cards it's showing now.
export interface CardNavGrid {
  game: string
  ids: string[]
}

// A card's place within the grid that holds it: the ids to move to on prev/next (null at a
// boundary), the 0-based index (-1 when the card is on no registered grid), and the grid size.
export interface CardNavPosition {
  prev: string | null
  next: string | null
  index: number
  total: number
}

const NOT_FOUND: CardNavPosition = { prev: null, next: null, index: -1, total: 0 }

export const useCardNavStore = defineStore('cardNav', () => {
  // Registered grids in mount order, keyed by a monotonic handle. A reactive Map so the modal's
  // position re-computes whenever a grid registers, repages (its ids change), or unmounts.
  const grids = ref(new Map<number, CardNavGrid>())
  let nextHandle = 0

  function register(grid: CardNavGrid): number {
    const handle = nextHandle++
    grids.value.set(handle, grid)
    return handle
  }

  function update(handle: number, grid: CardNavGrid): void {
    // Only when still registered — an update racing an unmount must not resurrect the entry.
    if (grids.value.has(handle)) grids.value.set(handle, grid)
  }

  function unregister(handle: number): void {
    grids.value.delete(handle)
  }

  // The neighbours of `cardId` within the first registered grid (in mount order) for `game`
  // that contains it — strictly "left/right on the current page", so no wraparound and no
  // cross-page fetch (issue #275). Mount order makes the page's own grid win over the modal's
  // nested "other printings" grid (registered later, when the modal opens). `index === -1`
  // when the card is on no current grid (a fresh deep link, or a printing not on the page
  // underneath), which the modal reads as "no nav".
  function locate(game: string, cardId: string): CardNavPosition {
    for (const grid of grids.value.values()) {
      if (grid.game !== game) continue
      const index = grid.ids.indexOf(cardId)
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
