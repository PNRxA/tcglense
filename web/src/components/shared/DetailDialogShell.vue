<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { ChevronLeft, ChevronRight, Expand, X } from '@lucide/vue'
import { Dialog, DialogClose, DialogContent, DialogTitle } from '@/components/ui/dialog'
import type { NavStoreApi } from '@/stores/nav'

// The frame both detail modals share (issues #275, #438). Clicking an item in any browse grid
// adds `?<queryKey>=<id>` to the current URL instead of leaving the page (see CardTile's and
// ProductTile's click handlers), and the modal — mounted once in App.vue — overlays the detail
// body wherever that param appears. Driving it off the URL keeps the browser's Back button
// closing/reopening it, makes an open item shareable (a fresh load shows the modal over the
// list), and leaves the real detail page as the canonical, crawlable home the header links to.
// Clicking another item inside the modal just rewrites the param, swapping the body in place.
// The game comes from the route's `:game` param — every grid page has one; on a route without
// it the modal falls back to the query (see `game` below).
//
// CardDetailDialog and ProductDetailDialog are thin wrappers: each brings its body, its
// canonical link, its registry, and its noun. The scaffold, the pinned header, prev/next + the
// arrow keys, the escape hatches, and the close rules are identical, so they live here.
const props = defineProps<{
  // The URL query key holding the open item's id ('card' / 'product') — read to open, rewritten
  // to step, removed on close. The same key App.vue latches the lazy chunk mount on.
  queryKey: string
  // This modal's item kind, lower-case ('card' / 'sealed product'). Every label is built from
  // it, so the two surfaces can't drift into half-renamed wording.
  noun: string
  // The item's canonical full page: the crawlable home the "Open full page" escape hatch links
  // to, and the href its tiles keep. `/cards/:game/cards/:id` and `/sealed/:game/:id` share no
  // shape, so each wrapper hands down its own.
  canonical: (game: string, id: string) => string
  // The registry the grids underneath publish their ordered ids into (`stores/nav.ts`).
  nav: NavStoreApi
  // Further query keys this modal owns, dropped alongside `queryKey` on close — state that
  // exists only while the overlay does (the product modal's namespaced card search).
  ownedKeys?: string[]
}>()

const route = useRoute()
const router = useRouter()

const itemId = computed(() => {
  const value = route.query[props.queryKey]
  return typeof value === 'string' && value ? value : null
})
const game = computed(() => {
  const param = route.params.game
  if (typeof param === 'string' && param) return param
  // A route without a `:game` path param (the public deck page) carries the game in the
  // query instead — see CardTile's/ProductTile's click handlers.
  const q = route.query.game
  return typeof q === 'string' && q ? q : null
})
const open = computed(() => itemId.value !== null && game.value !== null)

// Every label is this one noun in a fixed frame — "Card details" / "Sealed product details",
// "Previous card" / "Previous sealed product" — so the wrappers stay honestly parallel.
const capitalizedNoun = computed(() => props.noun.charAt(0).toUpperCase() + props.noun.slice(1))

// Prev/next through the items on the page this modal opened over (issue #275). The grid the
// user clicked from registered its ordered ids in the nav store, so we just look the open item
// up to find its neighbours — no wraparound, and only within the current page (matching the
// underlying list). `index === -1` (a deep link, or an item on no on-page grid) means no nav;
// `total <= 1` (a lone item) hides it too.
const position = computed(() =>
  open.value && game.value && itemId.value
    ? props.nav.locate(game.value, itemId.value)
    : { prev: null, next: null, index: -1, total: 0 },
)
const hasNav = computed(() => position.value.index >= 0 && position.value.total > 1)

// Stepping to another item is just rewriting the id param — but with `replace`, not `push`, so
// holding an arrow key (or clicking prev/next) to flip through a page doesn't bury the list
// underneath dozens of history entries: Back still exits the modal in one press. The list keeps
// its scroll/search/page state throughout (issue #275).
function goTo(id: string | null) {
  if (!id) return
  void router.replace({ query: { ...route.query, [props.queryKey]: id } })
}

// Left/right arrow keys mirror the buttons. The listener sits on the dialog's OWN content (see
// the template `@keydown`), not window, so it only fires while this modal holds focus — arrows
// over a nested overlay stacked on top (the image-zoom lightbox, a quick-add popover), or over
// the page behind a closed modal, are left untouched. It still bows out while the user is typing
// in one of the modal's fields (the quantity inputs) or holding a modifier (a browser/OS
// shortcut — Cmd/Ctrl+Arrow is Back/Forward). reka's Dialog owns Escape and the focus trap;
// arrow keys are ours.
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

// Closing (X / ESC / overlay click) drops the params this modal owns but keeps the rest of the
// URL — list state (page, search, sort, ghosts…) is untouched underneath.
function close() {
  const next = { ...route.query }
  delete next[props.queryKey]
  for (const key of props.ownedKeys ?? []) delete next[key]
  // Drop the game a `:game`-less route (the public deck page) carried alongside the id. This is
  // unconditional because `?game=` is never anyone else's: CardTile and ProductTile are its only
  // writers and both set it only when the route has no `:game` param, so wherever it exists it
  // is this modal's to remove — and on a route that does have the path param it isn't there to
  // begin with, making the delete a no-op.
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
      v-if="game && itemId"
      class="bg-background max-h-[90svh] w-[min(96vw,64rem)] overflow-y-auto rounded-xl border p-5 shadow-lg sm:p-8"
      @keydown="onKeydown"
    >
      <DialogTitle class="sr-only">{{ capitalizedNoun }} details</DialogTitle>

      <!-- Header row, pinned above the scrolling body: prev/next through the page's items on
           the left (issue #275), the canonical detail page + a plain close on the right. The
           left group stays present (an empty flex item) even without nav so the escape hatches
           keep their right edge. -->
      <div class="mb-4 flex items-center justify-between gap-2">
        <!-- Prev/next through the items on the page underneath — arrow keys mirror these (see
             onKeydown). Hidden when the open item isn't part of a known on-page grid (a deep
             link, or a lone item), so there's never a dead control. -->
        <div class="flex items-center gap-1">
          <template v-if="hasNav">
            <button
              type="button"
              class="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex items-center justify-center rounded-md p-1.5 disabled:pointer-events-none disabled:opacity-40"
              :disabled="!position.prev"
              :aria-label="`Previous ${noun}`"
              :title="`Previous ${noun} (←)`"
              @click="goTo(position.prev)"
            >
              <ChevronLeft class="size-4" aria-hidden="true" />
            </button>
            <button
              type="button"
              class="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex items-center justify-center rounded-md p-1.5 disabled:pointer-events-none disabled:opacity-40"
              :disabled="!position.next"
              :aria-label="`Next ${noun}`"
              :title="`Next ${noun} (→)`"
              @click="goTo(position.next)"
            >
              <ChevronRight class="size-4" aria-hidden="true" />
            </button>
            <span
              class="text-muted-foreground ml-1 text-xs tabular-nums"
              aria-live="polite"
              :aria-label="`${capitalizedNoun} ${position.index + 1} of ${position.total}`"
            >
              {{ position.index + 1 }} / {{ position.total }}
            </span>
          </template>
        </div>

        <div class="flex items-center gap-1">
          <RouterLink
            :to="canonical(game, itemId)"
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

      <!-- The wrapper's detail body. Scoped so it reads the resolved game + id from the URL
           rather than re-deriving them. -->
      <slot :game="game" :id="itemId" />
    </DialogContent>
  </Dialog>
</template>
