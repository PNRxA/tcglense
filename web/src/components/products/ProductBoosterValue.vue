<script setup lang="ts">
import { computed, toRef } from 'vue'
import type { Product } from '@/lib/api'
import { Skeleton } from '@/components/ui/skeleton'
import { useProductEvQuery } from '@/composables/useProducts'
import { useCurrency } from '@/composables/useCurrency'
import { useDetailModalLink } from '@/composables/useDetailModalLink'
import {
  boosterLabel,
  cardsPerPackLabel,
  evVersusPrice,
  expectedValueHeading,
  oddsLabel,
  pricedShareLabel,
} from '@/lib/productCounts'

// What an average copy of this sealed product is worth at today's prices (issue #682),
// built entirely from the server's answer (`GET /api/games/{game}/products/{id}/ev`) so a
// CLI asking the same question gets the same panel's worth of answer.
//
// This is the first *money* number on a sealed page that isn't a price, and it is the one a
// visitor is most likely to misread: an expectation over many openings says nothing about
// the copy in front of them. So the wording is not free-form here — every label comes from
// the `lib/productCounts.ts` vocabulary seam that already guards the card manifest's
// numbers, and it is what keeps the unit ("per pack" vs "per copy"), the average, and the
// odds from ever reading as containment or as a guarantee.
//
// The slot table and the contributors list are the panel's claim to being checkable: the
// headline is a sum of the sheets underneath it, and the cards it mostly sits in are named
// and linked. The caveats are the server's own, shown verbatim — they say what the model
// assumes (with-replacement draws, no colour balancing, unpriced cards at $0), and a
// number nobody can audit is a number nobody should believe.
//
// It self-hides for a product with no booster sheets (the API answers `data: null` — a
// precon deck, a product MTGJSON doesn't describe) and, on a failed fetch, hides too: a
// public catalog read that didn't land must not push an error into the middle of the page.
const props = defineProps<{
  game: string
  id: string
  // The product itself, for the "vs the current price" line only — the panel mounts off the
  // route ids and never waits on it (it may still be loading, or carry no price at all).
  product?: Product
}>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')
const money = useCurrency()

const evQuery = useProductEvQuery(game, id)
const ev = computed(() => evQuery.data.value?.data ?? null)
const pending = computed(() => evQuery.isPending.value)
const show = computed(() => !evQuery.isError.value && (pending.value || ev.value !== null))

const heading = computed(() => (ev.value ? expectedValueHeading(ev.value) : null))
const vsPrice = computed(() =>
  ev.value ? evVersusPrice(ev.value.ev_usd, props.product?.prices?.usd) : null,
)

/** The API sends up to 12 contributors per copy; the panel is a summary, not a table of the
 * pool, so it shows the head of that list. */
const TOP_CONTRIBUTORS = 8
const contributors = computed(() => ev.value?.top.slice(0, TOP_CONTRIBUTORS) ?? [])

const { hrefFor, onActivate, warm } = useDetailModalLink()

/** Picks are an expectation over the pack's variants, so a slot that isn't a whole number
 * keeps its decimals rather than rounding to a count it never deals. */
function picksLabel(picks: number): string {
  return Number.isInteger(picks) ? String(picks) : picks.toFixed(2)
}
</script>

<template>
  <section v-if="show">
    <!-- Exactly "Expected value": the unit and the qualification go beneath, so the heading
      itself stays a stable landmark (and a screenshot script can wait on it). -->
    <h2 class="mb-1 text-base font-semibold tracking-tight">Expected value</h2>

    <!-- The resting shape while the read lands, so the page doesn't jump when it does. -->
    <template v-if="!ev || !heading">
      <Skeleton class="h-3 w-72 max-w-full" />
      <Skeleton class="mt-3 h-8 w-40" />
      <Skeleton class="mt-3 h-24 w-full" />
    </template>

    <template v-else>
      <p class="text-muted-foreground mb-3 text-xs">{{ heading.blurb }}</p>

      <!-- The headline, its unit, and — when the product has a price — the comparison. -->
      <div class="flex flex-wrap items-baseline gap-x-3 gap-y-1">
        <span class="text-2xl font-semibold tabular-nums">{{ money.formatUsd(ev.ev_usd) }}</span>
        <span class="text-muted-foreground text-sm">{{ heading.unit }}</span>
      </div>
      <p v-if="vsPrice" class="text-muted-foreground mt-1 text-xs">{{ vsPrice.label }}</p>

      <!-- One block per booster a copy opens: what it deals, what it's worth, and the
        sheets that sum to that figure. -->
      <div class="mt-4 space-y-3">
        <div
          v-for="pack in ev.packs"
          :key="`${pack.set_code}:${pack.booster_code}`"
          class="rounded-lg border p-3"
        >
          <div class="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
            <h3 class="text-sm font-medium">
              <span v-if="pack.quantity > 1" class="text-muted-foreground tabular-nums"
                >{{ pack.quantity }}×</span
              >
              {{ boosterLabel(pack) }}
            </h3>
            <p class="text-sm tabular-nums">
              <span class="font-semibold">{{ money.formatUsd(pack.ev_usd) }}</span>
              <span class="text-muted-foreground ml-1 text-xs">per pack, on average</span>
            </p>
          </div>
          <p class="text-muted-foreground mt-0.5 text-xs">
            <!-- The one per-pack card count on this page that is a real count of cards —
              and it is worded per pack, never as the product's contents. -->
            {{ cardsPerPackLabel(pack.cards_per_pack) }}
            <template v-if="pack.priced_share < 1">
              · {{ pricedShareLabel(pack.priced_share) }}
            </template>
          </p>

          <!-- The sheets the pack draws from. Scrolls inside itself on a narrow screen
            rather than widening the page. -->
          <div v-if="pack.slots.length" class="mt-2 overflow-x-auto">
            <table class="w-full text-left text-xs">
              <thead class="text-muted-foreground">
                <tr>
                  <th scope="col" class="py-1 pr-3 font-medium">Sheet</th>
                  <th scope="col" class="py-1 pr-3 text-right font-medium">Picks</th>
                  <th scope="col" class="py-1 pr-3 text-right font-medium">Value</th>
                  <th scope="col" class="py-1 font-medium">Best pull</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="slot in pack.slots" :key="slot.sheet" class="border-t">
                  <td class="py-1 pr-3">
                    <span class="align-middle">{{ slot.sheet }}</span>
                    <span
                      v-if="slot.foil"
                      class="bg-foil/15 text-foil ml-1.5 rounded px-1 py-0.5 align-middle text-[0.65rem] font-semibold tracking-wide uppercase"
                      >Foil</span
                    >
                  </td>
                  <td class="py-1 pr-3 text-right tabular-nums">{{ picksLabel(slot.picks) }}</td>
                  <td class="py-1 pr-3 text-right tabular-nums">
                    {{ money.formatUsd(slot.ev_usd) }}
                  </td>
                  <td class="py-1">
                    <a
                      v-if="slot.top[0]"
                      :href="hrefFor('card', game, slot.top[0].card.id)"
                      class="hover:underline"
                      @click="onActivate($event, 'card', game, slot.top[0].card.id)"
                      @pointerenter="warm('card')"
                      @focusin="warm('card')"
                      >{{ slot.top[0].card.name }}</a
                    >
                    <span v-else class="text-muted-foreground">—</span>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>
      </div>

      <!-- Where the expectation actually sits, so the headline can be audited against the
        cards that made it. Odds are the API's, worded by the vocabulary seam. -->
      <template v-if="contributors.length">
        <h3 class="mt-4 text-sm font-medium">Biggest contributors</h3>
        <p class="text-muted-foreground text-xs">
          The cards most of the expected value sits in, per copy.
        </p>
        <ul class="mt-1.5">
          <li
            v-for="entry in contributors"
            :key="`${entry.sheet}:${entry.card.id}:${entry.foil}`"
            class="flex flex-wrap items-baseline gap-x-2 gap-y-0.5 border-t py-1.5 text-sm"
          >
            <a
              :href="hrefFor('card', game, entry.card.id)"
              class="min-w-0 truncate font-medium hover:underline"
              @click="onActivate($event, 'card', game, entry.card.id)"
              @pointerenter="warm('card')"
              @focusin="warm('card')"
              >{{ entry.card.name }}</a
            >
            <span
              v-if="entry.foil"
              class="bg-foil/15 text-foil rounded px-1 py-0.5 text-[0.65rem] font-semibold tracking-wide uppercase"
              >Foil</span
            >
            <span class="text-muted-foreground text-xs">{{ oddsLabel(entry.one_in) }}</span>
            <span class="text-muted-foreground ml-auto text-xs tabular-nums">{{
              money.formatUsd(entry.price_usd) ?? 'No price'
            }}</span>
            <span class="w-20 text-right font-medium tabular-nums">{{
              money.formatUsd(entry.contribution_usd)
            }}</span>
          </li>
        </ul>
      </template>

      <!-- The server's own caveats, verbatim: what the model assumes and what it can't see. -->
      <ul v-if="ev.caveats.length" class="text-muted-foreground mt-3 space-y-1 text-xs">
        <li v-for="(caveat, index) in ev.caveats" :key="index">{{ caveat }}</li>
      </ul>
    </template>
  </section>
</template>
