<script setup lang="ts">
import { Dices, PackageOpen } from '@lucide/vue'
import DemoCardTile from './DemoCardTile.vue'

// Four of the cards one mocked pack dealt: two chase pulls carry the real opener's rarity
// and foil chips (`bg-rarity-mythic/15` / `bg-foil/15`), the rest are plain tiles.
const pulls = [
  { key: 1, gradient: 'muted' as const },
  {
    key: 2,
    gradient: 'primary' as const,
    chip: 'Mythic',
    chipClass: 'bg-rarity-mythic/15 text-rarity-mythic',
  },
  {
    key: 3,
    gradient: 'primary' as const,
    foil: true,
    chip: 'Foil',
    chipClass: 'bg-foil/15 text-foil',
  },
  { key: 4, gradient: 'muted' as const },
]

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

      <!-- The rarity / foil chips overlay their tiles, anchored inside the tile's bounds
           (a top-left inset with a max width that truncates) rather than hanging off its
           corner: four tiles share a half-width panel at md, and at phone width a tile is
           barely wider than the chip. -->
      <div class="mt-3 grid grid-cols-4 gap-2">
        <div v-for="pull in pulls" :key="pull.key" class="relative min-w-0">
          <div :class="pull.foil ? 'ring-foil rounded-lg ring-1' : ''">
            <DemoCardTile :bars="false" :gradient="pull.gradient" />
          </div>
          <span
            v-if="pull.chip"
            class="absolute top-1 left-1 max-w-[calc(100%-0.5rem)] truncate rounded px-1 py-0.5 text-[10px] leading-none font-medium"
            :class="pull.chipClass"
          >
            {{ pull.chip }}
          </span>
        </div>
      </div>

      <p class="text-muted-foreground mt-3 text-[10px]">
        This run dealt $31.20 — 125% of what one copy costs at today's price
      </p>
    </div>
  </div>
</template>
