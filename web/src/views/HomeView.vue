<script setup lang="ts">
import { computed, type Component } from 'vue'
import {
  ArrowRight,
  Bot,
  ChevronRight,
  CircleCheck,
  Code,
  ExternalLink,
  Ghost,
  Heart,
  Import,
  Layers,
  LayoutGrid,
  Library,
  LibraryBig,
  Lock,
  Package,
  PackageOpen,
  Search,
  Terminal,
  TrendingUp,
} from '@lucide/vue'
import { RouterLink } from 'vue-router'
import GitHubMark from '@/components/GitHubMark.vue'
import DemoCardTile from '@/components/home/DemoCardTile.vue'
import FeatureDemoRow from '@/components/home/FeatureDemoRow.vue'
import { buttonVariants } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { useGamesQuery } from '@/composables/useCatalog'
import { usePageMeta } from '@/lib/seo'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()

usePageMeta({
  description:
    'Browse trading-card games, sets, cards, and sealed products, chart daily prices, and ' +
    'track your collection and wish list — with ghost mode showing exactly which cards you ' +
    'are missing.',
  canonicalPath: '/',
})

// Optional, resilient to an empty/loading list: lets a visitor jump straight into a real
// game's public catalog from the homepage.
const gamesQuery = useGamesQuery()
const games = computed(() => gamesQuery.data.value?.data ?? [])

interface FeatureLink {
  icon: Component
  title: string
  description: string
  to: string
}

// The compact "everything else" grid — only shipped features, each linking to where it
// lives. The sixth card (Open source, external) is rendered on its own in the template.
const otherFeatures: FeatureLink[] = [
  {
    icon: Search,
    title: 'Scryfall-style search',
    description:
      'Full search syntax on every card list — colors, types, oracle text, prices, even regex.',
    to: '/cards',
  },
  {
    icon: Layers,
    title: 'Secret Lair by-drop views',
    description:
      'Drop-grouped sets break into their real drops, in the catalog, your collection, and ' +
      'your wish list.',
    to: '/cards',
  },
  {
    icon: PackageOpen,
    title: 'Cards to sealed',
    description:
      "Every card page lists the sealed products it's found in, can be pulled from, or may be in.",
    to: '/cards',
  },
  {
    icon: LayoutGrid,
    title: 'Set-by-set browsing',
    description: 'Browse what you own set by set, with per-set counts and value.',
    to: '/collection',
  },
  {
    icon: Lock,
    title: 'Free accounts',
    description:
      'Register with just an email address — free to track your collection and wish list.',
    to: '/register',
  },
  {
    icon: Code,
    title: 'Public API',
    description:
      'A documented public API for the catalog, plus scoped API keys for your collection and ' +
      'wish list. Interactive reference included.',
    to: '/docs',
  },
]

// Text-link CTA style under each feature demo row (buttons stay reserved for the hero and
// the closing band).
const rowLinkClass =
  'text-primary inline-flex items-center gap-1 text-sm font-medium underline-offset-4 ' +
  'hover:underline'
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 pt-14 pb-20 sm:pt-20">
    <!-- Hero: value prop + auth-branched CTAs, beside a decorative "show the product" vignette. -->
    <section class="grid items-center gap-10 lg:grid-cols-[1fr_minmax(0,30rem)] lg:gap-14">
      <div>
        <span
          class="border-border bg-muted text-muted-foreground inline-flex items-center gap-1.5 rounded-full border px-3 py-1 text-xs font-medium"
        >
          <Package class="size-3.5" aria-hidden="true" />
          New — sealed products, priced daily
        </span>
        <h1 class="mt-6 text-4xl font-semibold tracking-tight text-balance sm:text-5xl">
          Your collection, priced every day.
        </h1>
        <p class="text-muted-foreground mt-4 max-w-xl text-base text-pretty sm:text-lg">
          TCGLense is a free, open-source tracker for trading-card games: browse every set, chart
          singles and sealed prices day by day, and see exactly which cards you own — and which ones
          you're still missing.
        </p>

        <!-- Auth-branched CTAs: authed variant on a token, guest variant once resolved
             signed-out, and while the session is still unresolved reserve the two lg
             buttons' footprint with skeletons so the row doesn't jump on resolve. -->
        <div class="mt-8 flex flex-col gap-3 sm:flex-row">
          <template v-if="auth.isAuthenticated">
            <RouterLink to="/collection" :class="buttonVariants({ size: 'lg' })">
              Open your collection
              <ArrowRight aria-hidden="true" />
            </RouterLink>
            <RouterLink to="/cards" :class="buttonVariants({ variant: 'outline', size: 'lg' })">
              Browse cards
            </RouterLink>
          </template>
          <template v-else-if="auth.sessionResolved">
            <RouterLink to="/register" :class="buttonVariants({ size: 'lg' })">
              Create a free account
              <ArrowRight aria-hidden="true" />
            </RouterLink>
            <RouterLink to="/cards" :class="buttonVariants({ variant: 'outline', size: 'lg' })">
              Browse the catalog
            </RouterLink>
          </template>
          <template v-else>
            <Skeleton class="h-10 w-52" />
            <Skeleton class="h-10 w-40" />
          </template>
        </div>

        <p
          v-if="auth.sessionResolved && !auth.isAuthenticated"
          class="text-muted-foreground mt-4 text-sm text-pretty"
        >
          No account needed to browse cards, sets, sealed products, and prices. Already have an
          account?
          <RouterLink to="/login" class="text-primary underline-offset-4 hover:underline">
            Sign in
          </RouterLink>
        </p>
      </div>

      <!-- Decorative vignette: a mini set grid with owned badges + ghosts, a completion chip,
           and a price sparkline. Illustrative only, hidden from assistive tech. -->
      <div class="relative mx-auto w-full max-w-md sm:pb-6 lg:max-w-none" aria-hidden="true">
        <!-- Decorative mock UI — illustrative values, not real market data. -->
        <div
          class="bg-background absolute -top-3 right-6 inline-flex items-center gap-1.5 rounded-full border px-3 py-1 text-xs font-medium shadow-sm"
        >
          <CircleCheck class="text-primary size-3.5" />
          4 / 6 owned
        </div>

        <div class="bg-card rounded-2xl border p-4 shadow-sm sm:p-5">
          <div class="grid grid-cols-3 gap-3">
            <!-- Tile 1: owned (badge: 3 + 1 foil). -->
            <DemoCardTile :layers="3" :foil="1" />
            <!-- Tile 2. -->
            <DemoCardTile gradient="muted" />
            <!-- Tile 3. -->
            <DemoCardTile gradient="muted" />
            <!-- Tile 4. -->
            <DemoCardTile />
            <!-- Tile 5: ghost (missing). -->
            <DemoCardTile gradient="muted" ghost />
            <!-- Tile 6: ghost with a crisp quick-add chip (not dimmed). -->
            <DemoCardTile ghost quick-add />
          </div>
        </div>

        <div
          class="bg-card mt-4 w-48 rounded-xl border p-3 shadow-sm sm:absolute sm:-bottom-6 sm:-left-4 sm:mt-0"
        >
          <div class="flex items-baseline justify-between">
            <span class="text-muted-foreground text-xs font-medium">Price history</span>
            <span class="text-xs font-semibold tabular-nums">$18.40</span>
          </div>
          <svg
            viewBox="0 0 128 40"
            class="mt-1.5 h-10 w-full"
            fill="none"
            preserveAspectRatio="none"
          >
            <polyline
              points="0,30 16,28 32,31 48,24 64,26 80,18 96,20 112,12 128,14"
              stroke="var(--chart-1)"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
            />
            <polyline
              points="0,22 16,24 32,20 48,18 64,21 80,12 96,15 112,8 128,10"
              class="opacity-80"
              stroke="var(--chart-2)"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
            />
          </svg>
          <div class="text-muted-foreground mt-1.5 flex items-center gap-3 text-[10px]">
            <span class="flex items-center gap-1">
              <span class="size-1.5 rounded-full" style="background: var(--chart-1)"></span>
              USD
            </span>
            <span class="flex items-center gap-1">
              <span class="size-1.5 rounded-full" style="background: var(--chart-2)"></span>
              USD foil
            </span>
          </div>
        </div>
      </div>
    </section>

    <!-- Feature demo rows: each pairs a text column with a decorative mock panel, alternating
         sides at md+. -->
    <section class="mt-20 sm:mt-24">
      <div class="space-y-20 sm:space-y-24">
        <!-- Row A — Prices (demo right). -->
        <FeatureDemoRow
          :icon="TrendingUp"
          eyebrow="Price history"
          heading="Every price, charted every day"
          :body="
            'TCGLense captures singles prices daily — USD, EUR, and foil — and charts USD and foil ' +
            'on every card, windowed from the last 7 days to the full history. Sealed products get ' +
            'the same treatment: current prices and price history for booster boxes, bundles, and ' +
            'decks.'
          "
          demo-side="right"
        >
          <RouterLink to="/cards" :class="rowLinkClass">
            Browse cards
            <ArrowRight class="size-4" aria-hidden="true" />
          </RouterLink>
          <RouterLink to="/sealed" :class="rowLinkClass">
            Browse sealed products
            <ArrowRight class="size-4" aria-hidden="true" />
          </RouterLink>
          <template #demo>
            <!-- Decorative mock UI — illustrative values, not real market data. -->
            <div class="flex items-center justify-between">
              <span class="text-sm font-semibold">Price history</span>
              <div class="bg-muted/50 inline-flex items-center gap-1 rounded-lg p-0.5">
                <span class="text-muted-foreground rounded px-2 py-1 text-xs font-medium">7D</span>
                <span
                  class="bg-background text-foreground rounded px-2 py-1 text-xs font-medium shadow-sm"
                  >30D</span
                >
                <span class="text-muted-foreground rounded px-2 py-1 text-xs font-medium">1Y</span>
                <span class="text-muted-foreground rounded px-2 py-1 text-xs font-medium">3Y</span>
                <span class="text-muted-foreground rounded px-2 py-1 text-xs font-medium">All</span>
              </div>
            </div>
            <svg
              viewBox="0 0 320 96"
              class="mt-4 h-28 w-full"
              fill="none"
              preserveAspectRatio="none"
            >
              <line x1="0" y1="24" x2="320" y2="24" stroke="var(--border)" stroke-width="1" />
              <line x1="0" y1="48" x2="320" y2="48" stroke="var(--border)" stroke-width="1" />
              <line x1="0" y1="72" x2="320" y2="72" stroke="var(--border)" stroke-width="1" />
              <polyline
                points="0,74 40,70 80,76 120,58 160,62 200,44 240,48 280,30 320,34"
                stroke="var(--chart-1)"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
              <polyline
                points="0,54 40,58 80,50 120,46 160,52 200,30 240,36 280,20 320,24"
                class="opacity-80"
                stroke="var(--chart-2)"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
            <div class="text-muted-foreground mt-2 flex justify-between text-[10px]">
              <span>Jun 5</span>
              <span>Jun 19</span>
              <span>Jul 3</span>
            </div>
          </template>
        </FeatureDemoRow>

        <!-- Row B — Collection (demo left at md+). -->
        <FeatureDemoRow
          :icon="Library"
          eyebrow="Collection"
          heading="Know exactly what you own"
          :body="
            'Count regular and foil copies per game and watch the totals move — unique cards, ' +
            'total copies, and a live estimated value. As you browse the catalog, owned-count ' +
            'badges mark the cards already in your collection, and quick-add drops a card in by ' +
            'name without leaving the page.'
          "
          demo-side="left"
        >
          <RouterLink to="/collection" :class="rowLinkClass">
            <template v-if="auth.isAuthenticated">Open your collection</template>
            <template v-else-if="auth.sessionResolved">Start a collection</template>
            <Skeleton v-else class="h-4 w-32" />
            <ArrowRight class="size-4" aria-hidden="true" />
          </RouterLink>
          <template #demo>
            <!-- Decorative mock UI — illustrative values, not real market data. -->
            <dl class="flex flex-wrap gap-x-8 gap-y-3">
              <div>
                <dt class="text-muted-foreground text-xs tracking-wide uppercase">Unique cards</dt>
                <dd class="text-xl font-semibold tabular-nums">1,204</dd>
              </div>
              <div>
                <dt class="text-muted-foreground text-xs tracking-wide uppercase">Total copies</dt>
                <dd class="text-xl font-semibold tabular-nums">3,418</dd>
              </div>
              <div>
                <dt class="text-muted-foreground text-xs tracking-wide uppercase">Total value</dt>
                <dd class="text-xl font-semibold tabular-nums">$2,148.32</dd>
              </div>
            </dl>
            <div class="mt-5 grid grid-cols-3 gap-3">
              <!-- Tile 1: 4 owned. -->
              <DemoCardTile :layers="4" />
              <!-- Tile 2: 2 + 1 foil. -->
              <DemoCardTile :layers="2" :foil="1" />
              <!-- Tile 3: unbadged. -->
              <DemoCardTile />
            </div>
          </template>
        </FeatureDemoRow>

        <!-- Row C — Ghost mode (demo right). -->
        <FeatureDemoRow
          :icon="Ghost"
          eyebrow="Ghost mode"
          heading="See the gaps in every set"
          :body="
            'Flip on \u0022Show ghosts\u0022 in any set to dim the cards you\'re missing, with a ' +
            'live \u0022X of Y owned\u0022 count. The gaps read at a glance — and every ghost ' +
            'carries a quick-add button right where it sits. It works across your collection and ' +
            'your wish list, including Secret Lair by-drop views.'
          "
          demo-side="right"
        >
          <template v-if="auth.isAuthenticated">
            <RouterLink to="/collection" :class="rowLinkClass">
              See your set gaps
              <ArrowRight class="size-4" aria-hidden="true" />
            </RouterLink>
          </template>
          <template v-else-if="auth.sessionResolved">
            <RouterLink to="/register" :class="rowLinkClass">
              Create a free account to try it
              <ArrowRight class="size-4" aria-hidden="true" />
            </RouterLink>
          </template>
          <Skeleton v-else class="h-5 w-48" />
          <template #demo>
            <!-- Decorative mock UI — illustrative values, not real market data. -->
            <div class="flex items-center justify-between">
              <div
                class="bg-background inline-flex items-center gap-1.5 rounded-full border px-3 py-1 text-xs font-medium shadow-sm"
              >
                <CircleCheck class="text-primary size-3.5" aria-hidden="true" />
                5 / 8 owned
              </div>
              <div class="text-muted-foreground inline-flex items-center gap-2 text-xs font-medium">
                Show ghosts
                <span class="bg-primary inline-flex h-4 w-7 items-center rounded-full">
                  <span class="bg-primary-foreground size-3 translate-x-3.5 rounded-full"></span>
                </span>
              </div>
            </div>
            <div class="mt-4 grid grid-cols-4 gap-2.5">
              <!-- 5 owned. -->
              <DemoCardTile :bars="false" />
              <DemoCardTile :bars="false" />
              <DemoCardTile :bars="false" />
              <DemoCardTile :bars="false" />
              <DemoCardTile :bars="false" />
              <!-- Ghost with a crisp quick-add chip. -->
              <DemoCardTile :bars="false" ghost quick-add />
              <!-- 2 more ghosts. -->
              <DemoCardTile :bars="false" ghost />
              <DemoCardTile :bars="false" ghost />
            </div>
          </template>
        </FeatureDemoRow>

        <!-- Row D — Import & sync (demo left at md+). -->
        <FeatureDemoRow
          :icon="Import"
          eyebrow="Import & sync"
          heading="Bring your collection with you"
          :body="
            'Import from Archidekt by link and pick how it reconciles — overwrite matched cards, ' +
            'mirror-replace, add-merge, or a smart incremental sync — then save the link and ' +
            're-sync on demand. Prefer a file? Upload a CSV export from Archidekt or Moxfield and ' +
            'it reconciles on the spot.'
          "
          demo-side="left"
        >
          <RouterLink to="/collection" :class="rowLinkClass">
            Import into your collection
            <ArrowRight class="size-4" aria-hidden="true" />
          </RouterLink>
          <template #demo>
            <!-- Decorative mock UI — illustrative values, not real market data. -->
            <div
              class="bg-muted text-muted-foreground inline-flex rounded-md p-0.5 text-xs font-medium"
            >
              <span class="bg-background text-foreground rounded px-2.5 py-1 shadow-sm">
                Paste a link
              </span>
              <span class="px-2.5 py-1">Upload a CSV</span>
            </div>
            <div
              class="text-muted-foreground bg-background mt-3 flex h-8 items-center truncate rounded-md border px-2.5 text-xs"
            >
              archidekt.com/collection/…
            </div>
            <div class="mt-3 flex flex-wrap gap-1.5">
              <span class="text-muted-foreground rounded-full border px-2.5 py-0.5 text-xs">
                Overwrite
              </span>
              <span class="text-muted-foreground rounded-full border px-2.5 py-0.5 text-xs">
                Replace
              </span>
              <span class="text-muted-foreground rounded-full border px-2.5 py-0.5 text-xs">
                Merge
              </span>
              <span
                class="border-primary/30 bg-primary/10 text-primary rounded-full border px-2.5 py-0.5 text-xs font-medium"
              >
                Smart
              </span>
            </div>
            <div class="text-muted-foreground mt-4 flex items-center gap-1.5 text-xs">
              <CircleCheck class="text-primary size-3.5" aria-hidden="true" />
              Matched 1,204 cards · 96 foil
            </div>
          </template>
        </FeatureDemoRow>

        <!-- Row E — Wish list (demo right). -->
        <FeatureDemoRow
          :icon="Heart"
          eyebrow="Wish lists"
          heading="Price the cards you want next"
          :body="
            'A wish list works just like your collection — regular and foil counts, per-set ' +
            'views, ghosts across whole sets — but for the cards you\'re still hunting. It keeps ' +
            'a running USD total, so you always know what buying the list would cost.'
          "
          demo-side="right"
        >
          <RouterLink to="/wishlist" :class="rowLinkClass">
            <template v-if="auth.isAuthenticated">Open your wish list</template>
            <template v-else-if="auth.sessionResolved">Start a wish list</template>
            <Skeleton v-else class="h-4 w-32" />
            <ArrowRight class="size-4" aria-hidden="true" />
          </RouterLink>
          <template #demo>
            <!-- Decorative mock UI — illustrative values, not real market data. -->
            <span class="text-muted-foreground text-xs font-medium tracking-wide uppercase">
              Your wish list
            </span>
            <div class="mt-3 space-y-2.5">
              <div class="flex items-center gap-3">
                <div class="bg-muted h-10 w-7 shrink-0 overflow-hidden rounded border">
                  <div
                    class="from-primary/25 via-primary/10 h-2/5 bg-gradient-to-br to-transparent"
                  ></div>
                </div>
                <div class="bg-foreground/15 h-1.5 w-full max-w-32 rounded-full"></div>
                <span class="text-muted-foreground ml-auto text-xs tabular-nums">$4.10</span>
              </div>
              <div class="flex items-center gap-3">
                <div class="bg-muted h-10 w-7 shrink-0 overflow-hidden rounded border">
                  <div
                    class="from-foreground/10 h-2/5 bg-gradient-to-br via-transparent to-muted"
                  ></div>
                </div>
                <div class="bg-foreground/15 h-1.5 w-full max-w-24 rounded-full"></div>
                <span class="text-muted-foreground ml-auto text-xs tabular-nums">$12.25</span>
              </div>
              <div class="flex items-center gap-3">
                <div class="bg-muted h-10 w-7 shrink-0 overflow-hidden rounded border">
                  <div
                    class="from-primary/25 via-primary/10 h-2/5 bg-gradient-to-br to-transparent"
                  ></div>
                </div>
                <div class="bg-foreground/15 h-1.5 w-full max-w-28 rounded-full"></div>
                <span class="text-muted-foreground ml-auto text-xs tabular-nums">$69.85</span>
              </div>
            </div>
            <div class="mt-4 flex items-center justify-between border-t pt-3">
              <span class="text-sm font-medium">List total</span>
              <span class="text-sm font-semibold tabular-nums">$86.20</span>
            </div>
          </template>
        </FeatureDemoRow>

        <!-- Row F — CLI & agents (demo left at md+). -->
        <FeatureDemoRow
          :icon="Terminal"
          eyebrow="CLI & agents"
          heading="Drive it from your terminal — or an agent"
          :body="
            'tcglense is a standalone, open-source command-line client and TUI for this API. Sign ' +
            'in from the terminal with a quick browser handshake — no password typed at the prompt ' +
            '— then search the catalog, check prices, and update your collection and wish list ' +
            'without leaving the shell. Scriptable output and scoped tcgl_ API keys make it a clean ' +
            'surface for automation and AI agents to work your collection on your behalf.'
          "
          demo-side="left"
        >
          <a
            href="https://github.com/PNRxA/tcglense-cli"
            target="_blank"
            rel="noopener noreferrer"
            :class="rowLinkClass"
          >
            Get the CLI
            <ExternalLink class="size-4" aria-hidden="true" />
          </a>
          <RouterLink to="/docs" :class="rowLinkClass">
            Explore the API
            <ArrowRight class="size-4" aria-hidden="true" />
          </RouterLink>
          <template #demo>
            <!-- Decorative mock terminal — illustrative commands, not live output. -->
            <div class="overflow-hidden rounded-xl border">
              <div class="bg-muted/60 flex items-center gap-1.5 border-b px-3 py-2">
                <span class="bg-foreground/20 size-2.5 rounded-full"></span>
                <span class="bg-foreground/20 size-2.5 rounded-full"></span>
                <span class="bg-foreground/20 size-2.5 rounded-full"></span>
                <span class="text-muted-foreground ml-2 text-xs font-medium">tcglense</span>
                <span
                  class="border-primary/30 bg-primary/10 text-primary ml-auto inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-[10px] font-medium"
                >
                  <Bot class="size-3" aria-hidden="true" />
                  Agent-ready
                </span>
              </div>
              <div class="bg-card p-4 font-mono text-xs leading-relaxed">
                <p><span class="text-primary">$</span> tcglense login</p>
                <p class="text-muted-foreground">→ browser authorized “workstation”</p>
                <p class="mt-2">
                  <span class="text-primary">$</span> tcglense cards mtg -q 't:dragon usd&lt;5'
                </p>
                <p class="text-muted-foreground">
                  &nbsp;&nbsp;42 cards · Ancient Brass Dragon $3.80 …
                </p>
                <p class="mt-2">
                  <span class="text-primary">$</span> tcglense collection mtg summary --json
                </p>
                <p class="text-muted-foreground">
                  &nbsp;&nbsp;{ "unique": 1204, "copies": 3418, "value_usd": 2148.32 }
                </p>
              </div>
            </div>
          </template>
        </FeatureDemoRow>
      </div>
    </section>

    <!-- Everything else: a compact grid of shipped features, each a link to where it lives. -->
    <section class="mt-20 sm:mt-24">
      <h2 class="text-xl font-semibold tracking-tight">Everything else that's live today</h2>
      <p class="text-muted-foreground mt-1 text-sm">
        No roadmap padding — every line here is a shipped feature.
      </p>
      <div class="mt-6 grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
        <RouterLink
          v-for="feature in otherFeatures"
          :key="feature.title"
          :to="feature.to"
          class="bg-card hover:border-ring/60 hover:bg-accent/40 group flex items-center gap-4 rounded-xl border p-5 transition-colors"
        >
          <span class="bg-muted flex size-12 shrink-0 items-center justify-center rounded-lg">
            <component :is="feature.icon" class="text-primary size-6" aria-hidden="true" />
          </span>
          <span class="min-w-0">
            <span class="text-foreground block font-medium">{{ feature.title }}</span>
            <span class="text-muted-foreground mt-0.5 block text-sm text-pretty">
              {{ feature.description }}
            </span>
          </span>
          <ChevronRight
            class="text-muted-foreground ml-auto size-5 shrink-0 transition-transform group-hover:translate-x-0.5"
            aria-hidden="true"
          />
        </RouterLink>
        <a
          href="https://github.com/PNRxA/tcglense"
          target="_blank"
          rel="noopener noreferrer"
          class="bg-card hover:border-ring/60 hover:bg-accent/40 group flex items-center gap-4 rounded-xl border p-5 transition-colors"
        >
          <span class="bg-muted flex size-12 shrink-0 items-center justify-center rounded-lg">
            <GitHubMark class="text-primary size-6" />
          </span>
          <span class="min-w-0">
            <span class="text-foreground block font-medium">Open source</span>
            <span class="text-muted-foreground mt-0.5 block text-sm text-pretty">
              The whole app — the API and this site — is public on GitHub.
            </span>
          </span>
          <ExternalLink
            class="text-muted-foreground ml-auto size-5 shrink-0 transition-transform group-hover:translate-x-0.5"
            aria-hidden="true"
          />
        </a>
      </div>
    </section>

    <!-- Built on open data: prominent credits for the three open data projects behind TCGLense. -->
    <section class="mt-20 sm:mt-24">
      <h2 class="text-xl font-semibold tracking-tight">Built on open data</h2>
      <p class="text-muted-foreground mt-1 text-sm text-pretty">
        Every price, card, and box on TCGLense traces back to three open data projects.
      </p>
      <div class="mt-6 grid gap-3 sm:grid-cols-3">
        <a
          href="https://scryfall.com"
          target="_blank"
          rel="noopener noreferrer"
          class="bg-card hover:border-ring/60 hover:bg-accent/40 group block rounded-xl border p-5 transition-colors"
        >
          <div class="flex items-center justify-between gap-2">
            <span class="font-semibold">Scryfall</span>
            <ExternalLink
              class="text-muted-foreground group-hover:text-foreground size-4 shrink-0 transition-colors"
              aria-hidden="true"
            />
          </div>
          <p class="text-primary mt-0.5 text-xs font-medium tracking-wide uppercase">
            Card data &amp; images
          </p>
          <p class="text-muted-foreground mt-2 text-sm text-pretty">
            The entire card catalog — sets, cards, and the daily singles prices behind every chart —
            is built from Scryfall's bulk data. Card images are served courtesy of Scryfall.
          </p>
          <p class="text-muted-foreground mt-3 text-xs">scryfall.com</p>
        </a>
        <a
          href="https://tcgcsv.com"
          target="_blank"
          rel="noopener noreferrer"
          class="bg-card hover:border-ring/60 hover:bg-accent/40 group block rounded-xl border p-5 transition-colors"
        >
          <div class="flex items-center justify-between gap-2">
            <span class="font-semibold">TCGCSV</span>
            <ExternalLink
              class="text-muted-foreground group-hover:text-foreground size-4 shrink-0 transition-colors"
              aria-hidden="true"
            />
          </div>
          <p class="text-primary mt-0.5 text-xs font-medium tracking-wide uppercase">
            Sealed product pricing
          </p>
          <p class="text-muted-foreground mt-2 text-sm text-pretty">
            Current sealed prices and the daily history behind every booster box, bundle, and deck
            chart.
          </p>
          <p class="text-muted-foreground mt-3 text-xs">tcgcsv.com</p>
        </a>
        <a
          href="https://mtgjson.com"
          target="_blank"
          rel="noopener noreferrer"
          class="bg-card hover:border-ring/60 hover:bg-accent/40 group block rounded-xl border p-5 transition-colors"
        >
          <div class="flex items-center justify-between gap-2">
            <span class="font-semibold">MTGJSON</span>
            <ExternalLink
              class="text-muted-foreground group-hover:text-foreground size-4 shrink-0 transition-colors"
              aria-hidden="true"
            />
          </div>
          <p class="text-primary mt-0.5 text-xs font-medium tracking-wide uppercase">
            Sealed product contents
          </p>
          <p class="text-muted-foreground mt-2 text-sm text-pretty">
            Which cards live inside which sealed products — the data behind 'found in' on every card
            page.
          </p>
          <p class="text-muted-foreground mt-3 text-xs">mtgjson.com</p>
        </a>
      </div>
      <p class="text-muted-foreground mt-3 text-xs text-pretty">
        All three are independent projects. None of them produces, endorses, or is affiliated with
        TCGLense.
      </p>

      <!-- Open-source strip (#195): the GitHub call-out. -->
      <div class="border-primary/30 bg-primary/5 mt-6 rounded-2xl border p-6 sm:p-8">
        <div class="flex flex-col gap-5 sm:flex-row sm:items-center sm:justify-between">
          <div class="flex items-start gap-4">
            <GitHubMark class="text-foreground mt-0.5 size-8 shrink-0" />
            <div>
              <h3 class="text-lg font-semibold tracking-tight">TCGLense is open source</h3>
              <p class="text-muted-foreground mt-1 text-sm text-pretty">
                The whole project — the Rust API and this web app — is public on GitHub. Browse the
                code, open an issue, or send a PR.
              </p>
            </div>
          </div>
          <div class="flex shrink-0 flex-wrap gap-3">
            <a
              href="https://github.com/PNRxA/tcglense"
              target="_blank"
              rel="noopener noreferrer"
              :class="buttonVariants({ variant: 'outline' })"
            >
              View on GitHub
              <ExternalLink aria-hidden="true" />
            </a>
            <a
              href="https://github.com/PNRxA/tcglense/issues"
              target="_blank"
              rel="noopener noreferrer"
              class="text-primary inline-flex items-center self-center text-sm font-medium underline-offset-4 hover:underline"
            >
              Report an issue
            </a>
          </div>
        </div>
      </div>
    </section>

    <!-- Games strip: jump straight into a real game's catalog or sealed products (public). -->
    <section v-if="games.length" class="mt-20 sm:mt-24">
      <h2 class="text-xl font-semibold tracking-tight">Start with your game</h2>
      <p class="text-muted-foreground mt-1 text-sm">
        Browse the full catalog and sealed products — no account needed.
      </p>
      <div class="mt-4 flex flex-wrap gap-2">
        <RouterLink
          v-for="game in games"
          :key="game.id"
          :to="`/cards/${game.id}`"
          class="bg-card hover:border-ring/60 hover:bg-accent/40 inline-flex items-center gap-2 rounded-full border px-4 py-2 text-sm font-medium transition-colors"
        >
          <LibraryBig class="text-muted-foreground size-4" aria-hidden="true" />
          {{ game.name }}
          <ChevronRight class="text-muted-foreground size-4" aria-hidden="true" />
        </RouterLink>
        <RouterLink
          to="/sealed"
          class="bg-card hover:border-ring/60 hover:bg-accent/40 inline-flex items-center gap-2 rounded-full border px-4 py-2 text-sm font-medium transition-colors"
        >
          <Package class="text-muted-foreground size-4" aria-hidden="true" />
          Sealed products
          <ChevronRight class="text-muted-foreground size-4" aria-hidden="true" />
        </RouterLink>
      </div>
      <p class="text-muted-foreground mt-3 text-sm text-pretty">
        Magic: The Gathering is the first game on TCGLense — the catalog is built game-agnostic, so
        more can follow.
      </p>
    </section>

    <!-- Closing CTA band: repeat the primary conversion ask, auth-branched. -->
    <section class="mt-20 sm:mt-24">
      <div class="bg-card rounded-2xl border p-8 text-center sm:p-12">
        <template v-if="auth.isAuthenticated">
          <h2 class="text-2xl font-semibold tracking-tight text-balance sm:text-3xl">
            Pick up where you left off
          </h2>
          <p class="text-muted-foreground mx-auto mt-3 max-w-xl text-pretty">
            Jump back into your collection and see what you're still chasing.
          </p>
          <div class="mt-6 flex flex-col items-center justify-center gap-3 sm:flex-row">
            <RouterLink to="/collection" :class="buttonVariants({ size: 'lg' })">
              Open your collection
              <ArrowRight aria-hidden="true" />
            </RouterLink>
            <RouterLink to="/cards" :class="buttonVariants({ variant: 'outline', size: 'lg' })">
              Browse cards
            </RouterLink>
          </div>
        </template>
        <template v-else-if="auth.sessionResolved">
          <h2 class="text-2xl font-semibold tracking-tight text-balance sm:text-3xl">
            Start tracking in minutes
          </h2>
          <p class="text-muted-foreground mx-auto mt-3 max-w-xl text-pretty">
            Create a free account to track your collection and wish list — or keep browsing cards,
            sealed products, and prices with no sign-up at all.
          </p>
          <div class="mt-6 flex flex-col items-center justify-center gap-3 sm:flex-row">
            <RouterLink to="/register" :class="buttonVariants({ size: 'lg' })">
              Create a free account
              <ArrowRight aria-hidden="true" />
            </RouterLink>
            <RouterLink to="/login" :class="buttonVariants({ variant: 'outline', size: 'lg' })">
              Sign in
            </RouterLink>
          </div>
        </template>
        <!-- Session unresolved: reserve the band's heading, copy, and two lg CTAs so the
             closing section keeps its height while the auth branch resolves. -->
        <template v-else>
          <Skeleton class="mx-auto h-8 w-72 max-w-full" />
          <div class="mx-auto mt-3 max-w-xl space-y-2">
            <Skeleton class="mx-auto h-4 w-full max-w-md" />
            <Skeleton class="mx-auto h-4 w-4/5 max-w-sm" />
          </div>
          <div class="mt-6 flex flex-col items-center justify-center gap-3 sm:flex-row">
            <Skeleton class="h-10 w-52" />
            <Skeleton class="h-10 w-32" />
          </div>
        </template>
      </div>
    </section>
  </div>
</template>
