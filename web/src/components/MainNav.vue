<script setup lang="ts">
import { computed } from 'vue'
import { Layers, Library } from '@lucide/vue'
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

// The top-bar primary nav: "Cards" (public catalog) and "Collection" (per-user).
//
// Both items live under ONE NavigationMenu / NavigationMenuList on purpose: reka-ui
// shares a single animated viewport across the items in a menu and computes the swipe
// direction (data-motion=from-start/from-end) only between siblings in that same menu.
// Splitting them into two separate <NavigationMenu> roots (as this used to) gives each
// its own isolated viewport and no directional motion, so the docs' swipe-and-fade
// between the two never plays.
//
// Both dropdowns are driven by the same cached games registry, so a new TCG appears in
// both automatically. Collection is shown to everyone; a signed-out visitor who opens a
// collection is prompted to sign in / sign up on that page.
const { data } = useGamesQuery()
const games = computed(() => data.value?.data ?? [])
</script>

<template>
  <NavigationMenu>
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
    </NavigationMenuList>
  </NavigationMenu>
</template>
