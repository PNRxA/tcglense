<script setup lang="ts">
import { computed } from 'vue'
import { useQuery } from '@tanstack/vue-query'
import { ChevronDown, Layers } from '@lucide/vue'
import { RouterLink, useRouter } from 'vue-router'
import { Button, buttonVariants } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { listGames } from '@/lib/api'

const router = useRouter()

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
  <div class="flex items-center">
    <!-- Clicking the parent goes to the games landing page. -->
    <RouterLink to="/cards" :class="buttonVariants({ variant: 'ghost', size: 'sm' })">
      <Layers />
      Cards
    </RouterLink>
    <!-- The chevron opens the game shortcut menu. -->
    <DropdownMenu>
      <DropdownMenuTrigger as-child>
        <Button variant="ghost" size="icon-sm">
          <ChevronDown class="size-4 opacity-60" />
          <span class="sr-only">Choose a game</span>
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" class="w-56">
        <DropdownMenuLabel>Games</DropdownMenuLabel>
        <DropdownMenuItem
          v-for="game in games"
          :key="game.id"
          @select="() => router.push(`/cards/${game.id}`)"
        >
          {{ game.name }}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  </div>
</template>
