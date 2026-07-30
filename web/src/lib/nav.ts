import type { Component } from 'vue'
import {
  BookCopy,
  BookOpen,
  Code,
  Compass,
  Heart,
  Layers,
  Library,
  Package,
  ScanLine,
  Wrench,
} from '@lucide/vue'
import type { Game } from '@/lib/api'
import { toolPath, toolsFor, toolsPath } from '@/lib/tools'

/**
 * The navigation registry — the one place the app's information architecture is written down.
 *
 * A registry rather than hand-written menus, for the same reason `lib/tools.ts` is one: three
 * surfaces render this same tree — the desktop top bar (`MainNav`), the mobile drawer
 * (`MobileNav`) and the footer's Product column (`AppFooter`) — and each used to spell it out
 * by hand. That is exactly how Decks, Alerts and the Keyword glossary each reached one nav and
 * not the others: three copies mean three chances to forget one, and nothing fails when you
 * do. A destination added here appears on every surface at once, and prefetch warming is
 * *derived* — `itemWarmTargets` flattens the same resolved tree the templates render — so the
 * warm list cannot drift from the links either (a declared `prefetch` field would just be the
 * old bug re-admitted as schema).
 *
 * This module describes **structure and meaning only, never presentation**. No Tailwind
 * classes, no breakpoints, no icon sizes, no per-surface ordering or visibility flags: a
 * consumer decides how a role looks, the registry only says what the role IS — `kind: 'index'`
 * marks an "…and the rest" link, `auth` mirrors the router's `requiresAuth`. A field belongs
 * here only if two or more consumers read it *and* it is semantic.
 *
 * It is also deliberately pure — types, data, and plain functions of `Game`, with no Vue
 * reactivity and no composables. Per-game expansion is stored as a *function* of `Game` so the
 * table stays static while the expansion stays reactive: `composables/useNav.ts` marries it to
 * the games query, and `AppFooter` imports from here without breaking its "fully static: no
 * queries" promise.
 */

/** One rendered link. `kind` says what the link *is*, so a consumer can style the roles apart
 * without the registry knowing anything about styling. */
export interface NavLink {
  label: string
  to: string
  kind?: 'leaf' | 'index'
}

/** One destination in the IA — a landing, plus the per-game rows it expands into. */
export interface NavItem {
  /** Stable seam consumers key presentation on (MobileNav promotes `'scan'` by id). */
  id: string
  label: string
  icon: Component
  /** The all-games landing: the item's own destination. */
  landing: string
  /** What the landing row says *inside an expanded panel*, where the item's own label is
   * already the header ('Browse all games', 'All decks'). A consumer rendering the item as a
   * single row uses `label`. */
  landingLabel?: string
  /** Per-game expansion. Omitted = the item is a single link everywhere. */
  gameLinks?: (game: Game) => NavLink[]
  /** Mirrors `requiresAuth` in the router. Metadata, not a visibility switch — both navs
   * deliberately show these signed out and let the route do the prompting. */
  auth?: boolean
}

/** A labelled column of items inside a menu root. */
export interface NavGroup {
  id: string
  label: string
  items: NavItem[]
}

/** A top-bar entry: either a menu that opens onto groups, or a bare link. */
export type NavRoot =
  | { kind: 'menu'; id: string; label: string; icon: Component; groups: NavGroup[] }
  | { kind: 'link'; item: NavItem }

/** The common expansion: one row per game, under the item's own base path. */
const perGame =
  (base: string) =>
  (game: Game): NavLink[] => [{ label: game.name, to: `${base}/${game.id}` }]

export const NAV: readonly NavRoot[] = [
  {
    kind: 'menu',
    id: 'browse',
    label: 'Browse',
    icon: Compass,
    groups: [
      {
        id: 'catalog',
        label: 'Catalog',
        items: [
          {
            id: 'cards',
            label: 'Cards',
            icon: Layers,
            landing: '/cards',
            landingLabel: 'Browse all games',
            gameLinks: perGame('/cards'),
          },
          {
            id: 'sealed',
            label: 'Sealed',
            icon: Package,
            landing: '/sealed',
            landingLabel: 'Browse all games',
            gameLinks: perGame('/sealed'),
          },
          {
            id: 'keywords',
            label: 'Keyword glossary',
            icon: BookOpen,
            landing: '/keywords',
            landingLabel: 'All games',
            gameLinks: perGame('/keywords'),
          },
        ],
      },
      {
        id: 'library',
        label: 'Your library',
        items: [
          {
            id: 'collection',
            label: 'Collection',
            icon: Library,
            landing: '/collection',
            landingLabel: 'All collections',
            gameLinks: perGame('/collection'),
          },
          {
            id: 'decks',
            label: 'Decks',
            icon: BookCopy,
            landing: '/decks',
            landingLabel: 'All decks',
            gameLinks: perGame('/decks'),
          },
          {
            id: 'wishlist',
            label: 'Wish list',
            icon: Heart,
            landing: '/wishlist',
            landingLabel: 'All wish lists',
            gameLinks: perGame('/wishlist'),
          },
          {
            id: 'scan',
            label: 'Scan cards',
            icon: ScanLine,
            landing: '/scan',
            auth: true,
          },
        ],
      },
    ],
  },
  {
    // Tools keeps its own root rather than becoming a third Browse column: a play aid is not
    // something you browse. One group, so the menu shape stays uniform for its consumers.
    kind: 'menu',
    id: 'tools',
    label: 'Tools',
    icon: Wrench,
    groups: [
      {
        id: 'tools',
        label: 'Tools',
        items: [
          {
            id: 'tools',
            label: 'Tools',
            icon: Wrench,
            landing: '/tools',
            landingLabel: 'All tools',
            // The one bespoke expansion, and the reason `gameLinks` is a function rather than
            // a base path: a game's tools come from the `lib/tools` registry, so each tool is
            // linked directly (with a small number of tools the hop through the game index
            // buys nothing) and the index trails as the "…and the rest" row. A game with no
            // tools returns nothing and `resolveItem` drops it — extending the tools seam
            // rather than forking a second copy of it here.
            gameLinks: (game) => {
              const tools = toolsFor(game.id)
              if (tools.length === 0) return []
              return [
                ...tools.map((tool) => ({ label: tool.name, to: toolPath(game.id, tool.slug) })),
                { label: `All ${game.name} tools`, to: toolsPath(game.id), kind: 'index' },
              ]
            },
          },
        ],
      },
    ],
  },
  {
    // A bare link: the API reference has nothing to expand into.
    kind: 'link',
    item: { id: 'docs', label: 'API', icon: Code, landing: '/docs' },
  },
]

/** An item with its per-game expansion applied — games that expand to nothing are gone. */
export interface ResolvedItem {
  item: NavItem
  perGame: Array<{ game: Game; links: NavLink[] }>
}

export interface ResolvedGroup {
  group: NavGroup
  items: ResolvedItem[]
}

/**
 * Expand an item over the games registry.
 *
 * A game whose expansion is empty is dropped rather than rendered as an empty heading — that
 * is what makes a game with no tools vanish from the Tools menu while every other item still
 * lists it.
 */
export function resolveItem(item: NavItem, games: readonly Game[]): ResolvedItem {
  const expand = item.gameLinks
  if (!expand) return { item, perGame: [] }
  return {
    item,
    perGame: games
      .map((game) => ({ game, links: expand(game) }))
      .filter((entry) => entry.links.length > 0),
  }
}

export function resolveGroup(group: NavGroup, games: readonly Game[]): ResolvedGroup {
  return { group, items: group.items.map((item) => resolveItem(item, games)) }
}

/** Every route chunk worth warming for an item: its landing plus each per-game link. Derived
 * from the resolved tree the templates render, so the two can't disagree. */
export function itemWarmTargets(resolved: ResolvedItem): string[] {
  return [
    resolved.item.landing,
    ...resolved.perGame.flatMap((entry) => entry.links.map((link) => link.to)),
  ]
}

export function groupWarmTargets(resolved: ResolvedGroup): string[] {
  return resolved.items.flatMap(itemWarmTargets)
}

/** Every item across the tree, in registry order — the list the specs sweep. */
export function allNavItems(): NavItem[] {
  return NAV.flatMap((root) =>
    root.kind === 'link' ? [root.item] : root.groups.flatMap((group) => group.items),
  )
}

/**
 * The footer's Product column: the account-free items, **from menu roots only**.
 *
 * The `kind: 'link'` roots are excluded on purpose. `docs` is one, and in the footer the API
 * reference already lives in the Project column beside GitHub and Terms — moving it into
 * Product would be a regression dressed as consistency. Rather than special-casing `/docs`,
 * the rule is structural: the Product column lists what the primary menus browse into, and a
 * bare top-bar link is not that. `auth` items (Scan) are filtered out because the footer is
 * the one surface with no room to explain a sign-in prompt.
 */
export function publicNavItems(): NavItem[] {
  return NAV.flatMap((root) =>
    root.kind === 'menu' ? root.groups.flatMap((g) => g.items) : [],
  ).filter((item) => !item.auth)
}
