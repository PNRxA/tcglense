<script setup lang="ts">
import { computed } from 'vue'
import { Heart, Layers, Library } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import {
  NavigationMenu,
  NavigationMenuContent,
  NavigationMenuItem,
  NavigationMenuLink,
  NavigationMenuList,
  NavigationMenuTrigger,
} from '@/components/ui/navigation-menu'
import { useGamesQuery } from '@/composables/useCatalog'

// The top-bar primary nav: "Cards" (public catalog), "Collection" and "Wish list"
// (per-user).
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
</script>

<template>
  <NavigationMenu :viewport="false">
    <NavigationMenuList>
      <!-- Cards: the public catalog. -->
      <NavigationMenuItem>
        <NavigationMenuTrigger>
          <Layers class="mr-1.5 size-4" aria-hidden="true" />
          Cards
        </NavigationMenuTrigger>
        <NavigationMenuContent>
          <ul class="grid w-56 gap-1">
            <li>
              <!-- Override on the wrapper so cn()/tailwind-merge resolves the
                   flex-col→flex-row + gap conflict deterministically (not via CSS order). -->
              <NavigationMenuLink as-child class="flex-row items-center gap-2 font-medium">
                <RouterLink to="/cards">
                  <Layers aria-hidden="true" />
                  Browse all games
                </RouterLink>
              </NavigationMenuLink>
            </li>
            <li v-for="game in games" :key="game.id">
              <NavigationMenuLink as-child>
                <RouterLink :to="`/cards/${game.id}`">{{ game.name }}</RouterLink>
              </NavigationMenuLink>
            </li>
          </ul>
        </NavigationMenuContent>
      </NavigationMenuItem>

      <!-- Collection: the signed-in user's owned cards (prompts sign-in if needed). -->
      <NavigationMenuItem>
        <NavigationMenuTrigger>
          <Library class="mr-1.5 size-4" aria-hidden="true" />
          Collection
        </NavigationMenuTrigger>
        <NavigationMenuContent>
          <ul class="grid w-56 gap-1">
            <li>
              <NavigationMenuLink as-child class="flex-row items-center gap-2 font-medium">
                <RouterLink to="/collection">
                  <Library aria-hidden="true" />
                  All collections
                </RouterLink>
              </NavigationMenuLink>
            </li>
            <li v-for="game in games" :key="game.id">
              <NavigationMenuLink as-child>
                <RouterLink :to="`/collection/${game.id}`">{{ game.name }}</RouterLink>
              </NavigationMenuLink>
            </li>
          </ul>
        </NavigationMenuContent>
      </NavigationMenuItem>

      <!-- Wish list: the cards the user wants to buy (issue #167; prompts sign-in if
           needed). -->
      <NavigationMenuItem>
        <NavigationMenuTrigger>
          <Heart class="mr-1.5 size-4" aria-hidden="true" />
          Wish list
        </NavigationMenuTrigger>
        <NavigationMenuContent>
          <ul class="grid w-56 gap-1">
            <li>
              <NavigationMenuLink as-child class="flex-row items-center gap-2 font-medium">
                <RouterLink to="/wishlist">
                  <Heart aria-hidden="true" />
                  All wish lists
                </RouterLink>
              </NavigationMenuLink>
            </li>
            <li v-for="game in games" :key="game.id">
              <NavigationMenuLink as-child>
                <RouterLink :to="`/wishlist/${game.id}`">{{ game.name }}</RouterLink>
              </NavigationMenuLink>
            </li>
          </ul>
        </NavigationMenuContent>
      </NavigationMenuItem>
    </NavigationMenuList>
  </NavigationMenu>
</template>
