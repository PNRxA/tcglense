<script setup lang="ts">
import { RouterLink, useRouter, type RouteLocationRaw } from 'vue-router'
import {
  NavigationMenu,
  NavigationMenuContent,
  NavigationMenuItem,
  NavigationMenuLink,
  NavigationMenuList,
  NavigationMenuTrigger,
  navigationMenuTriggerStyle,
} from '@/components/ui/navigation-menu'
import { useNav } from '@/composables/useNav'
import { groupWarmTargets, type ResolvedItem } from '@/lib/nav'
import { prefetchRouteChunks } from '@/lib/prefetch'

// The top-bar primary nav: "Browse" (the whole catalog plus your own library, one
// mega-panel of two columns), "Tools" (the play aids) and a bare "API" link.
//
// Nothing about the information architecture is written here: `useNav()` resolves the
// registry in `lib/nav.ts` over the cached games query, and this file iterates whatever
// it returns. That is the point of the registry — three surfaces used to spell the same
// tree out by hand, which is exactly how Decks and the Keyword glossary each reached one
// nav and not the others. So the template branches on a root's *shape* (`kind`, and how
// many groups it has), never on a root's id: a third menu, or a fourth Browse column,
// renders correctly with no edit here.
//
// All roots live under ONE NavigationMenu / NavigationMenuList on purpose: reka-ui
// computes the swipe direction (data-motion=from-start/from-end) only between siblings
// in that same menu, so moving across triggers still animates directionally. The menu
// runs `viewport=false`, though: the default shared viewport renders every panel in one
// box pinned to the menu's left edge, which reads as the dropdown being stuck under the
// first trigger — without it each NavigationMenuContent positions itself under its own
// trigger (the item is `relative`, the content `top-full`), like UserMenu already does.
//
// Collection, decks and wish list are shown to everyone; a signed-out visitor who opens
// one is prompted to sign in / sign up on that page (the registry's `auth` flag mirrors
// the router, it is not a visibility switch).
const { roots } = useNav()

/** The item's per-game rows, flattened — one `<li>` each, in registry order. */
const itemLinks = (resolved: ResolvedItem) => resolved.perGame.flatMap((entry) => entry.links)

// Prefetch the target route's JS chunk on hover/focus so the click lands on an
// already-loaded view (see lib/prefetch.ts — chunks only, never data/images).
const router = useRouter()
const warm = (to: RouteLocationRaw) => prefetchRouteChunks(router, to)

// A dropdown opens 300ms–1s before the click that follows: warm the whole panel's
// landing + per-game chunks then, since those links only exist in the DOM while open.
// reka emits the opening item's `value` (empty string on close), which is why each menu
// item carries `:value="root.id"`.
//
// The list is *derived* — `groupWarmTargets` flattens the very same resolved tree the
// panel below renders — so a destination added to the registry is warmed without anyone
// remembering to add it here, and the warm list cannot drift from the rendered links.
// That is what replaced the hand-written per-section if/else chain; deliberately not a
// single path string lives in this block. Per-game links mostly map to the same view
// chunk, so iterating games costs nothing extra (import() dedupes).
function warmRoot(value: string) {
  const root = roots.value.find((entry) => entry.kind === 'menu' && entry.id === value)
  if (root?.kind !== 'menu') return
  for (const group of root.groups) {
    for (const to of groupWarmTargets(group)) warm(to)
  }
}
</script>

<template>
  <NavigationMenu :viewport="false" @update:model-value="warmRoot">
    <NavigationMenuList>
      <template v-for="root in roots" :key="root.kind === 'link' ? root.item.id : root.id">
        <!-- A menu root: a trigger plus a panel of one column per group. -->
        <NavigationMenuItem v-if="root.kind === 'menu'" :value="root.id">
          <NavigationMenuTrigger>
            <component :is="root.icon" class="mr-1.5 size-4" aria-hidden="true" />
            {{ root.label }}
          </NavigationMenuTrigger>
          <!-- Force a floating dropdown at every width the nav shows. The shared
               NavigationMenuContent only turns `absolute md:w-auto` at the md breakpoint;
               this explicit override ties the floating to MainNav's own gate (lg) rather
               than the primitive's, keeping the panel floating from the moment it appears —
               without it a statically laid-out panel's z-50 goes inert and it slips under
               page content like the sealed-product image (issue #259). Same override
               UserMenu already carries — MainNav dropdowns are left-aligned, so no end-0. -->
          <NavigationMenuContent class="absolute top-full w-auto">
            <!-- Columns come from the group count, never from the root's id: Browse's two
                 groups become a two-column mega-panel, Tools' single group stays the ~w-60
                 dropdown it is today, and a future second two-column menu needs no edit.
                 34rem is two comfortable ~16.5rem columns; anchored under the first trigger
                 (~140px in) it ends well short of the 1024px lg viewport where MainNav
                 first appears, so the panel never pushes the window into overflow. -->
            <div
              class="grid gap-x-4"
              :class="root.groups.length > 1 ? 'w-[34rem] grid-cols-2' : 'w-60 grid-cols-1'"
            >
              <ul
                v-for="{ group, items } in root.groups"
                :key="group.id"
                class="grid content-start gap-1"
              >
                <!-- A single-group menu needs no column heading: its trigger already names
                     it, and "Tools › Tools › Tools" was the result of rendering one anyway. -->
                <li v-if="root.groups.length > 1">
                  <p class="text-muted-foreground px-2 pb-1 text-xs font-medium">
                    {{ group.label }}
                  </p>
                </li>
                <template v-for="(entry, index) in items" :key="entry.item.id">
                  <!-- Every item after the column's first is fenced off by a divider, the
                       way Sealed and Scan already were. -->
                  <li :class="index > 0 ? 'mt-1 border-t pt-2' : ''">
                    <!-- Override on the wrapper so cn()/tailwind-merge resolves the
                         flex-col→flex-row + gap conflict deterministically (not via CSS
                         order). -->
                    <NavigationMenuLink as-child class="flex-row items-center gap-2 font-medium">
                      <RouterLink
                        :to="entry.item.landing"
                        @pointerenter="warm(entry.item.landing)"
                        @focusin="warm(entry.item.landing)"
                      >
                        <component :is="entry.item.icon" aria-hidden="true" />
                        <!-- The item's own name, on the link to its all-games landing. It
                             used to say "Browse all games" under a separate per-item
                             heading; with the columns grouped by Catalog / Your library
                             that heading is gone, and three rows all reading "Browse all
                             games" told you nothing about which was Cards. Naming the row
                             is also exactly what the drawer does, so the two surfaces now
                             read identically. -->
                        {{ entry.item.label }}
                      </RouterLink>
                    </NavigationMenuLink>
                  </li>
                  <!-- The per-game expansion, indented under the item it narrows so the two
                       tiers read apart (pl-8 lands the text under the label above, past the
                       icon column). `kind: 'index'` is the "…and the rest" row (a game's own
                       tools page), muted so it reads as a footnote to the links above it
                       rather than a peer of them. -->
                  <li v-for="link in itemLinks(entry)" :key="link.to">
                    <NavigationMenuLink as-child>
                      <RouterLink
                        :to="link.to"
                        :class="['pl-8', link.kind === 'index' ? 'text-muted-foreground' : '']"
                        @pointerenter="warm(link.to)"
                        @focusin="warm(link.to)"
                        >{{ link.label }}</RouterLink
                      >
                    </NavigationMenuLink>
                  </li>
                </template>
              </ul>
            </div>
          </NavigationMenuContent>
        </NavigationMenuItem>

        <!-- A bare link root (the in-app Scalar API reference, issue #284): nothing to
             expand into, so no dropdown. NavigationMenuLink's base class is
             `flex flex-col gap-1` (built for stacked dropdown entries) and the trigger
             style doesn't override the direction, so without `flex-row` the icon stacks
             ABOVE the label and "API" sits lower than its sibling triggers. `gap-0` drops
             the leaked column gap so the icon's `mr-1.5` is the only spacing — matching
             the Browse/Tools triggers exactly. -->
        <NavigationMenuItem v-else>
          <NavigationMenuLink as-child :class="[navigationMenuTriggerStyle(), 'flex-row gap-0']">
            <RouterLink
              :to="root.item.landing"
              @pointerenter="warm(root.item.landing)"
              @focusin="warm(root.item.landing)"
            >
              <component :is="root.item.icon" class="mr-1.5 size-4" aria-hidden="true" />
              {{ root.item.label }}
            </RouterLink>
          </NavigationMenuLink>
        </NavigationMenuItem>
      </template>
    </NavigationMenuList>
  </NavigationMenu>
</template>
