<script setup lang="ts">
import { defineAsyncComponent, defineComponent, h, onMounted, ref, watch } from 'vue'
import { RouterLink, RouterView, useRoute, useRouter } from 'vue-router'
import AppFooter from '@/components/AppFooter.vue'
import MainNav from '@/components/MainNav.vue'
import MobileNav from '@/components/MobileNav.vue'
import NavigationProgressBar from '@/components/NavigationProgressBar.vue'
import ThemeToggle from '@/components/ThemeToggle.vue'
import UserMenu from '@/components/UserMenu.vue'
import { Skeleton } from '@/components/ui/skeleton'
import { loadCardDetailDialog } from '@/components/cards/detailDialogLoader'
import { useAuthCacheReset } from '@/composables/useAuthCacheReset'
import { scheduleIdleWarm } from '@/lib/prefetch'

// Session restore happens once in the router guard (see router/index.ts).

// Wipe the per-user query cache whenever the signed-in identity changes, so one
// account never sees another's cached collection/wish list (issue #177).
useAuthCacheReset()

const route = useRoute()
const router = useRouter()

// Instant feedback for a click that races the chunk warm: a bare fixed backdrop with a
// centered card-shaped skeleton. Self-contained (no reka) so it needs nothing from the
// not-yet-loaded dialog chunk.
const CardDetailDialogLoading = defineComponent({
  name: 'CardDetailDialogLoading',
  render: () =>
    h(
      'div',
      { class: 'fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4' },
      h(Skeleton, { class: 'aspect-[61/85] w-[min(90vw,22rem)] rounded-xl' }),
    ),
})

// The card-detail modal is a lazy chunk kept off the first paint of every page. It's
// hover-warmed by CardTile and idle-warmed below, so the click that opens ?card= usually
// finds the chunk already fetched; the loadingComponent covers a click that still races
// the fetch. Accepted trade: a COLD deep-link with ?card= (no warm yet) paints the page
// for one chunk-RTT before the overlay appears.
const CardDetailDialog = defineAsyncComponent({
  loader: loadCardDetailDialog,
  loadingComponent: CardDetailDialogLoading,
})

// The loadingComponent is a full-screen backdrop that Vue keeps rendering for as long as
// the wrapper is mounted while the chunk loads — it has no knowledge of `?card=`, since
// the open/closed logic lives inside the not-yet-loaded chunk. So mount the wrapper only
// while `?card=` is present OR the chunk has already loaded: a user who backs out before
// a cold chunk lands unmounts the backdrop with them (rather than being blocked by a
// dead overlay). Once loaded, `dialogLoaded` latches true so the loaded component stays
// mounted and owns its own open/close — reopening never re-fetches the chunk.
const dialogLoaded = ref(false)
watch(
  () => route.query.card,
  (card) => {
    if (card && !dialogLoaded.value) {
      loadCardDetailDialog()
        .then(() => {
          dialogLoaded.value = true
        })
        .catch(() => {})
    }
  },
  { immediate: true },
)

// When the browser goes idle, warm the JS chunks for the section landings and the default
// game's per-game views, plus the card-detail dialog chunk — so the first click into any
// of them is instant. Chunks only (never data/images — Scryfall guideline).
onMounted(() => {
  scheduleIdleWarm(
    router,
    [
      { name: 'cards' },
      { name: 'sealed' },
      { name: 'collection' },
      { name: 'wishlists' },
      { name: 'game', params: { game: 'mtg' } },
      { name: 'game-sealed', params: { game: 'mtg' } },
      { name: 'game-collection', params: { game: 'mtg' } },
      { name: 'game-wishlist', params: { game: 'mtg' } },
    ],
    [loadCardDetailDialog],
  )
})
</script>

<template>
  <!-- overflow-x-clip (not -hidden) contains any accidental horizontal overflow WITHOUT
       turning this root into a scroll container: `overflow-x: hidden` forces the computed
       `overflow-y` to `auto`, which silently breaks every descendant `position: sticky`
       (the /docs Scalar sidebar scrolled away with the page). `clip` leaves overflow-y
       `visible`, so the page scrolls the document and sticky works. -->
  <div class="bg-background text-foreground flex min-h-screen flex-col overflow-x-clip">
    <!-- Top-of-page navigation progress bar, shown only when a route change runs long. -->
    <NavigationProgressBar />
    <header class="border-b">
      <div class="mx-auto flex h-14 max-w-6xl items-center justify-between gap-2 px-4">
        <div class="flex min-w-0 items-center gap-1">
          <!-- Below sm the two nav dropdowns don't fit alongside the brand + theme +
               account controls, so they collapse into MobileNav's hamburger. -->
          <MobileNav class="sm:hidden" />
          <RouterLink to="/" class="truncate text-lg font-semibold tracking-tight"
            >TCGLense</RouterLink
          >
          <!-- MainNav renders its own <nav> landmark (reka NavigationMenu), so this is a div.
               Both dropdowns live under one NavigationMenu so the swipe/fade motion plays
               when moving between them. Hidden below sm in favour of MobileNav. -->
          <div class="ml-3 hidden sm:block">
            <MainNav />
          </div>
        </div>
        <div class="flex shrink-0 items-center gap-1">
          <ThemeToggle />
          <UserMenu />
        </div>
      </div>
    </header>
    <main class="flex-1">
      <RouterView />
    </main>
    <!-- Site-wide footer (data-source credits, GitHub, Terms/Privacy, WotC disclaimer). The
         flex-1 main above pins it to the viewport bottom on short pages. -->
    <AppFooter />
    <!-- The card-detail modal any browse grid opens via `?card=<id>` (see CardTile /
         CardDetailDialog) — mounted while `?card=` is present (or once its chunk has
         loaded) so it overlays whichever page is up without a stuck loading backdrop. -->
    <CardDetailDialog v-if="route.query.card || dialogLoaded" />
  </div>
</template>
