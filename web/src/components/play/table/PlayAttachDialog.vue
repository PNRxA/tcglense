<script setup lang="ts">
import { computed } from 'vue'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import PlayCard from '@/components/play/table/PlayCard.vue'
import { usePlayTableContext } from '@/composables/usePlayTable'
import { cardName } from '@/lib/playTable'
import type { PlayCardView } from '@/lib/api/play'

// "Attach to…" — the picker behind an aura, an equipment, or anything else that goes under
// something else.
//
// It is a dialog rather than a drag because attachment is a *relationship*, not a position:
// dropping one card on another is ambiguous (did you mean to attach it, or just to put it
// there?), and the relationship has to survive the host being moved afterwards. The candidate
// list is every permanent on the table except the card itself — attaching across the table is
// normal (my Pacifism on their creature), so opponents' boards are included.
const table = usePlayTableContext()

const card = computed<PlayCardView | null>(() => {
  const id = table.attachFor.value
  return id === null ? null : (table.store.card(id) ?? null)
})

const candidates = computed<PlayCardView[]>(() => {
  const subject = card.value
  if (!subject) return []
  return table.store.seats.flatMap((seat) =>
    (table.store.cardsIn(seat.id, 'battlefield') as PlayCardView[]).filter(
      (other) => other.id !== subject.id && other.attached_to !== subject.id,
    ),
  )
})

function attach(to: number | null) {
  const subject = card.value
  if (!subject) return
  table.send({ type: 'attach', card: subject.id, to })
  table.attachFor.value = null
}
</script>

<template>
  <Dialog
    :open="card !== null"
    @update:open="(value: boolean) => !value && (table.attachFor.value = null)"
  >
    <DialogContent
      anchor="top"
      class="bg-background flex max-h-[85dvh] w-[min(94vw,48rem)] flex-col rounded-xl border p-5 shadow-xl"
    >
      <DialogTitle>Attach {{ card ? cardName(card) : '' }}</DialogTitle>
      <DialogDescription>Pick the permanent it goes under.</DialogDescription>

      <div class="mt-4 min-h-0 flex-1 overflow-y-auto">
        <p v-if="candidates.length === 0" class="text-muted-foreground text-sm">
          There is nothing else on the battlefield yet.
        </p>
        <ul v-else class="grid grid-cols-[repeat(auto-fill,minmax(6rem,1fr))] gap-3">
          <li v-for="candidate in candidates" :key="candidate.id">
            <PlayCard
              :card="candidate"
              :game="table.store.game"
              size="small"
              @click="attach(candidate.id)"
            />
          </li>
        </ul>
      </div>

      <div class="mt-4 flex justify-end gap-2">
        <Button variant="outline" @click="table.attachFor.value = null">Cancel</Button>
      </div>
    </DialogContent>
  </Dialog>
</template>
