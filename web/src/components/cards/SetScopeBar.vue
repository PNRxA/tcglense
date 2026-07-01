<script setup lang="ts">
import { Check, ChevronDown, Layers } from '@lucide/vue'
import { buttonVariants } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { cn } from '@/lib/utils'

// The banner offering to fold a set's related sub-sets (tokens, promos, decks, …)
// into one listing, plus a picker to drop into any single set in the group. All the
// grouping derivations come in as props (from useSetGrouping); it emits `toggle`
// (fold in / return to the origin set) and `select` (jump to a specific set).
defineProps<{
  includeRelated: boolean
  isMainSet: boolean
  mainName: string
  relatedCount: number
  setsWord: string
  memberOptions: { code: string; name: string; label: string }[]
  activeSetCode: string | null
  originName: string
}>()

const emit = defineEmits<{ toggle: [on: boolean]; select: [code: string] }>()
</script>

<template>
  <div
    class="bg-muted/40 mb-6 flex flex-wrap items-center justify-between gap-3 rounded-lg border p-3"
  >
    <p class="text-muted-foreground text-sm">
      <template v-if="includeRelated">
        Showing {{ mainName }} with its {{ relatedCount }} related {{ setsWord }}.
      </template>
      <template v-else-if="isMainSet">
        This set has {{ relatedCount }} related {{ setsWord }} — tokens, promos, decks and more.
      </template>
      <template v-else>
        Part of {{ mainName }} — {{ relatedCount }} related {{ setsWord }} in this group.
      </template>
    </p>
    <!-- A split button in both modes. The main action toggles the grouped view —
         fold the related sub-sets in, or (when grouped) return to the set you came
         from — while the caret always opens a menu to drop straight into any single
         set in the group. -->
    <div class="flex">
      <button
        v-if="!includeRelated"
        type="button"
        :class="cn(buttonVariants({ variant: 'default', size: 'sm' }), 'rounded-r-none')"
        @click="emit('toggle', true)"
      >
        <Layers />
        View all together
      </button>
      <button
        v-else
        type="button"
        :class="cn(buttonVariants({ variant: 'outline', size: 'sm' }), 'rounded-r-none')"
        :title="originName ? `View just ${originName}` : undefined"
        @click="emit('toggle', false)"
      >
        <Layers />
        View just this set
      </button>
      <DropdownMenu>
        <DropdownMenuTrigger as-child>
          <button
            type="button"
            :class="
              cn(
                buttonVariants({
                  variant: includeRelated ? 'outline' : 'default',
                  size: 'icon-sm',
                }),
                '-ml-px rounded-l-none',
                // The outline variant's border already divides the two halves; the
                // filled variant has none, so add a faint seam ourselves (else the
                // chevron reads as part of one solid block — no hover on touch to
                // reveal it).
                !includeRelated && 'border-l border-l-primary-foreground/20',
              )
            "
            aria-label="Jump to a set in this group"
          >
            <ChevronDown />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" class="max-w-64">
          <DropdownMenuLabel>Jump to a set</DropdownMenuLabel>
          <DropdownMenuSeparator />
          <DropdownMenuItem
            v-for="option in memberOptions"
            :key="option.code"
            :title="option.name"
            @select="emit('select', option.code)"
          >
            <span class="min-w-0 truncate">{{ option.label }}</span>
            <Check v-if="option.code === activeSetCode" class="ml-auto shrink-0" />
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  </div>
</template>
