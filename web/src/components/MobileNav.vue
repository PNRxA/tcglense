<script setup lang="ts">
import { computed } from 'vue'
import { Heart, Layers, Library, Menu, Package } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { useGamesQuery } from '@/composables/useCatalog'

// The mobile counterpart to MainNav: the top-bar's Cards/Collection/Wish list
// dropdowns don't fit alongside the brand + theme + account controls at phone width,
// so below `sm` they collapse into this single hamburger. It reuses the DropdownMenu
// primitives (which open on tap) and folds the nav sections — Cards, Collection and
// Wish list — into one flat menu, driven by the same cached games registry as MainNav
// so a new TCG shows up here automatically. Real <RouterLink> anchors (via as-child)
// keep the links keyboard- and middle-click-friendly, and reka closes the menu on
// navigation.
const { data } = useGamesQuery()
const games = computed(() => data.value?.data ?? [])
</script>

<template>
  <DropdownMenu>
    <DropdownMenuTrigger as-child>
      <Button variant="ghost" size="icon" aria-label="Open navigation menu">
        <Menu />
      </Button>
    </DropdownMenuTrigger>
    <DropdownMenuContent align="start" class="w-56">
      <DropdownMenuLabel class="flex items-center gap-2">
        <Layers class="size-4" aria-hidden="true" />
        Cards
      </DropdownMenuLabel>
      <DropdownMenuItem as-child>
        <RouterLink to="/cards">Browse all games</RouterLink>
      </DropdownMenuItem>
      <DropdownMenuItem v-for="game in games" :key="`cards-${game.id}`" as-child>
        <RouterLink :to="`/cards/${game.id}`">{{ game.name }}</RouterLink>
      </DropdownMenuItem>

      <template v-if="games.length">
        <DropdownMenuLabel class="flex items-center gap-2">
          <Package class="size-4" aria-hidden="true" />
          Sealed products
        </DropdownMenuLabel>
        <DropdownMenuItem v-for="game in games" :key="`sealed-${game.id}`" as-child>
          <RouterLink :to="`/cards/${game.id}/sealed`">{{ game.name }}</RouterLink>
        </DropdownMenuItem>
      </template>

      <DropdownMenuSeparator />

      <DropdownMenuLabel class="flex items-center gap-2">
        <Library class="size-4" aria-hidden="true" />
        Collection
      </DropdownMenuLabel>
      <DropdownMenuItem as-child>
        <RouterLink to="/collection">All collections</RouterLink>
      </DropdownMenuItem>
      <DropdownMenuItem v-for="game in games" :key="`collection-${game.id}`" as-child>
        <RouterLink :to="`/collection/${game.id}`">{{ game.name }}</RouterLink>
      </DropdownMenuItem>

      <DropdownMenuSeparator />

      <DropdownMenuLabel class="flex items-center gap-2">
        <Heart class="size-4" aria-hidden="true" />
        Wish list
      </DropdownMenuLabel>
      <DropdownMenuItem as-child>
        <RouterLink to="/wishlist">All wish lists</RouterLink>
      </DropdownMenuItem>
      <DropdownMenuItem v-for="game in games" :key="`wishlist-${game.id}`" as-child>
        <RouterLink :to="`/wishlist/${game.id}`">{{ game.name }}</RouterLink>
      </DropdownMenuItem>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
