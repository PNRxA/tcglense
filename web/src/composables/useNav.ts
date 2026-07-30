import { computed, type Component } from 'vue'
import { useGamesQuery } from '@/composables/useCatalog'
import { NAV, resolveGroup, type NavItem, type ResolvedGroup } from '@/lib/nav'

/**
 * The nav registry (`lib/nav.ts`) married to the games query — the reactive half of the seam,
 * kept out of the pure table so the footer can read the table without pulling in a query.
 *
 * Before the query resolves, `games` is `[]` and the tree carries landings only: the panels
 * show "Browse all games" / "All decks" with no per-game rows yet, which is exactly the
 * degraded state both navs render today.
 *
 * Nothing is memoised and no resolved state lives at module scope. Resolving ~10 items over a
 * couple of games is nanoseconds, and a cache here would only be a reactivity bug waiting for
 * a second TCG to arrive.
 */

/** The resolved analogue of `NavRoot`: a menu's groups carry their per-game expansion. */
export type ResolvedRoot =
  | { kind: 'menu'; id: string; label: string; icon: Component; groups: ResolvedGroup[] }
  | { kind: 'link'; item: NavItem }

export function useNav() {
  const { data } = useGamesQuery()
  const roots = computed<ResolvedRoot[]>(() => {
    const games = data.value?.data ?? []
    return NAV.map((root) =>
      root.kind === 'link'
        ? root
        : { ...root, groups: root.groups.map((group) => resolveGroup(group, games)) },
    )
  })
  return { roots }
}
