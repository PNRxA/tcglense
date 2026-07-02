<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { Expand, X } from '@lucide/vue'
import { Dialog, DialogClose, DialogContent, DialogTitle } from '@/components/ui/dialog'
import CardDetailContent from '@/components/cards/CardDetailContent.vue'

// The card-detail modal: clicking a card in any browse grid (catalog, collection,
// wish list, other-printings) adds `?card=<id>` to the current URL instead of leaving
// the page (see CardTile's click handler), and this dialog — mounted once in App.vue —
// overlays the full detail body wherever that param appears. Driving it off the URL
// keeps the browser's Back button closing/reopening it, makes an open card shareable
// (a fresh load shows the modal over the list), and leaves the real card page
// (`/cards/:game/cards/:id`) as the canonical, crawlable home the header links to.
// Clicking another printing inside the modal just rewrites `?card`, swapping the card
// in place. The game comes from the route's `:game` param — every grid page has one;
// on a route without it the param is ignored.
const route = useRoute()
const router = useRouter()

const cardId = computed(() => {
  const value = route.query.card
  return typeof value === 'string' && value ? value : null
})
const game = computed(() => {
  const value = route.params.game
  return typeof value === 'string' && value ? value : null
})
const open = computed(() => cardId.value !== null && game.value !== null)

// Closing (X / ESC / overlay click) drops the param but keeps the rest of the URL —
// list state (page, search, sort, ghosts…) is untouched underneath.
function close() {
  const next = { ...route.query }
  delete next.card
  void router.push({ query: next })
}
function onOpenChange(value: boolean) {
  if (!value) close()
}
</script>

<template>
  <Dialog :open="open" @update:open="onOpenChange">
    <DialogContent
      v-if="game && cardId"
      class="bg-background max-h-[90svh] w-[min(96vw,64rem)] overflow-y-auto rounded-xl border p-5 shadow-lg sm:p-8"
    >
      <DialogTitle class="sr-only">Card details</DialogTitle>

      <!-- Escape hatches, pinned above the scrolling body: the canonical card page
           (with its meta, back link, and shareable URL bar) and a plain close. -->
      <div class="mb-4 flex items-center justify-end gap-1">
        <RouterLink
          :to="`/cards/${game}/cards/${cardId}`"
          class="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex items-center gap-1.5 rounded-md px-2 py-1.5 text-xs font-medium"
        >
          <Expand class="size-3.5" aria-hidden="true" />
          Open full page
        </RouterLink>
        <DialogClose
          class="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex items-center justify-center rounded-md p-1.5"
          aria-label="Close"
        >
          <X class="size-4" aria-hidden="true" />
        </DialogClose>
      </div>

      <CardDetailContent :game="game" :id="cardId" />
    </DialogContent>
  </Dialog>
</template>
