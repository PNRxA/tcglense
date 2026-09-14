<script setup lang="ts">
import { computed } from 'vue'
import { MoreHorizontal, X } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { usePlayTableContext } from '@/composables/usePlayTable'
import { cardName } from '@/lib/playTable'

// What to do with the card you just tapped.
//
// On a phone there is no hover to reveal an affordance, no right-click, and a first-time player
// has no reason to guess at a double-tap — so a **tap selects a hand card and this bar names
// the three things people actually do with one**. Double-tap still plays it, and a long press
// still opens the full context menu; this is the discoverable route, not a replacement.
//
// "More…" is a dropdown rather than a second bar because the long tail is exactly the context
// menu's list, built from the same `menuFor` descriptors — one vocabulary, rendered twice.
//
// It is an **overlay**, absolutely placed over the top of the hand, so showing it never changes
// the height of anything: the desktop layout measures the same whether a card is selected or
// not, which is the only reason it can be on for every pointer type rather than phone-only. It
// is centred and capped rather than stretched, because a 1140px-wide bar holding four buttons
// is a banner, not a toolbar.
const table = usePlayTableContext()

const card = computed(() => table.selectedHandCard.value)

/** Everything the context menu would offer, minus the three already on the bar. */
const QUICK = new Set(['play', 'play_face_down', 'to_graveyard'])
const more = computed(() => {
  const subject = card.value
  if (!subject) return []
  return table.menuFor(subject, 'hand').filter((action) => !QUICK.has(action.id))
})

function run(id: 'play' | 'play_face_down' | 'to_graveyard') {
  const subject = card.value
  if (!subject) return
  table.runMenuAction(id, subject)
  table.clearSelection()
}
</script>

<template>
  <div
    v-if="card"
    class="bg-popover text-popover-foreground pointer-events-auto absolute bottom-full left-1/2 z-30 mb-1 flex w-[min(30rem,calc(100%-0.5rem))] -translate-x-1/2 items-center gap-1 rounded-lg border p-1 shadow-lg"
    role="group"
    :aria-label="`Actions for ${cardName(card)}`"
  >
    <span class="text-muted-foreground min-w-0 flex-1 truncate px-1 text-xs">
      {{ cardName(card) }}
    </span>
    <Button size="sm" class="px-2" :disabled="!table.canAct.value" @click="run('play')"
      >Play</Button
    >
    <Button
      variant="outline"
      size="sm"
      class="px-2"
      :disabled="!table.canAct.value"
      @click="run('play_face_down')"
    >
      Face down
    </Button>
    <Button
      variant="outline"
      size="sm"
      class="px-2"
      :disabled="!table.canAct.value"
      @click="run('to_graveyard')"
    >
      Bin
    </Button>
    <DropdownMenu v-if="more.length > 0">
      <DropdownMenuTrigger as-child>
        <Button variant="outline" size="icon-sm" aria-label="More actions">
          <MoreHorizontal class="size-4" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" side="top">
        <DropdownMenuItem
          v-for="action in more"
          :key="action.id"
          :variant="action.destructive ? 'destructive' : 'default'"
          @select="
            () => {
              table.runMenuAction(action.id, card!)
              table.clearSelection()
            }
          "
        >
          {{ action.label }}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
    <Button
      variant="ghost"
      size="icon-sm"
      aria-label="Clear selection"
      @click="table.clearSelection()"
    >
      <X class="size-4" />
    </Button>
  </div>
</template>
