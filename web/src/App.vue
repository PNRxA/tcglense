<script setup lang="ts">
import { defineAsyncComponent, defineComponent, h, onMounted, ref, watch } from 'vue'
import { RouterLink, RouterView, useRoute, useRouter } from 'vue-router'
import AppFooter from '@/components/AppFooter.vue'
import MaintenanceMode from '@/components/MaintenanceMode.vue'
import MainNav from '@/components/MainNav.vue'
import MobileNav from '@/components/MobileNav.vue'
import NavigationProgressBar from '@/components/NavigationProgressBar.vue'
import ThemeToggle from '@/components/ThemeToggle.vue'
import CurrencyMenu from '@/components/CurrencyMenu.vue'
import UserMenu from '@/components/UserMenu.vue'
import { Skeleton } from '@/components/ui/skeleton'
import { loadCardDetailDialog } from '@/components/cards/detailDialogLoader'
import { loadProductDetailDialog } from '@/components/products/detailDialogLoader'
import { useAuthCacheReset } from '@/composables/useAuthCacheReset'
import { useMaintenanceMode } from '@/composables/useMaintenanceMode'
import { scheduleIdleWarm } from '@/lib/prefetch'

// Session restore happens once in the router guard (see router/index.ts).

// Wipe the per-user query cache whenever the signed-in identity changes, so one
// account never sees another's cached collection/wish list (issue #177).
useAuthCacheReset()
const maintenanceMode = useMaintenanceMode()

const route = useRoute()
const router = useRouter()

// Instant feedback for a click that races a chunk warm: a bare fixed backdrop with a
// card- or product-shaped skeleton. Self-contained (no reka) so it needs nothing from the
// not-yet-loaded dialog chunk.
function makeDetailDialogLoading(name: string, shapeClass: string) {
  return defineComponent({
    name,
    render: () =>
      h(
        'div',
        { class: 'fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4' },
        h(Skeleton, { class: `${shapeClass} w-[min(90vw,22rem)] rounded-xl` }),
      ),
  })
}

const CardDetailDialogLoading = makeDetailDialogLoading('CardDetailDialogLoading', 'aspect-[61/85]')
const ProductDetailDialogLoading = makeDetailDialogLoading(
  'ProductDetailDialogLoading',
  'aspect-square',
)

// Both detail modals are lazy chunks kept off the first paint of every page. Their tiles
// hover-warm them and App idle-warms them below; the loading components cover a cold click.
const CardDetailDialog = defineAsyncComponent({
  loader: loadCardDetailDialog,
  loadingComponent: CardDetailDialogLoading,
})
const ProductDetailDialog = defineAsyncComponent({
  loader: loadProductDetailDialog,
  loadingComponent: ProductDetailDialogLoading,
})

// The loadingComponent is a full-screen backdrop that Vue keeps rendering for as long as
// the wrapper is mounted while the chunk loads — it has no knowledge of its query key,
// since open/closed logic lives inside the not-yet-loaded chunk. Mount each wrapper only
// while its key is present OR its chunk has loaded: backing out before a cold chunk lands
// removes the backdrop, and reopening an already-loaded dialog never re-fetches its chunk.
function latchDialog(queryKey: 'card' | 'product', loader: () => Promise<unknown>) {
  const loaded = ref(false)
  watch(
    () => route.query[queryKey],
    (id) => {
      if (id && !loaded.value) {
        loader()
          .then(() => {
            loaded.value = true
          })
          .catch(() => {})
      }
    },
    { immediate: true },
  )
  return loaded
}

const cardDialogLoaded = latchDialog('card', loadCardDetailDialog)
const productDialogLoaded = latchDialog('product', loadProductDetailDialog)

// When the browser goes idle, warm the JS chunks for the section landings and the default
// game's per-game views, plus both detail-dialog chunks — so the first click into any
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
    [loadCardDetailDialog, loadProductDetailDialog],
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
    <MaintenanceMode v-if="maintenanceMode" />
    <template v-else>
      <!-- Top-of-page navigation progress bar, shown only when a route change runs long. -->
      <NavigationProgressBar />
      <header class="border-b">
        <div class="mx-auto flex h-14 max-w-6xl items-center justify-between gap-2 px-4">
          <div class="flex min-w-0 items-center gap-1">
            <!-- Below lg the nav items don't fit alongside the brand and display/account
               controls, so they collapse into MobileNav's hamburger. The brand
               itself is fixed-width and never truncates. The lg:hidden lives on this
               wrapper, not on <MobileNav>: reka's Sheet (Dialog) root is renderless, so a
               class set on the component is dropped and the hamburger would never hide —
               it would double up with MainNav from lg up. -->
            <div class="lg:hidden">
              <MobileNav />
            </div>
            <RouterLink
              to="/"
              class="shrink-0 whitespace-nowrap text-lg font-semibold tracking-tight"
              >TCGLense</RouterLink
            >
            <!-- MainNav renders its own <nav> landmark (reka NavigationMenu), so this is a div.
               Both dropdowns live under one NavigationMenu so the swipe/fade motion plays
               when moving between them. Hidden below lg in favour of MobileNav. -->
            <div class="ml-3 hidden lg:block">
              <MainNav />
            </div>
          </div>
          <div class="flex shrink-0 items-center gap-1">
            <ThemeToggle />
            <CurrencyMenu />
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
      <!-- URL-driven detail modals for card and sealed-product browse grids. Each remains
         mounted after its first load so reopening is instant; ProductTile/CardTile remove
         the opposite key when transitioning between the two surfaces. -->
      <CardDetailDialog v-if="route.query.card || cardDialogLoaded" />
      <ProductDetailDialog v-if="route.query.product || productDialogLoaded" />
    </template>
  </div>
</template>
