<script setup lang="ts">
import { computed } from 'vue'
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from '@/components/ui/context-menu'
import { usePlayTableContext } from '@/composables/usePlayTable'
import { PLAY_MENU_GROUPS, cardName, type PlayMenuAction } from '@/lib/playTable'
import type { PlayCardView, PlayZone } from '@/lib/api/play'

// The verbs of one card, on right-click / long-press / the Menu key.
//
// The list itself is computed by `menuActionsFor` in `lib/playTable` rather than by `v-if`s
// here, because "what may this card do" is a six-zone × mine/theirs × format matrix — as
// markup it would be unreadable and untestable, and the first thing to rot would be the
// entries that only appear for someone else's permanent. This component's whole job is to
// render the descriptors in group order and hand the chosen id back to the engine.
const props = defineProps<{ card: PlayCardView; zone: PlayZone }>()

const table = usePlayTableContext()

const actions = computed(() => table.menuFor(props.card, props.zone))

/** The entries, bucketed into the runs a separator goes between. */
const groups = computed(() =>
  PLAY_MENU_GROUPS.map((group) => ({
    group,
    items: actions.value.filter((action: PlayMenuAction) => action.group === group),
  })).filter((bucket) => bucket.items.length > 0),
)
</script>

<template>
  <ContextMenu>
    <ContextMenuTrigger :disabled="actions.length === 0">
      <slot />
    </ContextMenuTrigger>
    <ContextMenuContent v-if="actions.length > 0" class="w-52">
      <ContextMenuLabel>{{ cardName(card) }}</ContextMenuLabel>
      <template v-for="(bucket, index) in groups" :key="bucket.group">
        <ContextMenuSeparator v-if="index > 0" />
        <ContextMenuItem
          v-for="action in bucket.items"
          :key="action.id"
          :variant="action.destructive ? 'destructive' : 'default'"
          @select="table.runMenuAction(action.id, card)"
        >
          {{ action.label }}
        </ContextMenuItem>
      </template>
    </ContextMenuContent>
  </ContextMenu>
</template>
