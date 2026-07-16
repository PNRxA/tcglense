<script setup lang="ts">
import { computed } from 'vue'
import { Code, Heart, Layers, Library, Package, ScanLine } from '@lucide/vue'
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
import { useGamesQuery } from '@/composables/useCatalog'
import { prefetchRouteChunks } from '@/lib/prefetch'

// The top-bar primary nav: "Products" (the public catalog — Cards + Sealed products,
// grouped in one dropdown), "Collection" and "Wish list" (per-user).
//
// All items live under ONE NavigationMenu / NavigationMenuList on purpose: reka-ui
// computes the swipe direction (data-motion=from-start/from-end) only between siblings
// in that same menu, so moving across triggers still animates directionally. The menu
// runs `viewport=false`, though: the default shared viewport renders every panel in one
// box pinned to the menu's left edge, which reads as the dropdown being stuck under
// "Cards" — without it each NavigationMenuContent positions itself under its own
// trigger (the item is `relative`, the content `top-full`), like UserMenu already does.
//
// The dropdowns are driven by the same cached games registry, so a new TCG appears in
// all of them automatically. Collection and Wish list are shown to everyone; a
// signed-out visitor who opens one is prompted to sign in / sign up on that page.
const { data } = useGamesQuery()
const games = computed(() => data.value?.data ?? [])

// Prefetch the target route's JS chunk on hover/focus so the click lands on an
// already-loaded view (see lib/prefetch.ts — chunks only, never data/images).
const router = useRouter()
const warm = (to: RouteLocationRaw) => prefetchRouteChunks(router, to)

// A dropdown opens 300ms–1s before the click that follows: warm the whole section's
// landing + per-game chunks then, since those links only exist in the DOM while open.
// reka emits the open item's `value` (empty string on close). Per-game links all map to
// the same view chunk, so iterating games costs nothing extra (import() dedupes).
function warmSection(value: string) {
  if (value === 'products') {
    warm('/cards')
    warm('/sealed')
    for (const game of games.value) {
      warm(`/cards/${game.id}`)
      warm(`/sealed/${game.id}`)
    }
  } else if (value === 'collection') {
    warm('/collection')
    warm('/scan')
    warm('/decks')
    for (const game of games.value) {
      warm(`/collection/${game.id}`)
      warm(`/decks/${game.id}`)
    }
  } else if (value === 'wishlist') {
    warm('/wishlist')
    for (const game of games.value) warm(`/wishlist/${game.id}`)
  }
}
</script>

<template>
  <NavigationMenu :viewport="false" @update:model-value="warmSection">
    <NavigationMenuList>
      <!-- Products: the public catalog, split into Cards + Sealed products. Both share
           one dropdown so they read as one catalog; each group has a "browse all games"
           landing plus one entry per game. -->
      <NavigationMenuItem value="products">
        <NavigationMenuTrigger>
          <Layers class="mr-1.5 size-4" aria-hidden="true" />
          Products
        </NavigationMenuTrigger>
        <!-- Force a floating dropdown at every width the nav shows. The shared
             NavigationMenuContent only turns `absolute md:w-auto` at the md breakpoint;
             this explicit override ties the floating to MainNav's own gate (lg) rather
             than the primitive's, keeping the panel floating from the moment it appears —
             without it a statically laid-out panel's z-50 goes inert and it slips under
             page content like the sealed-product image (issue #259). Same override
             UserMenu already carries — MainNav dropdowns are left-aligned, so no end-0. -->
        <NavigationMenuContent class="absolute top-full w-auto">
          <ul class="grid w-56 gap-1">
            <!-- Cards -->
            <li>
              <p class="text-muted-foreground px-2 pb-1 text-xs font-medium">Cards</p>
            </li>
            <li>
              <!-- Override on the wrapper so cn()/tailwind-merge resolves the
                   flex-col→flex-row + gap conflict deterministically (not via CSS order). -->
              <NavigationMenuLink as-child class="flex-row items-center gap-2 font-medium">
                <RouterLink to="/cards" @pointerenter="warm('/cards')" @focusin="warm('/cards')">
                  <Layers aria-hidden="true" />
                  Browse all games
                </RouterLink>
              </NavigationMenuLink>
            </li>
            <li v-for="game in games" :key="`cards-${game.id}`">
              <NavigationMenuLink as-child>
                <RouterLink
                  :to="`/cards/${game.id}`"
                  @pointerenter="warm(`/cards/${game.id}`)"
                  @focusin="warm(`/cards/${game.id}`)"
                  >{{ game.name }}</RouterLink
                >
              </NavigationMenuLink>
            </li>
            <!-- Sealed products (booster boxes, bundles, decks). -->
            <li class="mt-1 border-t pt-2">
              <p class="text-muted-foreground px-2 pb-1 text-xs font-medium">Sealed</p>
            </li>
            <li>
              <NavigationMenuLink as-child class="flex-row items-center gap-2 font-medium">
                <RouterLink to="/sealed" @pointerenter="warm('/sealed')" @focusin="warm('/sealed')">
                  <Package aria-hidden="true" />
                  Browse all games
                </RouterLink>
              </NavigationMenuLink>
            </li>
            <li v-for="game in games" :key="`sealed-${game.id}`">
              <NavigationMenuLink as-child>
                <RouterLink
                  :to="`/sealed/${game.id}`"
                  @pointerenter="warm(`/sealed/${game.id}`)"
                  @focusin="warm(`/sealed/${game.id}`)"
                  >{{ game.name }}</RouterLink
                >
              </NavigationMenuLink>
            </li>
          </ul>
        </NavigationMenuContent>
      </NavigationMenuItem>

      <!-- Collection: the signed-in user's owned cards (prompts sign-in if needed). -->
      <NavigationMenuItem value="collection">
        <NavigationMenuTrigger>
          <Library class="mr-1.5 size-4" aria-hidden="true" />
          Collection
        </NavigationMenuTrigger>
        <!-- absolute top-full w-auto: floating dropdown at every width (see Products note, issue #259). -->
        <NavigationMenuContent class="absolute top-full w-auto">
          <ul class="grid w-56 gap-1">
            <li>
              <NavigationMenuLink as-child class="flex-row items-center gap-2 font-medium">
                <RouterLink
                  to="/collection"
                  @pointerenter="warm('/collection')"
                  @focusin="warm('/collection')"
                >
                  <Library aria-hidden="true" />
                  All collections
                </RouterLink>
              </NavigationMenuLink>
            </li>
            <li v-for="game in games" :key="game.id">
              <NavigationMenuLink as-child>
                <RouterLink
                  :to="`/collection/${game.id}`"
                  @pointerenter="warm(`/collection/${game.id}`)"
                  @focusin="warm(`/collection/${game.id}`)"
                  >{{ game.name }}</RouterLink
                >
              </NavigationMenuLink>
            </li>
            <!-- Decks (issue #363): build and organise decks of cards. Sit below the
                 collections behind a divider, with a per-game list mirroring them so a deck
                 game is reachable in one hop (issue #394). -->
            <li class="mt-1 border-t pt-2">
              <NavigationMenuLink as-child class="flex-row items-center gap-2 font-medium">
                <RouterLink to="/decks" @pointerenter="warm('/decks')" @focusin="warm('/decks')">
                  <Layers aria-hidden="true" />
                  All decks
                </RouterLink>
              </NavigationMenuLink>
            </li>
            <li v-for="game in games" :key="`decks-${game.id}`">
              <NavigationMenuLink as-child>
                <RouterLink
                  :to="`/decks/${game.id}`"
                  @pointerenter="warm(`/decks/${game.id}`)"
                  @focusin="warm(`/decks/${game.id}`)"
                  >{{ game.name }}</RouterLink
                >
              </NavigationMenuLink>
            </li>
            <!-- Scan: a distinct action, so it gets its own divider below the decks (issue
                 #394). -->
            <li class="mt-1 border-t pt-2">
              <NavigationMenuLink as-child class="flex-row items-center gap-2 font-medium">
                <RouterLink to="/scan" @pointerenter="warm('/scan')" @focusin="warm('/scan')">
                  <ScanLine aria-hidden="true" />
                  Scan cards
                </RouterLink>
              </NavigationMenuLink>
            </li>
          </ul>
        </NavigationMenuContent>
      </NavigationMenuItem>

      <!-- Wish list: the cards the user wants to buy (issue #167; prompts sign-in if
           needed). -->
      <NavigationMenuItem value="wishlist">
        <NavigationMenuTrigger>
          <Heart class="mr-1.5 size-4" aria-hidden="true" />
          Wish list
        </NavigationMenuTrigger>
        <!-- absolute top-full w-auto: floating dropdown at every width (see Products note, issue #259). -->
        <NavigationMenuContent class="absolute top-full w-auto">
          <ul class="grid w-56 gap-1">
            <li>
              <NavigationMenuLink as-child class="flex-row items-center gap-2 font-medium">
                <RouterLink
                  to="/wishlist"
                  @pointerenter="warm('/wishlist')"
                  @focusin="warm('/wishlist')"
                >
                  <Heart aria-hidden="true" />
                  All wish lists
                </RouterLink>
              </NavigationMenuLink>
            </li>
            <li v-for="game in games" :key="game.id">
              <NavigationMenuLink as-child>
                <RouterLink
                  :to="`/wishlist/${game.id}`"
                  @pointerenter="warm(`/wishlist/${game.id}`)"
                  @focusin="warm(`/wishlist/${game.id}`)"
                  >{{ game.name }}</RouterLink
                >
              </NavigationMenuLink>
            </li>
          </ul>
        </NavigationMenuContent>
      </NavigationMenuItem>

      <!-- API docs: a plain link (no dropdown) to the in-app Scalar reference (issue #284).
           NavigationMenuLink's base class is `flex flex-col gap-1` (built for stacked
           dropdown entries); the trigger style doesn't override the direction, so without
           `flex-row` the icon stacks ABOVE the label and "API" sits lower than its sibling
           triggers. `gap-0` drops the leaked column gap so the icon's `mr-1.5` is the only
           spacing — matching the Products/Collection/Wish-list triggers exactly. -->
      <NavigationMenuItem>
        <NavigationMenuLink as-child :class="[navigationMenuTriggerStyle(), 'flex-row gap-0']">
          <RouterLink to="/docs" @pointerenter="warm('/docs')" @focusin="warm('/docs')">
            <Code class="mr-1.5 size-4" aria-hidden="true" />
            API
          </RouterLink>
        </NavigationMenuLink>
      </NavigationMenuItem>
    </NavigationMenuList>
  </NavigationMenu>
</template>
