import { makeNavList } from '@/composables/navList'
import { useCardNavStore } from '@/stores/cardNav'

// Publish a browse grid's ordered card ids into the card nav registry so the card-detail modal
// can step prev/next through them (issue #275). Both CardGrid and CollectionGrid call this —
// one bridge, every card surface. Mechanics: `composables/navList.ts`.
export const useCardNavList = makeNavList(useCardNavStore)
