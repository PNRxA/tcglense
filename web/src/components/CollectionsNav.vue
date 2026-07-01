<script setup lang="ts">
import { computed } from 'vue'
import { Library } from '@lucide/vue'
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

// Mirrors CardsNav (same games registry, cached), but points at the per-game
// collection pages instead of the public catalog. Rendered only for signed-in users
// (see App.vue) — collections are per-account.
const { data } = useGamesQuery()
const games = computed(() => data.value?.data ?? [])
</script>

<template>
  <NavigationMenu>
    <NavigationMenuList>
      <NavigationMenuItem>
        <NavigationMenuTrigger>
          <Library class="mr-1.5 size-4" aria-hidden="true" />
          Collection
        </NavigationMenuTrigger>
        <NavigationMenuContent>
          <ul class="grid w-56 gap-1">
            <!-- Parent link to the collections landing (all games). -->
            <li>
              <NavigationMenuLink as-child class="flex-row items-center gap-2 font-medium">
                <RouterLink to="/collection">
                  <Library aria-hidden="true" />
                  All collections
                </RouterLink>
              </NavigationMenuLink>
            </li>
            <!-- One quick-access shortcut per game. -->
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
