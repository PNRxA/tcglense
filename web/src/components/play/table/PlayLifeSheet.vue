<script setup lang="ts">
import { Sheet, SheetContent, SheetTitle } from '@/components/ui/sheet'
import PlayLifePanel from '@/components/play/table/PlayLifePanel.vue'
import { usePlayTableContext } from '@/composables/usePlayTable'

// My totals, on a phone.
//
// The full panel is a 176px-wide column of steppers — a quarter of a phone's width, spent on
// numbers that change a few times a turn. So on a phone it moves into a sheet, opened from the
// life chip in the turn bar. The chip keeps the part you look at constantly (the number) and
// the ±1 you press most; everything else — poison, energy, experience, commander damage per
// opponent — is one tap away in here.
const table = usePlayTableContext()
</script>

<template>
  <Sheet
    :open="table.lifeOpen.value"
    @update:open="(value: boolean) => (table.lifeOpen.value = value)"
  >
    <SheetContent side="bottom" class="max-h-[85dvh] overflow-y-auto p-3">
      <SheetTitle class="text-sm">Your totals</SheetTitle>
      <!-- The same panel the desktop shows down the right-hand side, widened to the sheet. -->
      <PlayLifePanel full class="mt-2" />
    </SheetContent>
  </Sheet>
</template>
