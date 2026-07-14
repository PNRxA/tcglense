<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { ChevronLeft, ChevronRight, Expand, X } from '@lucide/vue'
import { Dialog, DialogClose, DialogContent, DialogTitle } from '@/components/ui/dialog'
import CardDetailContent from '@/components/cards/CardDetailContent.vue'
import { useCardNavStore } from '@/stores/cardNav'

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
  const param = route.params.game
  if (typeof param === 'string' && param) return param
  // A route without a `:game` path param (the public deck page) carries the game in the
  // query instead — see CardTile's click handler.
  const q = route.query.game
  return typeof q === 'string' && q ? q : null
})
const open = computed(() => cardId.value !== null && game.value !== null)

// Prev/next through the cards on the page this modal opened over (issue #275). The grid the
// user clicked from registered its ordered ids in `useCardNavStore`, so we just look the open
// card up to find its neighbours — no wraparound, and only within the current page (matching
// the underlying list). `index === -1` (a deep link, or a card on no on-page grid) means no
// nav; `total <= 1` (a lone card) hides it too.
const nav = useCardNavStore()
const position = computed(() =>
  open.value && game.value && cardId.value
    ? nav.locate(game.value, cardId.value)
    : { prev: null, next: null, index: -1, total: 0 },
)
const hasNav = computed(() => position.value.index >= 0 && position.value.total > 1)

// Stepping to another card is just rewriting `?card=` — but with `replace`, not `push`, so
// holding an arrow key (or clicking prev/next) to flip through a page of cards doesn't bury the
// list underneath dozens of history entries: Back still exits the modal in one press. The list
// keeps its scroll/search/page state throughout (issue #275).
function goTo(id: string | null) {
  if (!id) return
  void router.replace({ query: { ...route.query, card: id } })
}

// Left/right arrow keys mirror the buttons. The listener sits on the dialog's OWN content (see
// the template `@keydown`), not window, so it only fires while this modal holds focus — arrows
// over a nested overlay stacked on top (the image-zoom lightbox, a quick-add popover), or over
// the page behind a closed modal, are left untouched. It still bows out while the user is typing
// in one of the modal's fields (the quantity inputs) or holding a modifier (a browser/OS
// shortcut). reka's Dialog owns Escape and the focus trap; arrow keys are ours.
function onKeydown(event: KeyboardEvent) {
  if (event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) return
  const target = event.target as HTMLElement | null
  if (
    target &&
    (target.isContentEditable ||
      target.tagName === 'INPUT' ||
      target.tagName === 'TEXTAREA' ||
      target.tagName === 'SELECT')
  ) {
    return
  }
  if (event.key === 'ArrowLeft' && position.value.prev) {
    event.preventDefault()
    goTo(position.value.prev)
  } else if (event.key === 'ArrowRight' && position.value.next) {
    event.preventDefault()
    goTo(position.value.next)
  }
}

// Closing (X / ESC / overlay click) drops the param but keeps the rest of the URL —
// list state (page, search, sort, ghosts…) is untouched underneath.
function close() {
  const next = { ...route.query }
  delete next.card
  // Drop the game the public deck page carried alongside `card` (a no-op elsewhere, where
  // the game lives in the path, not the query).
  delete next.game
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
      @keydown="onKeydown"
    >
      <DialogTitle class="sr-only">Card details</DialogTitle>

      <!-- Header row, pinned above the scrolling body: prev/next through the page's cards on
           the left (issue #275), the canonical card page + a plain close on the right. The
           left group stays present (an empty flex item) even without nav so the escape hatches
           keep their right edge. -->
      <div class="mb-4 flex items-center justify-between gap-2">
        <!-- Prev/next through the cards on the page underneath — arrow keys mirror these (see
             onKeydown). Hidden when the open card isn't part of a known on-page grid (a deep
             link, or a lone card), so there's never a dead control. -->
        <div class="flex items-center gap-1">
          <template v-if="hasNav">
            <button
              type="button"
              class="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex items-center justify-center rounded-md p-1.5 disabled:pointer-events-none disabled:opacity-40"
              :disabled="!position.prev"
              aria-label="Previous card"
              title="Previous card (←)"
              @click="goTo(position.prev)"
            >
              <ChevronLeft class="size-4" aria-hidden="true" />
            </button>
            <button
              type="button"
              class="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex items-center justify-center rounded-md p-1.5 disabled:pointer-events-none disabled:opacity-40"
              :disabled="!position.next"
              aria-label="Next card"
              title="Next card (→)"
              @click="goTo(position.next)"
            >
              <ChevronRight class="size-4" aria-hidden="true" />
            </button>
            <span
              class="text-muted-foreground ml-1 text-xs tabular-nums"
              aria-live="polite"
              :aria-label="`Card ${position.index + 1} of ${position.total}`"
            >
              {{ position.index + 1 }} / {{ position.total }}
            </span>
          </template>
        </div>

        <div class="flex items-center gap-1">
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
      </div>

      <CardDetailContent :game="game" :id="cardId" />
    </DialogContent>
  </Dialog>
</template>
