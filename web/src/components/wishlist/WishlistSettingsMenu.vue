<script setup lang="ts">
import { toRef } from 'vue'
import { Settings2 } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import WishlistVisibilityControl from '@/components/wishlist/WishlistVisibilityControl.vue'

// The per-game wish-list landing's settings menu (issue #493) — the wish-list twin of
// CollectionSettingsMenu. A wish list is a shopping list with no landing display prefs, so the
// menu holds only the sharing control (public/private + share link). Kept as a gear popover for
// UX symmetry with the collection landing.
const props = defineProps<{ game: string }>()
const game = toRef(props, 'game')
</script>

<template>
  <Popover>
    <PopoverTrigger as-child>
      <Button variant="outline" size="sm" class="shrink-0">
        <Settings2 />
        Settings
      </Button>
    </PopoverTrigger>
    <PopoverContent align="end" class="w-80 space-y-4">
      <section class="space-y-2">
        <h3 class="text-muted-foreground text-xs font-medium tracking-wide uppercase">Sharing</h3>
        <WishlistVisibilityControl :game="game" />
      </section>
    </PopoverContent>
  </Popover>
</template>
