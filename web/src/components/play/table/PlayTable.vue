<script setup lang="ts">
import { computed } from 'vue'
import { MessageSquare } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Sheet, SheetContent, SheetTitle } from '@/components/ui/sheet'
import PlayAttachDialog from '@/components/play/table/PlayAttachDialog.vue'
import PlayBattlefield from '@/components/play/table/PlayBattlefield.vue'
import PlayCard from '@/components/play/table/PlayCard.vue'
import PlayCardPreview from '@/components/play/table/PlayCardPreview.vue'
import PlayHand from '@/components/play/table/PlayHand.vue'
import PlayLifePanel from '@/components/play/table/PlayLifePanel.vue'
import PlayLifeSheet from '@/components/play/table/PlayLifeSheet.vue'
import PlayLog from '@/components/play/table/PlayLog.vue'
import PlayOpponentBoard from '@/components/play/table/PlayOpponentBoard.vue'
import PlayOpponentStrip from '@/components/play/table/PlayOpponentStrip.vue'
import PlayTokenDialog from '@/components/play/table/PlayTokenDialog.vue'
import PlayTurnBar from '@/components/play/table/PlayTurnBar.vue'
import PlayZoneRail from '@/components/play/table/PlayZoneRail.vue'
import PlayZoneViewer from '@/components/play/table/PlayZoneViewer.vue'
import { usePlayTable } from '@/composables/usePlayTable'

// The table itself.
//
// It takes **no props**: everything it draws is the live room in `stores/playRoom`, and
// everything it does goes back out through that store's socket. The room view mounts it
// `fixed inset-0` and listens for one event — `leave` — because navigation is the view's
// business and the table's business is the game.
//
// The layout is the seating plan, top to bottom: the turn bar (whose turn, what phase), the
// opponents across from you, then your own half — rail, board, totals — and your hand along
// the near edge where your hand actually is. The log sits down the right on a laptop and
// becomes a sheet on a phone, because at 400px a permanent 18rem column is most of the table.
//
// **A phone is a different seating plan, not a smaller one.** Measured at 390×844, the desktop
// arrangement left the battlefield a 110px sliver: the zone rail and the life panel kept their
// widths, the opponent boards took a third of the height and the turn bar wrapped to three
// rows. So below `sm` the table re-forms around the one thing that has to be big — your board:
// the opponents become a one-line chip strip (`PlayOpponentStrip`), the rail turns ninety
// degrees into a row of four tiles above the hand, the life panel moves into a sheet behind a
// chip on the bar, and the battlefield takes everything that is left.
//
// The switch is `compact`, which is **narrow or short** rather than narrow alone: a phone held
// sideways is 844px wide — nobody's idea of narrow — and got the desktop layout, which left it
// a 2px-tall battlefield. A short viewport drops the opponent strip too, because at 390px of
// height there is none to spend on it.
//
// Everything interactive is created once here (`usePlayTable`) and provided to the subtree, so
// a drag that starts in the hand and ends on the battlefield is one state machine rather than
// a conversation between two components.
const emit = defineEmits<{ leave: [] }>()

const table = usePlayTable()
const store = table.store

const dragCard = computed(() => {
  const state = table.drag.value
  if (!state || !state.moved) return null
  return store.card(state.cardId) ?? null
})

const ghostStyle = computed(() => {
  const state = table.drag.value
  if (!state) return {}
  return {
    left: `${state.x - state.grabX}px`,
    top: `${state.y - state.grabY}px`,
    width: `${state.width}px`,
  }
})
</script>

<template>
  <div class="bg-muted/40 text-foreground flex h-full w-full flex-col overflow-hidden">
    <PlayTurnBar @leave="emit('leave')" />

    <div class="flex min-h-0 flex-1">
      <div class="flex min-h-0 min-w-0 flex-1 flex-col gap-1 p-1 sm:gap-1.5 sm:p-1.5">
        <!-- Across the table. On a laptop, boards that scroll sideways rather than shrinking:
          four opponents are four boards you scroll to, not four too small to read. On a phone,
          a chip each — and on a phone held sideways, not even that. -->
        <PlayOpponentStrip v-if="table.compact.value && !table.isShort.value" />
        <div
          v-if="!table.compact.value && store.opponents.length > 0"
          class="flex shrink-0 gap-1.5 overflow-x-auto pb-1"
          aria-label="Opponents"
        >
          <PlayOpponentBoard
            v-for="opponent in store.opponents"
            :key="opponent.id"
            :seat="opponent"
          />
        </div>

        <!-- My half. The rail and the totals flank the board on a laptop; on a phone the board
          gets the whole width and they move below it and into a sheet. -->
        <div class="flex min-h-0 flex-1 gap-1.5">
          <PlayZoneRail v-if="!table.compact.value" />
          <div class="min-h-0 min-w-0 flex-1">
            <PlayBattlefield v-if="store.mySeatId !== null" :seat-id="store.mySeatId" />
            <div
              v-else
              class="bg-card text-muted-foreground grid h-full place-items-center rounded-lg border text-sm"
            >
              You're watching this table.
            </div>
          </div>
          <PlayLifePanel v-if="!table.compact.value" />
        </div>

        <PlayZoneRail v-if="table.compact.value" />
        <PlayHand v-if="store.mySeatId !== null" />
      </div>

      <!-- A wrapper, not a `hidden lg:flex` on the panel itself: the panel already sets its
        own `display`, and two display utilities on one element are decided by stylesheet
        order rather than by which one you wrote last. -->
      <div class="hidden lg:flex">
        <PlayLog />
      </div>
    </div>

    <!-- Below the log column's breakpoint the log is a sheet: the same panel, out of the way
      until it's wanted. On a tablet it opens from a floating button; on a phone that button
      would sit on top of the hand, so the turn bar carries it instead. -->
    <Sheet
      :open="table.logSheetOpen.value"
      @update:open="(value: boolean) => (table.logSheetOpen.value = value)"
    >
      <SheetContent
        :side="table.compact.value ? 'bottom' : 'right'"
        :class="table.compact.value ? 'h-[70dvh] p-0' : 'w-[min(90vw,22rem)] p-0'"
        data-play-log-sheet
      >
        <SheetTitle class="sr-only">Game log</SheetTitle>
        <PlayLog :in-sheet="true" />
      </SheetContent>
    </Sheet>
    <Button
      v-if="!table.compact.value"
      variant="outline"
      size="icon"
      class="fixed right-3 bottom-3 z-30 rounded-full shadow-lg lg:hidden"
      aria-label="Open the log"
      @click="table.logSheetOpen.value = true"
    >
      <MessageSquare class="size-4" />
    </Button>

    <!-- The card under the finger. `pointer-events-none` is load-bearing: the drop target is
      whatever the document reports under the pointer, and a ghost that answered would be it. -->
    <div
      v-if="dragCard"
      class="pointer-events-none fixed z-50 opacity-90"
      :style="ghostStyle"
      aria-hidden="true"
    >
      <PlayCard :card="dragCard" :game="store.game" size="small" :interactive="false" />
    </div>

    <PlayCardPreview />
    <PlayLifeSheet />
    <PlayZoneViewer />
    <PlayTokenDialog />
    <PlayAttachDialog />
  </div>
</template>
