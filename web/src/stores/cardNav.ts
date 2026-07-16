import { makeNavStore } from '@/stores/nav'

// The card registry (issue #275). CardGrid + CollectionGrid publish into it via `useCardNavList`
// — one bridge feeding the catalog, sets, collection, wish list, sealed-product card sections,
// and the modal's own "other printings" — and CardDetailDialog reads it to step prev/next
// through the cards on the page underneath. Mechanics + rationale: `stores/nav.ts`.
export const useCardNavStore = makeNavStore('cardNav')
