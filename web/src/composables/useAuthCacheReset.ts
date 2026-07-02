import { watch } from 'vue'
import { useQueryClient, type QueryClient } from '@tanstack/vue-query'
import { useAuthStore } from '@/stores/auth'

/**
 * Remove every cached per-user query — those `useAuthedQuery` tags with `meta.authed`
 * (the collection + wish-list families and their per-card/summary/set/import reads).
 * Public catalog queries (games/sets/cards/prices), which are identical for everyone,
 * are left untouched so a logout/login doesn't needlessly reload them.
 */
export function clearAuthedQueries(qc: QueryClient) {
  qc.removeQueries({ predicate: (query) => query.meta?.authed === true })
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
 */
export function useAuthCacheReset() {
  const qc = useQueryClient()
  const auth = useAuthStore()
  watch(
    () => auth.user?.id ?? null,
    () => clearAuthedQueries(qc),
  )
}
