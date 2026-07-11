<script setup lang="ts">
import { computed } from 'vue'
import { Code, Heart, Layers, Library, Menu, Package, ScanLine } from '@lucide/vue'
import { RouterLink, useRouter } from 'vue-router'
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
import { prefetchRouteChunks } from '@/lib/prefetch'

// The mobile counterpart to MainNav: the top-bar's Cards/Collection/Wish list
// dropdowns don't fit alongside the brand + theme + account controls at phone width,
// so below `md` they collapse into this single hamburger. It reuses the DropdownMenu
// primitives (which open on tap) and folds the nav sections — Cards + Sealed (the
// desktop "Products" menu, grouped together here), Collection and Wish list — into one
// flat menu, driven by the same cached games registry as MainNav
// so a new TCG shows up here automatically. Real <RouterLink> anchors (via as-child)
// keep the links keyboard- and middle-click-friendly, and reka closes the menu on
// navigation.
const { data } = useGamesQuery()
const games = computed(() => data.value?.data ?? [])

// Touch has no hover, so warm every nav destination's JS chunk when the hamburger opens
// (see lib/prefetch.ts — chunks only, never data/images). The tap-to-tap gap covers the
// fetch. Per-game links all map to one view chunk, so iterating games costs nothing.
const router = useRouter()
function warmAll(open: boolean) {
  if (!open) return
  for (const to of ['/cards', '/sealed', '/collection', '/scan', '/wishlist', '/docs']) {
    prefetchRouteChunks(router, to)
  }
  for (const game of games.value) {
    prefetchRouteChunks(router, `/cards/${game.id}`)
    prefetchRouteChunks(router, `/sealed/${game.id}`)
    prefetchRouteChunks(router, `/collection/${game.id}`)
    prefetchRouteChunks(router, `/wishlist/${game.id}`)
  }
}
</script>

<template>
  <DropdownMenu @update:open="warmAll">
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

      <DropdownMenuLabel class="flex items-center gap-2">
        <Package class="size-4" aria-hidden="true" />
        Sealed
      </DropdownMenuLabel>
      <DropdownMenuItem as-child>
        <RouterLink to="/sealed">Browse all games</RouterLink>
      </DropdownMenuItem>
      <DropdownMenuItem v-for="game in games" :key="`sealed-${game.id}`" as-child>
        <RouterLink :to="`/sealed/${game.id}`">{{ game.name }}</RouterLink>
      </DropdownMenuItem>

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
      <DropdownMenuItem as-child>
        <RouterLink to="/scan">
          <ScanLine class="size-4" aria-hidden="true" />
          Scan cards
        </RouterLink>
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

      <DropdownMenuSeparator />

      <DropdownMenuItem as-child>
        <RouterLink to="/docs">
          <Code class="size-4" aria-hidden="true" />
          API docs
        </RouterLink>
      </DropdownMenuItem>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
