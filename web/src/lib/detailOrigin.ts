import type { LocationQueryRaw } from 'vue-router'

// The one query key that records which detail surface a modal was opened FROM, so the shared
// DetailDialogShell can offer a one-tap "← Back to <origin>" crumb when you cross from a sealed
// product into one of its contained cards — or the reverse, from a card into one of its sealed
// products. It is per-trip navigation state: set only on a product<->card swap (CardTile /
// ProductTile), and dropped again on close, on stepping to a neighbour, and on opening any
// unrelated item — so it never lingers past the single hop it describes. Deeper history is the
// browser's Back button's job; this only ever points one surface back.
//
// Card and product both live in the same game (the `:game` route param, or `?game=` on the
// game-less public deck page), so only the origin's kind + id need to travel — encoded as
// `<kind>:<id>`. The id half is opaque (an external card / TCGplayer id) and never contains a
// colon, so a split on the FIRST colon round-trips any id safely.
//
// NOT `?from=`: that key is already the set-grouping "entered from" set code on the card pages
// this modal overlays (see `useSetGrouping.ts`). A distinct key keeps the two from clobbering each
// other — the modal never touches `?from=`, and the grouped view never touches this.

export const DETAIL_ORIGIN_KEY = 'openedFrom'

/** The two detail surfaces a modal can hand off to. Matches the shell's `queryKey`s. */
export type DetailOriginKind = 'card' | 'product'

/** A parsed origin marker: the surface you came from and its id (same game as the current item). */
export interface DetailOrigin {
  kind: DetailOriginKind
  id: string
}

/** Encode an origin marker for the URL. */
export function encodeDetailOrigin(kind: DetailOriginKind, id: string): string {
  return `${kind}:${id}`
}

/** Parse `?openedFrom=<kind>:<id>` back into an origin, or null for anything malformed / unknown. */
export function parseDetailOrigin(raw: unknown): DetailOrigin | null {
  if (typeof raw !== 'string') return null
  const sep = raw.indexOf(':')
  if (sep <= 0) return null
  const kind = raw.slice(0, sep)
  const id = raw.slice(sep + 1)
  if (!id || (kind !== 'card' && kind !== 'product')) return null
  return { kind, id }
}

/** Set the origin marker on a query when a swap leaves `fromId` behind, or drop it when there's
 * nothing to come back to. Mutates and returns `query` for chaining inside a tile's click handler
 * (which has already copied `route.query`). */
export function applyDetailOrigin(
  query: LocationQueryRaw,
  kind: DetailOriginKind,
  fromId: string | null | undefined,
): LocationQueryRaw {
  if (fromId) query[DETAIL_ORIGIN_KEY] = encodeDetailOrigin(kind, fromId)
  else delete query[DETAIL_ORIGIN_KEY]
  return query
}
