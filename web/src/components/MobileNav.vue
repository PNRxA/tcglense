<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  Bell,
  ChevronRight,
  Code,
  Heart,
  Layers,
  Library,
  Menu,
  Package,
  ScanLine,
} from '@lucide/vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { Button } from '@/components/ui/button'
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from '@/components/ui/sheet'
import { useGamesQuery } from '@/composables/useCatalog'
import { prefetchRouteChunks } from '@/lib/prefetch'

// The mobile counterpart to MainNav: the top-bar's Cards/Collection/Wish list
// dropdowns don't fit alongside the brand + theme + account controls at narrow widths,
// so below `lg` they collapse into this hamburger, which opens a left Sheet drawer
// (reka Dialog underneath). The nav sections — Cards + Sealed (the desktop "Products"
// menu, split into two labeled sections here), Collection and Wish list — are driven
// by the same cached games registry as MainNav so a new TCG shows up here
// automatically. Real <RouterLink> anchors (via as-child / rendered directly) keep the
// links keyboard- and middle-click-friendly. Unlike the old DropdownMenu, a dialog
// does NOT auto-close when a link inside it is activated, so close-on-navigate is
// hand-wired (the delegated click handler + route watcher below).
const { data } = useGamesQuery()
const games = computed(() => data.value?.data ?? [])

// One source of truth for the four sections: the template AND warmAll derive from it,
// so chunk-warming can never drift from the rendered links.
const sections = [
  { id: 'mnav-cards', label: 'Cards', icon: Layers, base: '/cards' },
  { id: 'mnav-sealed', label: 'Sealed', icon: Package, base: '/sealed' },
  { id: 'mnav-collection', label: 'Collection', icon: Library, base: '/collection' },
  { id: 'mnav-wishlist', label: 'Wish list', icon: Heart, base: '/wishlist' },
] as const

// Touch has no hover, so warm every nav destination's JS chunk when the hamburger opens
// (see lib/prefetch.ts — chunks only, never data/images). The tap-to-tap gap covers the
// fetch. Per-game links all map to one view chunk, so iterating games costs nothing.
const router = useRouter()
function warmAll(open: boolean) {
  if (!open) return
  for (const s of sections) {
    prefetchRouteChunks(router, s.base)
    for (const game of games.value) prefetchRouteChunks(router, `${s.base}/${game.id}`)
  }
  prefetchRouteChunks(router, '/scan')
  prefetchRouteChunks(router, '/decks')
  prefetchRouteChunks(router, '/alerts')
  prefetchRouteChunks(router, '/docs')
}

const open = ref(false)

// The Sheet (a dialog), unlike the old DropdownMenu, does NOT auto-close when a link
// inside it is activated. One delegated handler closes on any left-click/Enter of an
// anchor — including a tap on the already-active route, where no navigation fires and a
// route watcher alone would leave the drawer stuck open. Middle-click fires auxclick,
// not click, so open-in-new-tab correctly leaves the drawer open.
function onNavClick(e: MouseEvent) {
  if ((e.target as HTMLElement).closest('a')) open.value = false
}

// Belt-and-braces: programmatic navigations while the drawer is open.
const route = useRoute()
watch(
  () => route.fullPath,
  () => {
    open.value = false
  },
)

// Shared link treatments, hoisted so the template rows stay under the 100-col limit.
const focusRing =
  'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring ' +
  'focus-visible:ring-inset'
const sectionLinkClass =
  'flex h-12 items-center gap-3 px-4 text-base font-medium hover:bg-accent/50 ' +
  `active:bg-accent transition-colors ${focusRing}`
const gameLinkClass =
  'flex min-h-11 items-center py-2 pl-12 pr-4 text-[15px] leading-snug ' +
  `hover:bg-accent/50 active:bg-accent transition-colors ${focusRing}`
// px-3 matches the Scan button's has-[>svg]:px-3 content inset so the two rows share a
// left edge; mx-0.5 on the icon centers the 16px glyph in the button icon's 20px column.
const docsLinkClass =
  'flex min-h-11 items-center gap-3 px-3 text-sm text-muted-foreground ' +
  `hover:text-foreground ${focusRing}`
</script>

<template>
  <!-- warmAll listens on the Sheet ROOT so it fires the moment the drawer opens, before
       any tap can land — not on the trigger or content. -->
  <Sheet v-model:open="open" @update:open="warmAll">
    <SheetTrigger as-child>
      <Button variant="ghost" size="icon" aria-label="Open navigation menu">
        <Menu />
      </Button>
    </SheetTrigger>
    <!-- Full-height left drawer (matches the hamburger's corner); the 85vw cap keeps a
         visible strip of dimmed page as the tap-to-dismiss affordance. p-0/gap-0 strip
         the default padding so the nav rows can be full-bleed tap strips. -->
    <SheetContent side="left" class="flex w-80 max-w-[85vw] flex-col gap-0 p-0">
      <!-- Pinned brand header. The sr-only description silences reka's
           missing-description warning and gives screen readers context. -->
      <SheetHeader class="border-b px-4 py-3 text-left">
        <SheetTitle class="text-lg font-semibold tracking-tight">TCGLense</SheetTitle>
        <SheetDescription class="sr-only">Site navigation</SheetDescription>
      </SheetHeader>
      <!-- Only this region scrolls; header and footer stay pinned. -->
      <nav
        aria-label="Main navigation"
        class="flex-1 overflow-y-auto overscroll-contain py-2"
        @click="onNavClick"
      >
        <div
          v-for="(section, i) in sections"
          :key="section.base"
          :class="i > 0 ? 'mt-2 border-t pt-2' : undefined"
        >
          <!-- The section title IS the landing link — 48px full-bleed tap strip with a
               trailing chevron signalling navigability. vue-router stamps
               aria-current="page" on the exact-active anchor automatically. -->
          <RouterLink
            :id="section.id"
            :to="section.base"
            :class="sectionLinkClass"
            exact-active-class="bg-accent text-accent-foreground"
          >
            <component :is="section.icon" class="size-5" aria-hidden="true" />
            {{ section.label }}
            <ChevronRight class="ml-auto size-4 text-muted-foreground" aria-hidden="true" />
          </RouterLink>
          <!-- aria-labelledby points at the landing link so SRs announce e.g.
               "Cards, list, 1 item". Rows are ≥44px; long game names wrap via
               min-h + leading-snug, never truncate. -->
          <ul :aria-labelledby="section.id">
            <li v-for="game in games" :key="`${section.base}-${game.id}`">
              <RouterLink
                :to="`${section.base}/${game.id}`"
                :class="gameLinkClass"
                exact-active-class="bg-accent text-accent-foreground font-medium"
                >{{ game.name }}</RouterLink
              >
            </li>
          </ul>
        </div>
      </nav>
      <!-- Pinned thumb-zone footer: Scan cards is the app's most mobile-native feature,
           so it gets the prominent slot; safe-area padding clears the iOS home bar. -->
      <div
        class="mt-auto flex flex-col gap-2 border-t p-4 pb-[max(1rem,env(safe-area-inset-bottom))]"
        @click="onNavClick"
      >
        <Button variant="secondary" as-child class="h-12 w-full justify-start gap-3 text-base">
          <RouterLink to="/scan">
            <ScanLine class="size-5" aria-hidden="true" />
            Scan cards
          </RouterLink>
        </Button>
        <RouterLink to="/decks" :class="docsLinkClass">
          <Layers class="mx-0.5 size-4" aria-hidden="true" />
          Decks
        </RouterLink>
        <RouterLink to="/alerts" :class="docsLinkClass">
          <Bell class="mx-0.5 size-4" aria-hidden="true" />
          Price alerts
        </RouterLink>
        <RouterLink to="/docs" :class="docsLinkClass">
          <Code class="mx-0.5 size-4" aria-hidden="true" />
          API docs
        </RouterLink>
      </div>
    </SheetContent>
  </Sheet>
</template>
