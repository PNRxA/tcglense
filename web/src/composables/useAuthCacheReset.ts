import { watch } from 'vue'
import { useQueryClient, type QueryClient } from '@tanstack/vue-query'
import { forgetAllSeatTokens } from '@/lib/playSeat'
import { useAuthStore } from '@/stores/auth'

/**
 * Whether a query key addresses per-user data. Every per-user family is namespaced under
 * a `collection*` / `wishlist*` / `life*` / `play*` prefix (or the `import-job` poll); no
 * public catalog key uses those, so this never matches shared data.
 *
 * This is a belt to the `meta.authed` tag's braces: `useAuthedQuery` tags every per-user
 * *read*, but the per-card entry mutations write their result straight into the cache with
 * `qc.setQueryData(['collection-entry'|'wishlist-entry', …])`. When no entry-query observer
 * is mounted for that id — the quick-add flow, which seeds from the batch counts and never
 * mounts the single-entry query — `setQueryData` builds a fresh cache entry from default
 * options, so it carries no `meta`. Matching by key too drops those orphans on a switch.
 */
function isPerUserQueryKey(key: readonly unknown[]): boolean {
  const head = key[0]
  return (
    typeof head === 'string' &&
    (head.startsWith('collection') ||
      head.startsWith('wishlist') ||
      head.startsWith('life') ||
      // The play table's room list is per-user (rooms you host or hold a seat in); the
      // single-room read is public, but it shares the family head, and dropping a public
      // room summary on an identity change costs one refetch and keeps the rule one line.
      head.startsWith('play') ||
      head === 'import-job')
  )
}

/**
 * Remove every cached per-user query — those `useAuthedQuery` tags with `meta.authed`
 * (the collection + wish-list families and their per-card/summary/set/import reads), plus
 * any `setQueryData`-seeded per-user entry that carries no meta (see {@link isPerUserQueryKey}).
 * Public catalog queries (games/sets/cards/prices), which are identical for everyone, are
 * left untouched so a logout/login doesn't needlessly reload them.
 */
export function clearAuthedQueries(qc: QueryClient) {
  qc.removeQueries({
    predicate: (query) => query.meta?.authed === true || isPerUserQueryKey(query.queryKey),
  })
  // `removeQueries` only touches the QueryCache; also drop cached mutation results so a
  // write's `data` can't outlive the identity — notably the create-api-key mutation,
  // whose `data` holds the one-time plaintext key (issue #284).
  qc.getMutationCache().clear()
}

/**
 * Drop all cached per-user data whenever the signed-in identity changes — on logout,
 * login, or an account switch. vue-query's cache outlives a logout (it's only wiped by
 * a hard page reload), so without this the previous account's collection/wish list
 * stays cached and shows through to the next signed-in user (issue #177).
 *
 * Keyed on the user *id* (not the access token, which rotates on every silent refresh
 * for the same user), so a routine token refresh never clobbers the cache. The watcher
 * is not `immediate`: on first load there's nothing cached to clear, and the restored
 * session is the same identity. Mounted once, from App.vue.
 *
 * The play table's seat tokens go with it: they live in `localStorage`, not in the query
 * cache, but they are the same kind of datum — per-identity proof that outlives a logout
 * unless something drops it (`lib/playSeat.ts`).
 */
export function useAuthCacheReset() {
  const qc = useQueryClient()
  const auth = useAuthStore()
  watch(
    () => auth.user?.id ?? null,
    (_id, prevId) => {
      // Only clear when the PREVIOUS id was a real identity (idA→idB switch, or id→null
      // logout). Skip the boot transition (null→id): under the non-blocking router guard
      // authed queries begin the moment refresh sets the token — BEFORE fetchMe resolves
      // the user id — so the null→id flip fetchMe triggers must not wipe those freshly
      // populated caches (that double-fetched every per-user query). A signed-out user
      // can't populate per-user cache anyway (useAuthedQuery is disabled while signed
      // out), so null→id never needs a wipe.
      if (prevId === null) return
      clearAuthedQueries(qc)
      // Seats are held by token, not by session, so a token left behind would seat the next
      // identity in the previous one's chair the moment they open that room's link.
      forgetAllSeatTokens()
    },
  )
}
