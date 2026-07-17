<script setup lang="ts">
import { RouterLink } from 'vue-router'
import { Globe, Layers, MoreVertical } from '@lucide/vue'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import type { Deck, DeckFolder } from '@/lib/api'

// One deck in the game's deck list: a link to the builder plus a "…" menu to move it
// between folders or delete it. The parent owns the mutations (so the menu just emits).
const props = defineProps<{ deck: Deck; game: string; folders: DeckFolder[] }>()
const emit = defineEmits<{ move: [folderId: number | null]; remove: [] }>()
</script>

<template>
  <div class="bg-card relative rounded-lg border transition hover:border-primary/50">
    <RouterLink :to="`/decks/${game}/${deck.id}`" class="block p-4 pr-10">
      <div class="flex items-center gap-2">
        <Layers class="text-muted-foreground size-4 shrink-0" aria-hidden="true" />
        <p class="truncate font-medium" :title="deck.name">{{ deck.name }}</p>
        <Globe
          v-if="deck.is_public"
          class="text-muted-foreground size-3.5 shrink-0"
          aria-label="Public"
        />
      </div>
      <p class="text-muted-foreground mt-1 text-sm">
        {{ deck.card_count }} card{{ deck.card_count === 1 ? '' : 's' }}
        <span v-if="deck.format"> · {{ deck.format }}</span>
      </p>
    </RouterLink>

    <DropdownMenu>
      <DropdownMenuTrigger
        class="text-muted-foreground hover:text-foreground absolute top-3 right-2 rounded p-1 outline-none focus-visible:ring-2 focus-visible:ring-ring"
        aria-label="Deck actions"
      >
        <MoreVertical class="size-4" />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuLabel>Move to folder</DropdownMenuLabel>
        <DropdownMenuItem v-if="deck.folder_id != null" @click="emit('move', null)">
          Remove from folder
        </DropdownMenuItem>
        <DropdownMenuItem
          v-for="folder in props.folders.filter((f) => f.id !== deck.folder_id)"
          :key="folder.id"
          @click="emit('move', folder.id)"
        >
          {{ folder.name }}
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem class="text-destructive" @click="emit('remove')">
          Delete deck
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  </div>
</template>
