<script setup lang="ts">
import { computed } from 'vue'
import { useQuery } from '@tanstack/vue-query'
import { Layers } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import {
  NavigationMenu,
  NavigationMenuContent,
  NavigationMenuItem,
  NavigationMenuLink,
  NavigationMenuList,
  NavigationMenuTrigger,
} from '@/components/ui/navigation-menu'
import { listGames } from '@/lib/api'

// Drive the menu from the same registry the /cards landing uses (cached), so a new
// TCG appears here automatically. The /cards landing page lists games too; this is
// the quick-access shortcut from the top bar.
const { data } = useQuery({
  queryKey: ['games'],
  queryFn: () => listGames(),
  staleTime: Infinity,
})
const games = computed(() => data.value?.data ?? [])
</script>

<template>
  <NavigationMenu>
    <NavigationMenuList>
      <NavigationMenuItem>
        <NavigationMenuTrigger>
          <Layers class="mr-1.5 size-4" aria-hidden="true" />
          Cards
        </NavigationMenuTrigger>
        <NavigationMenuContent>
          <ul class="grid w-56 gap-1">
            <!-- Browse-all keeps the old parent link to the games landing page. -->
            <li>
              <!-- Override lives on the wrapper so cn()/tailwind-merge resolves the
                   flex-col→flex-row + gap conflict deterministically (not via CSS order). -->
              <NavigationMenuLink as-child class="flex-row items-center gap-2 font-medium">
                <RouterLink to="/cards">
                  <Layers aria-hidden="true" />
                  Browse all games
                </RouterLink>
              </NavigationMenuLink>
            </li>
            <!-- One quick-access shortcut per game, from the same cached registry. -->
            <li v-for="game in games" :key="game.id">
              <NavigationMenuLink as-child>
                <RouterLink :to="`/cards/${game.id}`">{{ game.name }}</RouterLink>
              </NavigationMenuLink>
            </li>
          </ul>
        </NavigationMenuContent>
      </NavigationMenuItem>
    </NavigationMenuList>
  </NavigationMenu>
</template>
