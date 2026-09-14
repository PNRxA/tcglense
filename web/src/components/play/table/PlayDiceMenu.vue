<script setup lang="ts">
import { Dices } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { usePlayTableContext } from '@/composables/usePlayTable'
import { PLAY_DICE } from '@/lib/playTable'

// Dice and the coin. The *result* never appears here — it goes to the log, where everyone at
// the table can see it. A roll only one player can read isn't a roll, it's a claim.
const table = usePlayTableContext()
</script>

<template>
  <DropdownMenu>
    <DropdownMenuTrigger as-child>
      <Button variant="outline" size="sm" aria-label="Roll a die">
        <Dices class="size-4" />
        <span class="hidden sm:inline">Roll</span>
      </Button>
    </DropdownMenuTrigger>
    <DropdownMenuContent align="end">
      <DropdownMenuItem v-for="sides in PLAY_DICE" :key="sides" @select="table.roll(sides)">
        d{{ sides }}
      </DropdownMenuItem>
      <DropdownMenuSeparator />
      <DropdownMenuItem @select="table.flipCoin()">Flip a coin</DropdownMenuItem>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
