import { makeNavStore } from '@/stores/nav'

// The sealed-product registry (issue #438). ProductGrid publishes into it via `useProductNavList`
// and ProductDetailDialog reads it to step prev/next through the products on the browse page
// underneath. Mechanics + rationale: `stores/nav.ts`.
export const useProductNavStore = makeNavStore('productNav')
