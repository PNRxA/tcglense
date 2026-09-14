<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { AlertTriangle, PanelRightClose, PanelRightOpen, Send } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { usePlayTableContext } from '@/composables/usePlayTable'
import { logLine, seatColor } from '@/lib/playTable'
import type { PlayLogEntry } from '@/lib/api/play'

// What just happened, and what people are saying about it.
//
// A manual table enforces nothing, so the log **is** the rules: it is the only record that
// someone drew two, that a commander went back to the zone, that the die came up 17. That
// makes it a first-class panel rather than a toast stream — it scrolls, it keeps 300 entries,
// and it never auto-dismisses.
//
// Chat shares the column on purpose. Splitting them would mean watching two scrollers for one
// conversation ("I'll pass" / *passed the turn*), so they interleave and are told apart by
// weight and punctuation instead (`logLine` does the punctuation).
//
// Errors are the one transient thing here: a rejected action ("not your turn") is about the
// button you just pressed, not about the game, so it surfaces at the top of the panel and
// clears itself.
const table = usePlayTableContext()

const draft = ref('')
const list = ref<HTMLElement | null>(null)

const entries = computed<PlayLogEntry[]>(() => table.store.log)

/** The seat colour for an entry — identity is never colour alone, the name is in the text. */
function colorFor(entry: PlayLogEntry): string | undefined {
  if (entry.seat === null) return undefined
  const seat = table.store.seatById(entry.seat)
  return seat ? seatColor(seat.seat_index) : undefined
}

// A log you have to scroll to read is a log nobody reads: follow the tail.
watch(
  () => entries.value.length,
  async () => {
    await nextTick()
    const el = list.value
    if (el) el.scrollTop = el.scrollHeight
  },
)

let errorTimer: ReturnType<typeof setTimeout> | undefined
watch(
  () => table.store.lastError,
  (error) => {
    if (errorTimer !== undefined) clearTimeout(errorTimer)
    if (!error) return
    errorTimer = setTimeout(() => table.store.clearError(), 6000)
  },
)
onBeforeUnmount(() => {
  if (errorTimer !== undefined) clearTimeout(errorTimer)
})

/**
 * Inside a sheet the panel is the sheet's whole body: full width, full height, always open,
 * and without its own collapse toggle (the sheet has a close of its own).
 */
const props = defineProps<{ inSheet?: boolean }>()
const expanded = computed(() => props.inSheet === true || table.logOpen.value)

function submit() {
  table.chat(draft.value)
  draft.value = ''
}
</script>

<template>
  <aside
    class="bg-card flex shrink-0 flex-col"
    :class="
      props.inSheet ? 'h-full w-full' : table.logOpen.value ? 'w-72 border-l' : 'w-10 border-l'
    "
    aria-label="Game log"
  >
    <div class="flex items-center gap-1 border-b p-1.5">
      <Button
        v-if="!props.inSheet"
        variant="ghost"
        size="icon-sm"
        :aria-label="table.logOpen.value ? 'Hide the log' : 'Show the log'"
        :aria-expanded="table.logOpen.value"
        @click="table.logOpen.value = !table.logOpen.value"
      >
        <PanelRightClose v-if="table.logOpen.value" class="size-4" />
        <PanelRightOpen v-else class="size-4" />
      </Button>
      <h2 v-if="expanded" class="text-sm font-medium">Log</h2>
    </div>

    <template v-if="expanded">
      <div
        v-if="table.store.lastError"
        class="bg-destructive/10 text-destructive flex items-start gap-2 p-2 text-xs"
        role="status"
      >
        <AlertTriangle class="mt-px size-3.5 shrink-0" />
        <span class="min-w-0 flex-1">{{ table.store.lastError.message }}</span>
        <button type="button" class="underline" @click="table.store.clearError()">Dismiss</button>
      </div>

      <ol ref="list" class="min-h-0 flex-1 space-y-0.5 overflow-y-auto p-2 text-xs">
        <li v-if="entries.length === 0" class="text-muted-foreground">Nothing has happened yet.</li>
        <li
          v-for="entry in entries"
          :key="entry.id"
          :class="
            entry.kind === 'chat'
              ? 'text-foreground'
              : entry.kind === 'system'
                ? 'text-muted-foreground italic'
                : 'text-muted-foreground'
          "
        >
          <span
            v-if="entry.seat !== null"
            class="mr-1 inline-block size-1.5 rounded-full align-middle"
            :style="{ backgroundColor: colorFor(entry) }"
            aria-hidden="true"
          />
          <span :class="entry.kind === 'chat' ? 'font-medium' : ''">
            {{ logLine(entry, table.store.seats) }}
          </span>
        </li>
      </ol>

      <form class="flex gap-1 border-t p-1.5" @submit.prevent="submit">
        <Input v-model="draft" class="h-8" placeholder="Say something" aria-label="Chat message" />
        <Button type="submit" variant="outline" size="icon-sm" aria-label="Send">
          <Send class="size-4" />
        </Button>
      </form>
    </template>
  </aside>
</template>
