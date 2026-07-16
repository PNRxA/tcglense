import { makeNavList } from '@/composables/navList'
import { useProductNavStore } from '@/stores/productNav'

// Publish a product grid's ordered ids into the sealed-product nav registry so the product-detail
// modal can step prev/next through them (issue #438). ProductGrid calls this.
// Mechanics: `composables/navList.ts`.
export const useProductNavList = makeNavList(useProductNavStore)
