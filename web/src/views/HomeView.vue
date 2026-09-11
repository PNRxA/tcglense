<script setup lang="ts">
import { computed } from 'vue'
import {
  ArrowRight,
  Bell,
  BookCopy,
  BookOpen,
  Boxes,
  CalendarDays,
  ChartPie,
  ChevronRight,
  CircleCheck,
  Code,
  Dices,
  ExternalLink,
  Ghost,
  Heart,
  HeartPulse,
  IdCard,
  Import,
  Layers,
  Library,
  LibraryBig,
  Lock,
  Package,
  PackageOpen,
  ScanLine,
  Search,
  Share2,
  ShoppingCart,
  Terminal,
  TrendingUp,
} from '@lucide/vue'
import { RouterLink } from 'vue-router'
import GitHubMark from '@/components/GitHubMark.vue'
import BoosterOddsDemo from '@/components/home/BoosterOddsDemo.vue'
import DeckOverviewDemo from '@/components/home/DeckOverviewDemo.vue'
import DemoCardTile from '@/components/home/DemoCardTile.vue'
import FeatureDemoRow from '@/components/home/FeatureDemoRow.vue'
import FeatureLinkCard, { type FeatureLink } from '@/components/home/FeatureLinkCard.vue'
import LifeCounterDemo from '@/components/home/LifeCounterDemo.vue'
import PreconDemo from '@/components/home/PreconDemo.vue'
import ScannerFeatureDemo from '@/components/home/ScannerFeatureDemo.vue'
import UniversalSearchBox from '@/components/search/UniversalSearchBox.vue'
import { buttonVariants } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { useGamesQuery } from '@/composables/useCatalog'
import { usePageMeta } from '@/lib/seo'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()

usePageMeta({
  description:
    'Browse every set, card, sealed product, and preconstructed deck, chart daily prices, ' +
    'build and analyse decks, and track your collection and wish list.',
  canonicalPath: '/',
})

// The games registry feeds the universal search section at the top of the page: the box
// searches the first game (today the only one) and offers a picker when there are several.
// Resilient to an empty/loading list — the box renders at once and its queries wait for a
// game.
const gamesQuery = useGamesQuery()
const games = computed(() => gamesQuery.data.value?.data ?? [])

// The "everything else" section, in two weights. The headline features — the ones that
// used to carry a full demo row of their own (the page showed twelve, which made it a very
// long scroll), plus the search grammar and the public API — render as cards.
const headlineFeatures: FeatureLink[] = [
  {
    icon: Library,
    title: 'Collection tracking',
    description:
      'Regular and foil counts per game, sealed products beside them, and a live estimated ' +
      'value with its history and biggest movers.',
    to: '/collection',
  },
  {
    icon: Bell,
    title: 'Price alerts',
    description:
      'A Discord, Telegram, or email ping when a card or sealed product crosses your target ' +
      '— plus day-before release heads-ups.',
    to: '/alerts',
  },
  {
    icon: TrendingUp,
    title: 'Daily price history',
    description:
      'Singles captured daily in USD, EUR, and foil, with USD and foil charted on every ' +
      'card from 7 days to the full history — and a price history on every sealed product.',
    to: '/cards',
  },
  {
    icon: Heart,
    title: 'Wish lists',
    description:
      "The cards you're still hunting, with a running USD total and a one-click TCGplayer " +
      'mass-entry link for the whole list.',
    to: '/wishlist',
  },
  {
    icon: Import,
    title: 'Import your collection',
    description:
      'Archidekt by link — or a CSV export from Archidekt, Moxfield, ManaBox, or Mythic ' +
      'Tools, or your list pasted straight in.',
    to: '/collection',
  },
  {
    icon: Ghost,
    title: 'Ghost mode',
    description:
      "Dim the cards you're missing in any set, with a live owned count and a quick-add button " +
      'on every gap.',
    to: '/collection',
  },
  {
    icon: Search,
    title: 'Scryfall-style search',
    description:
      'Full search syntax on every card list — colors, types, oracle text, prices, even regex.',
    to: '/cards',
  },
  {
    icon: Terminal,
    title: 'CLI & agents',
    description:
      'A standalone, open-source CLI and TUI for this API — scriptable output and scoped ' +
      'keys for automation and AI agents.',
    href: 'https://github.com/PNRxA/tcglense-cli',
  },
  {
    icon: Code,
    title: 'Public API',
    description:
      'A documented public API for the catalog, plus scoped API keys for your collection, ' +
      'wish list, and decks. Interactive reference included.',
    to: '/docs',
  },
]

// The rest render as compact rows under an "Also live" label: every one a shipped feature
// that links to where it lives, just not one that needs a card to explain itself.
const otherFeatures: FeatureLink[] = [
  {
    icon: CalendarDays,
    title: 'Release calendar',
    description:
      'Set releases and Secret Lair drops month by month, with what each one ships — and an ' +
      'optional day-before heads-up.',
    to: '/releases',
  },
  {
    icon: BookOpen,
    title: 'Keyword glossary',
    description: 'Every rules keyword and ability word explained, with the cards that carry it.',
    to: '/keywords',
  },
  {
    icon: ShoppingCart,
    title: 'Shopping lists',
    description:
      'Buy a whole wish list, or the cards a deck still needs, in one click — a TCGplayer ' +
      'mass-entry link or a list for MTG Mate.',
    to: '/wishlist',
  },
  {
    icon: ChartPie,
    title: 'Collection insights',
    description:
      'Value history, biggest movers, and a breakdown by rarity, colour, type, and finish — ' +
      'plus your top holdings by value.',
    to: '/collection',
  },
  {
    icon: Share2,
    title: 'Public profiles',
    description:
      'Share your collection and wish list per game, and any deck on its own, read-only at ' +
      'your own /u/handle — only when you switch it on.',
    to: '/collection',
  },
  {
    icon: IdCard,
    title: 'Print details & buy links',
    description:
      'Artist, flavour text, finishes, frame, and Reserved List on every card page — plus ' +
      'links to TCGplayer, Cardmarket, Gatherer, and EDHREC.',
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
    icon: Lock,
    title: 'Free accounts',
    description:
      'Register with just an email address — free to track your collection, wish list, and ' +
      'decks.',
    to: '/register',
  },
  {
    icon: GitHubMark,
    title: 'Open source',
    description: 'The whole app — the API and this site — is public on GitHub.',
    href: 'https://github.com/PNRxA/tcglense',
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
    <!-- Universal search: its own section, front and centre above the hero — the fastest way
         off the homepage to the thing you came for. One box across cards, sealed products,
         precons and keywords, plus your own decks once signed in. It is set at hero scale
         (the heading a size under the h1, a taller input with an accent-tinted border and a
         lifted shadow, and a chip row naming what the box covers) so it reads as the page's
         lead action rather than a heading floating over the hero. The dropdown overlays the
         hero below it (z-40 inside the box), so nothing here needs to reserve space for it. -->
    <section aria-labelledby="home-search-heading" class="mx-auto max-w-3xl text-center">
      <h2
        id="home-search-heading"
        class="text-4xl font-semibold tracking-tight text-balance sm:text-5xl"
      >
        Search the whole catalog
      </h2>
      <p class="text-muted-foreground mx-auto mt-4 max-w-xl text-pretty sm:text-lg">
        Cards, sealed products, preconstructed decks, and rules keywords in one box — plus your own
        decks once you're signed in.
      </p>
      <UniversalSearchBox
        :games="games"
        class="mt-8 text-left [&_input]:h-14 [&_input]:border-primary/40 [&_input]:text-lg [&_input]:shadow-lift [&_input]:ring-1 [&_input]:ring-primary/10 md:[&_input]:text-lg"
      />
      <ul
        class="text-muted-foreground mt-5 flex flex-wrap justify-center gap-x-6 gap-y-2 text-sm"
        aria-label="What the search covers"
      >
        <li class="inline-flex items-center gap-1.5">
          <Layers class="text-primary size-4" aria-hidden="true" />
          Cards
        </li>
        <li class="inline-flex items-center gap-1.5">
          <Package class="text-primary size-4" aria-hidden="true" />
          Sealed products
        </li>
        <li class="inline-flex items-center gap-1.5">
          <BookCopy class="text-primary size-4" aria-hidden="true" />
          Precons
        </li>
        <li class="inline-flex items-center gap-1.5">
          <BookOpen class="text-primary size-4" aria-hidden="true" />
          Keywords
        </li>
      </ul>
    </section>

    <!-- Hero: value prop + auth-branched CTAs, beside a decorative "show the product" vignette. -->
    <section
      class="mt-16 grid items-center gap-10 sm:mt-24 lg:grid-cols-[1fr_minmax(0,30rem)] lg:gap-14"
    >
      <div>
        <h1 class="text-4xl font-semibold tracking-tight text-balance sm:text-5xl">
          Your collection, priced every day.
        </h1>
        <p class="text-muted-foreground mt-4 max-w-xl text-base text-pretty sm:text-lg">
          TCGLense is a free, open-source tracker for trading-card games: browse every set, chart
          singles and sealed prices day by day, build and analyse your decks, and see exactly which
          cards you own — and which ones you're still missing.
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
         sides at md+. Deliberately a short list — only the features whose demo earns the
         vertical space it costs. Everything else is a card in the compact grid below. -->
    <section class="mt-16 sm:mt-20">
      <div class="space-y-16 sm:space-y-20">
        <!-- Row 1 — Decks (demo left): the v0.18 headline, so it leads. -->
        <FeatureDemoRow
          :icon="BookCopy"
          eyebrow="Decks"
          heading="Build a deck, then let the numbers talk"
          :body="
            'Build decks from the catalog or import them — an Archidekt link, or an Archidekt, ' +
            'Moxfield, or plain-text export file — then let the analysis stack read them: draw ' +
            'odds, a legality verdict, an ' +
            'estimated Commander bracket, the mana base against its pips, card roles, the tokens ' +
            'to bring, combos from Commander Spellbook, and a pricing breakdown with one-click ' +
            'cheapest-printing swaps. Draw a test hand, diff two decks, or share one with a link.'
          "
          demo-side="left"
        >
          <template v-if="auth.isAuthenticated">
            <RouterLink to="/decks" :class="rowLinkClass">
              Open your decks
              <ArrowRight class="size-4" aria-hidden="true" />
            </RouterLink>
          </template>
          <template v-else-if="auth.sessionResolved">
            <RouterLink to="/decks" :class="rowLinkClass">
              Start a deck
              <ArrowRight class="size-4" aria-hidden="true" />
            </RouterLink>
          </template>
          <Skeleton v-else class="h-5 w-32" />
          <template #demo>
            <DeckOverviewDemo />
          </template>
        </FeatureDemoRow>

        <!-- Row 2 — Preconstructed decks (demo right). -->
        <FeatureDemoRow
          :icon="Boxes"
          eyebrow="Preconstructed decks"
          heading="Every published precon, ready to copy"
          :body="
            'Browse every preconstructed decklist in the catalog — by set, by deck type, or by ' +
            'price — with the same legality, bracket, mana, roles, tokens, combos, and draw-odds ' +
            'analysis your own decks get. Copy one into your decks to start upgrading it, or add ' +
            'its cards to your collection the day you buy it.'
          "
          demo-side="right"
        >
          <RouterLink to="/precons" :class="rowLinkClass">
            Browse preconstructed decks
            <ArrowRight class="size-4" aria-hidden="true" />
          </RouterLink>
          <template #demo>
            <PreconDemo />
          </template>
        </FeatureDemoRow>

        <!-- Row 3 — Booster odds (demo left). -->
        <FeatureDemoRow
          :icon="Dices"
          eyebrow="Booster odds"
          heading="Know what a pack is worth before you open it"
          :body="
            'Every booster with published sheet data gets an expected value — what an average ' +
            'pack deals, priced against today\'s singles and shown as a share of the product\'s ' +
            'price — plus the odds of its chase slots. Then open a seeded pack, or a whole box, in ' +
            'the browser: the same seed always deals the same packs, so a result is a link you can ' +
            'share.'
          "
          demo-side="left"
        >
          <RouterLink to="/sealed" :class="rowLinkClass">
            Browse sealed products
            <ArrowRight class="size-4" aria-hidden="true" />
          </RouterLink>
          <template #demo>
            <BoosterOddsDemo />
          </template>
        </FeatureDemoRow>

        <!-- Row 4 — Visual card scanner (demo right). -->
        <FeatureDemoRow
          :icon="ScanLine"
          eyebrow="Visual card scanner"
          heading="Turn a stack of Magic cards into your collection"
          :body="
            'Use your phone or webcam to match each card\'s artwork, confirm the exact printing, ' +
            'and add it as you work through a stack. Photos are processed locally and never ' +
            'uploaded; compact visual fingerprints are sent for matching.'
          "
          demo-side="right"
        >
          <template v-if="auth.isAuthenticated">
            <RouterLink to="/scan" :class="rowLinkClass">
              Scan Magic cards
              <ArrowRight class="size-4" aria-hidden="true" />
            </RouterLink>
          </template>
          <template v-else-if="auth.sessionResolved">
            <RouterLink
              :to="{ path: '/register', query: { redirect: '/scan' } }"
              :class="rowLinkClass"
            >
              Create a free account to scan
              <ArrowRight class="size-4" aria-hidden="true" />
            </RouterLink>
          </template>
          <Skeleton v-else class="h-5 w-48" />
          <template #demo>
            <ScannerFeatureDemo />
          </template>
        </FeatureDemoRow>

        <!-- Row 5 — Life counter (demo left). -->
        <FeatureDemoRow
          :icon="HeartPulse"
          eyebrow="Life counter"
          heading="A life counter that remembers the game"
          :body="
            'Track life for a table of up to six, with poison, energy, experience, and commander ' +
            'damage per opponent, and a full history so any tap can be undone. Name the deck each ' +
            'seat played and every finished game builds a win record for your decks — then ' +
            'rematch the pod with one tap.'
          "
          demo-side="left"
        >
          <RouterLink to="/tools" :class="rowLinkClass">
            Open the play aids
            <ArrowRight class="size-4" aria-hidden="true" />
          </RouterLink>
          <template #demo>
            <LifeCounterDemo />
          </template>
        </FeatureDemoRow>
      </div>
    </section>

    <!-- Everything else: every other shipped feature, each a link to where it lives, in two
         weights — headline cards, then compact rows — both through FeatureLinkCard, so an
         internal feature is a RouterLink and an external one a new-tab anchor without a
         hand-written element for either. -->
    <section class="mt-16 sm:mt-20">
      <h2 class="text-xl font-semibold tracking-tight">Everything else that's live today</h2>
      <p class="text-muted-foreground mt-1 text-sm">
        No roadmap padding — every line here is a shipped feature.
      </p>
      <div class="mt-6 grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        <FeatureLinkCard
          v-for="feature in headlineFeatures"
          :key="feature.title"
          :feature="feature"
        />
      </div>
      <h3 class="text-muted-foreground mt-10 text-xs font-semibold tracking-wide uppercase">
        Also live
      </h3>
      <div class="mt-3 grid gap-x-6 gap-y-1 sm:grid-cols-2">
        <FeatureLinkCard
          v-for="feature in otherFeatures"
          :key="feature.title"
          :feature="feature"
          variant="row"
        />
      </div>
    </section>

    <!-- Built on open data: prominent credits for the four open data projects behind TCGLense. -->
    <section class="mt-16 sm:mt-20">
      <h2 class="text-xl font-semibold tracking-tight">Built on open data</h2>
      <p class="text-muted-foreground mt-1 text-sm text-pretty">
        Every price, card, box, decklist, and combo on TCGLense traces back to four open data
        projects.
      </p>
      <div class="mt-6 grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
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
            The entire card catalog — sets, cards, rulings, keywords, and the daily singles prices
            behind every chart — is built from Scryfall's bulk data. Card images are served courtesy
            of Scryfall.
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
            Sealed contents, booster sheets &amp; precons
          </p>
          <p class="text-muted-foreground mt-2 text-sm text-pretty">
            Which cards a sealed product contains or can be pulled from, each booster's sheet
            configuration behind the expected value and the pack opener, and every published
            preconstructed decklist.
          </p>
          <p class="text-muted-foreground mt-3 text-xs">mtgjson.com</p>
        </a>
        <a
          href="https://commanderspellbook.com"
          target="_blank"
          rel="noopener noreferrer"
          class="bg-card hover:border-ring/60 hover:bg-accent/40 group block rounded-xl border p-5 transition-colors"
        >
          <div class="flex items-center justify-between gap-2">
            <span class="font-semibold">Commander Spellbook</span>
            <ExternalLink
              class="text-muted-foreground group-hover:text-foreground size-4 shrink-0 transition-colors"
              aria-hidden="true"
            />
          </div>
          <p class="text-primary mt-0.5 text-xs font-medium tracking-wide uppercase">
            Combo database
          </p>
          <p class="text-muted-foreground mt-2 text-sm text-pretty">
            Which cards go infinite together — the curated combo database behind the combos panel on
            every deck. Each combo links back to its Commander Spellbook page.
          </p>
          <p class="text-muted-foreground mt-3 text-xs">commanderspellbook.com</p>
        </a>
      </div>
      <p class="text-muted-foreground mt-3 text-xs text-pretty">
        All four are independent projects. None of them produces, endorses, or is affiliated with
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
    <section v-if="games.length" class="mt-16 sm:mt-20">
      <h2 class="text-xl font-semibold tracking-tight">Start with your game</h2>
      <p class="text-muted-foreground mt-1 text-sm">
        Browse the full catalog, sealed products, and precons — no account needed.
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
        <RouterLink
          to="/precons"
          class="bg-card hover:border-ring/60 hover:bg-accent/40 inline-flex items-center gap-2 rounded-full border px-4 py-2 text-sm font-medium transition-colors"
        >
          <Boxes class="text-muted-foreground size-4" aria-hidden="true" />
          Preconstructed decks
          <ChevronRight class="text-muted-foreground size-4" aria-hidden="true" />
        </RouterLink>
      </div>
      <p class="text-muted-foreground mt-3 text-sm text-pretty">
        Magic: The Gathering is the first game on TCGLense — the catalog is built game-agnostic, so
        more can follow.
      </p>
    </section>

    <!-- Closing CTA band: repeat the primary conversion ask, auth-branched. -->
    <section class="mt-16 sm:mt-20">
      <div class="bg-card rounded-2xl border p-8 text-center sm:p-12">
        <template v-if="auth.isAuthenticated">
          <h2 class="text-2xl font-semibold tracking-tight text-balance sm:text-3xl">
            Pick up where you left off
          </h2>
          <p class="text-muted-foreground mx-auto mt-3 max-w-xl text-pretty">
            Jump back into your collection and decks and see what you're still chasing.
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
            Create a free account to track your collection, wish list, and decks — or keep browsing
            cards, sealed products, precons, and prices with no sign-up at all.
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
