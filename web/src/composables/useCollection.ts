import {
  getCollection,
  getCollectionEntry,
  getCollectionOwned,
  getCollectionSetDrops,
  getCollectionSets,
  getCollectionSetSubtypes,
  getCollectionSummary,
  setCollectionEntry,
} from '@/lib/api'
import { makeHoldingQueries, type SetHoldingVars } from '@/composables/holdingQueries'

// Server state for the signed-in user's card collection — the `['collection', …]`
// query-key family. All the read hooks, the browse-badge counts hook, the invalidation
// helper, and the set-entry mutation are the shared holding composables (see
// `holdingQueries.ts`); this module instantiates that factory with the collection api
// functions and re-exports each member under its existing name/signature. The collection
// is the bulk-threshold-carrying instance (its summary/sets keys+calls thread the user's
// bulk-value cutoff) and the one that also invalidates the `collection-value-history` key.
const queries = makeHoldingQueries({
  prefix: 'collection',
  countsKey: 'collection-owned',
  getList: getCollection,
  getSetDrops: getCollectionSetDrops,
  getSetSubtypes: getCollectionSetSubtypes,
  getSummary: getCollectionSummary,
  getSets: getCollectionSets,
  getEntry: getCollectionEntry,
  getCounts: getCollectionOwned,
  setEntry: setCollectionEntry,
  withBulkThreshold: true,
  invalidateValueHistory: true,
  deferListRefetch: false,
})

/**
 * Refresh every view that depends on the collection contents after any collection write
 * — a per-card edit or a completed import/sync. Covers the grid, the summary header, the
 * per-card owned-count steppers, the by-drop owned-cards view, the per-set landing tiles
 * (ownership per set can change broadly), the collection value history, and the
 * browse-grid owned-count badges. Pass `entryId` to scope the per-card entry invalidation
 * to the edited card; an import touches many cards, so it invalidates the whole game.
 */
export const invalidateCollectionData = queries.invalidate

/** A page of the user's owned cards for a game. See `holdingQueries.useListQuery`. */
export const useCollectionQuery = queries.useListQuery

/** A page (by drop) of the user's owned cards in a drop-grouped set (e.g. Secret Lair). */
export const useCollectionDropsQuery = queries.useDropsQuery

/** A page (by sub-type) of the user's owned cards in a set, grouped by card treatment. */
export const useCollectionSubtypesQuery = queries.useSubtypesQuery

/** Aggregate stats for the collection, optionally scoped to one set (carries the
 * bulk-threshold preference). */
export const useCollectionSummaryQuery = queries.useSummaryQuery

/** The sets the user owns cards in (newest first) — the per-set collection landing. */
export const useCollectionSetsQuery = queries.useSetsQuery

/** How many copies of one card the signed-in user owns — for the card-detail controls. */
export const useCollectionEntryQuery = queries.useEntryQuery

/** Owned counts for the cards currently being browsed — the collection browse badges. */
export const useOwnedCounts = queries.useCounts

/** Variables for a collection write: which card, and the desired absolute counts. */
export type SetCollectionVars = SetHoldingVars

/** Set the owned counts for a card, then invalidate the dependent collection views. */
export const useSetCollectionEntryMutation = queries.useSetEntryMutation
