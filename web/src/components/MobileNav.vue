<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ChevronRight, Menu } from '@lucide/vue'
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
import { useNav } from '@/composables/useNav'
import { groupWarmTargets } from '@/lib/nav'
import { prefetchRouteChunks } from '@/lib/prefetch'

// The mobile counterpart to MainNav: the top bar's dropdowns don't fit alongside the
// brand + theme + account controls at narrow widths, so below `lg` they collapse into
// this hamburger, which opens a left Sheet drawer (reka Dialog underneath).
//
// The drawer renders `useNav().roots` — the SAME registry (`lib/nav.ts`), in the same
// order, as the desktop nav and the footer's Product column. That is the whole point:
// this file used to hand-write its own section list, which is how Decks, the Keyword
// glossary and Tools each reached one nav and not the others. Nothing here is keyed on a
// path or a label; the only id the template knows is the sanctioned `scan` promotion
// below.
//
// It is deliberately FLAT — no accordion, no drill-in. An accordion would regress the
// four highest-frequency destinations (Cards, Sealed, Collection, Wish list) from one tap
// to two; at one game this is ~15 rows in a scrolling region, which is fine. That trade
// was decided against, not overlooked.
//
// Real <RouterLink> anchors (via as-child / rendered directly) keep the links keyboard-
// and middle-click-friendly. Unlike a DropdownMenu, a dialog does NOT auto-close when a
// link inside it is activated, so close-on-navigate is hand-wired (the delegated click
// handler + route watcher below).
const { roots } = useNav()

// The scrolling region is every group of every menu root, flattened in registry order:
// Catalog, Your library, Tools. Groups are rendered generically — a new item, group or
// menu root appears here with no edit to this file.
const zones = computed(() =>
  roots.value.flatMap((root) => (root.kind === 'menu' ? root.groups : [])),
)

// The ONE sanctioned presentation hook in the registry contract: MobileNav promotes the
// item with id `scan` into the pinned thumb-zone button and skips it in the scroll region.
// Keyed on the stable `id` — not its path, not its label — and rendered ONCE. If this ever
// looks like an omission, it isn't: the row below the fold and the button are the same
// registry entry, so don't "fix" it by adding a duplicate row.
const SCAN_ID = 'scan'
const scan = computed(() =>
  zones.value.flatMap((zone) => zone.items).find((entry) => entry.item.id === SCAN_ID),
)

// A `kind: 'link'` root (today: the API reference) has nothing to expand into, so it is
// not a zone — it renders as the footer's muted row. Found by kind rather than by path so
// the destination still comes from the registry.
// Indexed rather than `.at(0)`: the build's lib target rejects Array.prototype.at.
const bareLink = computed(
  () => roots.value.flatMap((root) => (root.kind === 'link' ? [root.item] : []))[0],
)

// Touch has no hover, so warm every nav destination's JS chunk when the hamburger opens
// (see lib/prefetch.ts — chunks only, never data/images). The tap-to-tap gap covers the
// fetch. The list is DERIVED from the same resolved tree the template renders
// (`groupWarmTargets` flattens landings + per-game links), so it cannot drift from the
// links the way the hand-written tail of prefetch calls that used to live here did — no
// path string appears in this file at all. Per-game links mostly map to one view chunk,
// so iterating games costs nothing (import() dedupes).
const router = useRouter()
function warmAll(isOpen: boolean) {
  if (!isOpen) return
  for (const root of roots.value) {
    const targets =
      root.kind === 'link' ? [root.item.landing] : root.groups.flatMap(groupWarmTargets)
    for (const to of targets) prefetchRouteChunks(router, to)
  }
}

const open = ref(false)

// The Sheet (a dialog), unlike a DropdownMenu, does NOT auto-close when a link inside it
// is activated. One delegated handler closes on any left-click/Enter of an anchor —
// including a tap on the already-active route, where no navigation fires and a route
// watcher alone would leave the drawer stuck open. Middle-click fires auxclick, not
// click, so open-in-new-tab correctly leaves the drawer open.
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
const zoneHeadingClass =
  'px-4 pb-1 pt-2 text-xs font-medium uppercase tracking-wide text-muted-foreground'
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
          v-for="(zone, i) in zones"
          :key="zone.group.id"
          :class="i > 0 ? 'mt-2 border-t pt-2' : undefined"
        >
          <!-- The zone heading is the registry group's own label (Catalog / Your library
               / Tools) — a quiet divider, never a control: the rows below it are flat. -->
          <p :class="zoneHeadingClass">{{ zone.group.label }}</p>
          <template v-for="entry in zone.items" :key="entry.item.id">
            <!-- `scan` is promoted into the pinned footer button (see SCAN_ID above), so
                 it is skipped here rather than rendered twice. -->
            <div v-if="entry.item.id !== SCAN_ID">
              <!-- The item title IS the landing link — 48px full-bleed tap strip with a
                   trailing chevron signalling navigability. vue-router stamps
                   aria-current="page" on the exact-active anchor automatically. -->
              <RouterLink
                :id="`mnav-${entry.item.id}`"
                :to="entry.item.landing"
                :class="sectionLinkClass"
                exact-active-class="bg-accent text-accent-foreground"
              >
                <component :is="entry.item.icon" class="size-5" aria-hidden="true" />
                {{ entry.item.label }}
                <ChevronRight class="ml-auto size-4 text-muted-foreground" aria-hidden="true" />
              </RouterLink>
              <!-- aria-labelledby points at the landing link so SRs announce e.g.
                   "Cards, list, 1 item". Rows are ≥44px; long game names wrap via
                   min-h + leading-snug, never truncate. A game can expand to more than one
                   row (Tools lists each tool plus its index), so the links are flattened. -->
              <ul v-if="entry.perGame.length" :aria-labelledby="`mnav-${entry.item.id}`">
                <template v-for="expansion in entry.perGame" :key="expansion.game.id">
                  <li v-for="link in expansion.links" :key="link.to">
                    <RouterLink
                      :to="link.to"
                      :class="gameLinkClass"
                      exact-active-class="bg-accent text-accent-foreground font-medium"
                      >{{ link.label }}</RouterLink
                    >
                  </li>
                </template>
              </ul>
            </div>
          </template>
        </div>
      </nav>
      <!-- Pinned thumb-zone footer: Scan cards is the app's most mobile-native feature,
           so it gets the prominent slot; safe-area padding clears the iOS home bar.
           Alerts deliberately isn't here any more — account-scoped notification settings
           live in UserMenu (App.vue renders it at every width, outside the `lg` gate), so
           nothing became unreachable. That is a decision, not a regression. -->
      <div
        class="mt-auto flex flex-col gap-2 border-t p-4 pb-[max(1rem,env(safe-area-inset-bottom))]"
        @click="onNavClick"
      >
        <Button
          v-if="scan"
          variant="secondary"
          as-child
          class="h-12 w-full justify-start gap-3 text-base"
        >
          <RouterLink :to="scan.item.landing">
            <component :is="scan.item.icon" class="size-5" aria-hidden="true" />
            {{ scan.item.label }}
          </RouterLink>
        </Button>
        <!-- Label, destination and icon all come from the registry's bare link root. This
             row used to spell out "API docs" while the top bar and the footer both said
             "API" — a third spelling of one destination is the drift this file was rewritten
             to end, so the wording is now one string in one file. -->
        <RouterLink v-if="bareLink" :to="bareLink.landing" :class="docsLinkClass">
          <component :is="bareLink.icon" class="mx-0.5 size-4" aria-hidden="true" />
          {{ bareLink.label }}
        </RouterLink>
      </div>
    </SheetContent>
  </Sheet>
</template>
