import { defineStore } from 'pinia'
import { ref } from 'vue'

// The bridge that lets the sealed-product detail modal offer prev/next through the products
// on the page it opened over. The modal is mounted once in App.vue and driven by the URL, so
// ProductGrid registers its ordered ids here and the modal looks up the open product.

export interface ProductNavGrid {
  game: string
  ids: string[]
}

export interface ProductNavPosition {
  prev: string | null
  next: string | null
  index: number
  total: number
}

const NOT_FOUND: ProductNavPosition = { prev: null, next: null, index: -1, total: 0 }

export const useProductNavStore = defineStore('productNav', () => {
  const grids = ref(new Map<number, ProductNavGrid>())
  let nextHandle = 0

  function register(grid: ProductNavGrid): number {
    const handle = nextHandle++
    grids.value.set(handle, grid)
    return handle
  }

  function update(handle: number, grid: ProductNavGrid): void {
    if (grids.value.has(handle)) grids.value.set(handle, grid)
  }

  function unregister(handle: number): void {
    grids.value.delete(handle)
  }

  // The first matching grid wins, preserving the page grid's mount-order priority over any
  // later nested grid. Navigation stays within the currently displayed page, with no wrap.
  function locate(game: string, productId: string): ProductNavPosition {
    for (const grid of grids.value.values()) {
      if (grid.game !== game) continue
      const index = grid.ids.indexOf(productId)
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
