<script setup lang="ts">
import { Dices, PackageOpen } from '@lucide/vue'
import DemoCardTile from './DemoCardTile.vue'

// Presentational-only mock of a sealed product's expected-value panel and its seeded pack
// opener, used by the homepage's feature-demo rows. Nothing here fetches, and nothing is
// interactive — the whole panel sits inside FeatureDemoRow's aria-hidden demo box.
//
// Every money figure keeps its qualifier on purpose: an expectation is "per pack, on average",
// and a value read against the price is a *share of today's price*, never a gain, a profit, or
// a promise — and no number here is worded as a copy's contents. See AGENTS.md, "Don't break
// these" → "No number on a sealed product's page is a count of a copy's physical cards" and
// "Booster odds", mirrored by the real wording helpers in `@/lib/productCounts`
// (`expectedValueHeading`, `cardsPerPackLabel`, `oddsLabel`, `evVersusPrice`, `openingSummary`).
</script>

<template>
  <!-- Decorative mock UI — illustrative values, not real market data. -->
  <div>
    <div class="flex items-center justify-between gap-2">
      <div class="min-w-0">
        <p class="text-sm font-semibold">Collector Booster</p>
        <p class="text-muted-foreground mt-0.5 text-xs">14–15 cards per pack</p>
      </div>
      <span
        class="border-primary/30 bg-primary/10 text-primary shrink-0 rounded-full border px-2 py-0.5 text-[10px] font-medium"
      >
        <Dices class="mr-1 inline size-3" aria-hidden="true" />
        Booster odds
      </span>
    </div>

    <dl class="mt-4 flex flex-wrap gap-x-8 gap-y-3">
      <div>
        <dt class="text-muted-foreground text-xs tracking-wide uppercase">
          Expected value per pack
        </dt>
        <dd class="text-xl font-semibold tabular-nums">$28.40</dd>
        <dd class="text-muted-foreground text-[10px]">on average</dd>
      </div>
      <div>
        <dt class="text-muted-foreground text-xs tracking-wide uppercase">Today's price</dt>
        <dd class="text-xl font-semibold tabular-nums">$24.99</dd>
      </div>
    </dl>
    <p class="text-muted-foreground mt-2 text-xs">114% of today's price, on average</p>

    <div class="mt-4 space-y-1.5 border-t pt-3 text-xs">
      <div class="flex items-center justify-between gap-3">
        <span class="text-muted-foreground">Any serialized card</span>
        <span class="tabular-nums">1 in 24 packs</span>
      </div>
      <div class="flex items-center justify-between gap-3">
        <span class="text-muted-foreground">Foil mythic</span>
        <span class="tabular-nums">1 in 6 packs</span>
      </div>
      <div class="flex items-center justify-between gap-3">
        <span class="text-muted-foreground">Extended-art rare</span>
        <span class="tabular-nums">1 in 3 packs</span>
      </div>
    </div>

    <div class="mt-4 rounded-lg border p-3">
      <div class="flex items-center justify-between gap-2">
        <span class="flex items-center gap-1.5 text-xs font-semibold">
          <PackageOpen class="size-3.5" aria-hidden="true" />
          Open a pack
        </span>
        <span class="text-muted-foreground text-[10px] tabular-nums"> One pack · seed 4821 </span>
      </div>

      <div class="mt-3 grid grid-cols-5 gap-2">
        <DemoCardTile :bars="false" gradient="muted" />
        <div class="relative">
          <DemoCardTile :bars="false" />
          <span
            class="bg-rarity-mythic/15 text-rarity-mythic absolute -top-1 left-0 rounded-full px-1.5 py-0.5 text-[10px] font-medium"
          >
            Mythic
          </span>
        </div>
        <DemoCardTile :bars="false" gradient="muted" />
        <div class="ring-foil relative rounded-lg ring-1">
          <DemoCardTile :bars="false" />
          <span
            class="bg-foil/15 text-foil absolute -top-1 left-0 rounded-full px-1.5 py-0.5 text-[10px] font-medium"
          >
            Foil
          </span>
        </div>
        <DemoCardTile :bars="false" gradient="muted" />
      </div>

      <p class="text-muted-foreground mt-3 text-[10px]">
        This run dealt $31.20 — 125% of what one copy costs at today's price
      </p>
    </div>
  </div>
</template>
