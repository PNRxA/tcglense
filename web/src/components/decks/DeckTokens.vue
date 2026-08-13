<script setup lang="ts">
import { computed, ref, toRef, useId } from 'vue'
import { ChevronDown, Sparkles } from '@lucide/vue'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import CardTile from '@/components/cards/CardTile.vue'
import UpdatingCue from '@/components/cards/UpdatingCue.vue'
import {
  useDeckTokensQuery,
  usePreconTokensQuery,
  usePublicDeckTokensQuery,
} from '@/composables/useDeckAnalysis'
import { useDetailModalLink } from '@/composables/useDetailModalLink'
import { DECK_CARD_SIZE_GRID_CLASS } from '@/lib/cardSize'
import { useCardSizeStore } from '@/stores/cardSize'

// **Tokens to bring**: the tokens and emblems this deck's cards make, which is the one thing
// a decklist requires that a decklist doesn't contain. It sits at the very bottom of the
// deck page on purpose — it's the last thing you check while packing a box, not something to
// read past on the way to the cards.
//
// Every claim here is the server's (`GET /api/decks/{game}/{deck_id}/tokens`, or its public
// and precon mirrors), read off the catalog's per-card token relations rather than parsed out
// of rules text, so a CLI packing list and this panel can't disagree.
//
// Two deliberate silences, both inherited from the response:
//
// * **No "bring N of these".** Nothing upstream distinguishes "create a Treasure" from
//   "create X Treasures", so a number here would be invented. What the panel shows instead is
//   which of the deck's cards make each token — the thing a player actually reasons from.
// * **"Not checked yet" is not "makes none".** A catalog row imported before token data
//   existed says nothing about tokens, and the note under the grid says so rather than
//   letting the list read as complete.
//
// `handle` / `preconSlug` pick the surface, chosen ONCE at mount exactly as `DeckStats` and
// `DeckBracket` do.
const props = defineProps<{
  game: string
  /** Addressing is fixed at mount and mutually exclusive: a deck id (the owner's own deck),
   *  a `handle` + deck id (a shared deck), or a `preconSlug` (a published catalog decklist,
   *  which has no deck row and no numeric id). Exactly one applies. */
  deckId?: number
  handle?: string
  preconSlug?: string
}>()

const game = toRef(props, 'game')
const deckId = computed(() => props.deckId ?? 0)
const handle = computed(() => props.handle ?? '')
const preconSlug = computed(() => props.preconSlug ?? '')

// Three addressing modes, selected ONCE at mount — see the props.
const tokensQuery = props.preconSlug
  ? usePreconTokensQuery(game, preconSlug)
  : props.handle
    ? usePublicDeckTokensQuery(handle, deckId)
    : useDeckTokensQuery(game, deckId)

const tokens = computed(() => tokensQuery.data.value?.tokens ?? [])
const uncheckedCount = computed(() => tokensQuery.data.value?.unchecked_count ?? 0)
const pending = computed(() => tokensQuery.isPending.value)
const updating = computed(() => tokensQuery.isFetching.value && !pending.value)

const cardSize = useCardSizeStore()

const expanded = ref(false)
const detailsId = useId()

/** What the corner badge counts — cards, never tokens. The number is only ever "how many of
 *  this deck's cards make this", so it says so on hover rather than leaving a bare "×3" to
 *  be read as "bring three". */
function sourceCountTitle(count: number): string {
  return `${count} card${count === 1 ? '' : 's'} in this deck make${count === 1 ? 's' : ''} it`
}

const { hrefFor, onActivate, warm } = useDetailModalLink()
</script>

<template>
  <Card class="mt-8 gap-3 py-4" :aria-busy="updating || undefined">
    <CardHeader class="pb-0">
      <CardTitle class="flex items-center gap-2 text-base">
        <Sparkles class="size-4" aria-hidden="true" /> Tokens to bring
      </CardTitle>
      <!-- The count says what it counts. "Tokens to bring 3" would read as "bring three",
        which is precisely the number this panel refuses to state: nothing upstream separates
        "create a Treasure" from "create X Treasures". -->
      <p class="text-muted-foreground text-xs" aria-live="polite">
        <template v-if="updating"><UpdatingCue label="Rechecking…" /></template>
        <template v-else-if="tokens.length">
          This deck's cards make {{ tokens.length }} different token{{
            tokens.length === 1 ? '' : 's'
          }}.
        </template>
        <template v-else>What this deck's cards make, besides the cards themselves.</template>
      </p>
    </CardHeader>

    <CardContent class="space-y-3">
      <!-- Six frames rather than a spinner: this panel ends the page, so a skeleton the size
        of the answer keeps the footer from jumping up and back down as it lands. -->
      <div v-if="pending" class="grid gap-3" :class="DECK_CARD_SIZE_GRID_CLASS[cardSize.size]">
        <Skeleton v-for="n in 6" :key="n" class="aspect-[61/85] w-full rounded-lg" />
      </div>

      <p v-else-if="tokensQuery.isError.value" class="text-muted-foreground text-sm">
        Couldn't work out which tokens this deck makes.
      </p>

      <template v-else-if="tokens.length">
        <ul class="grid gap-3" :class="DECK_CARD_SIZE_GRID_CLASS[cardSize.size]">
          <li v-for="token in tokens" :key="token.key">
            <!-- A token with a printing in the catalog is a card like any other: same tile,
              same artwork, same detail modal. -->
            <CardTile v-if="token.card" :game="game" :card="token.card">
              <template #badge>
                <span
                  class="bg-background/90 text-foreground absolute top-1.5 right-1.5 z-20 inline-flex cursor-default items-center rounded-md border px-1.5 py-0.5 text-xs tabular-nums shadow select-none"
                  :title="sourceCountTitle(token.source_count)"
                >
                  ×{{ token.source_count }}
                </span>
              </template>
            </CardTile>

            <!-- No printing of it is in the catalog (a digital-only token, or a token set
              that hasn't been imported). The name and type line came with the reference, so
              the player still knows what to bring — there just isn't a card to link to.
              Laid out as `CardTile` lays a card out (frame, name, muted subline, the same
              corner badge) so it reads as one grid rather than two. -->
            <div v-else>
              <div class="relative">
                <div
                  class="bg-muted/40 text-muted-foreground flex aspect-[61/85] w-full flex-col items-center justify-center rounded-lg border border-dashed px-2 text-center"
                >
                  <Sparkles class="size-5 opacity-60" aria-hidden="true" />
                  <span class="mt-1 text-xs leading-tight">No image</span>
                </div>
                <span
                  class="bg-background/90 text-foreground absolute top-1.5 right-1.5 z-20 inline-flex cursor-default items-center rounded-md border px-1.5 py-0.5 text-xs tabular-nums shadow select-none"
                  :title="sourceCountTitle(token.source_count)"
                >
                  ×{{ token.source_count }}
                </span>
              </div>
              <div class="mt-1.5 px-0.5">
                <p class="truncate text-sm font-medium" :title="token.name">{{ token.name }}</p>
                <p class="text-muted-foreground truncate text-xs" :title="token.type_line ?? ''">
                  {{ token.type_line ?? 'Token' }}
                </p>
              </div>
            </div>
          </li>
        </ul>

        <!-- The audit trail: which card sent you looking for each token. Behind a disclosure
          because it's a second copy of the list above, and the grid answers the question most
          visits came with. -->
        <div class="flex items-center justify-between gap-3 pt-1">
          <p v-if="uncheckedCount > 0" class="text-muted-foreground min-w-0 text-xs">
            {{ uncheckedCount }} card{{ uncheckedCount === 1 ? '' : 's' }} in this deck
            {{ uncheckedCount === 1 ? "hasn't" : "haven't" }} been checked for tokens yet — card
            data is still syncing, so this list may be short.
          </p>
          <span v-else />
          <button
            type="button"
            class="text-muted-foreground hover:text-foreground flex shrink-0 items-center gap-1 text-xs font-medium"
            :aria-expanded="expanded"
            :aria-controls="expanded ? detailsId : undefined"
            @click="expanded = !expanded"
          >
            What makes them
            <ChevronDown
              class="size-3.5 transition-transform"
              :class="expanded ? 'rotate-180' : ''"
              aria-hidden="true"
            />
          </button>
        </div>

        <div v-if="expanded" :id="detailsId" class="grid gap-3 border-t pt-4 sm:grid-cols-2">
          <section v-for="token in tokens" :key="token.key" class="rounded-md border p-3">
            <div class="flex items-baseline justify-between gap-2">
              <h3 class="min-w-0 truncate text-sm font-medium">{{ token.name }}</h3>
              <span class="text-muted-foreground shrink-0 text-xs tabular-nums"
                >{{ token.source_count }} card{{ token.source_count === 1 ? '' : 's' }}</span
              >
            </div>
            <p v-if="token.type_line" class="text-muted-foreground mt-0.5 truncate text-xs">
              {{ token.type_line }}
            </p>
            <ul class="mt-2 flex flex-wrap gap-1.5">
              <li v-for="source in token.sources" :key="source.card_id">
                <a
                  :href="hrefFor('card', game, source.card_id)"
                  class="inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-xs hover:underline"
                  @click="onActivate($event, 'card', game, source.card_id)"
                  @pointerenter="warm('card')"
                  @focusin="warm('card')"
                >
                  {{ source.name }}
                  <span v-if="source.quantity > 1" class="text-muted-foreground tabular-nums"
                    >×{{ source.quantity }}</span
                  >
                </a>
              </li>
            </ul>
            <p
              v-if="token.source_count > token.sources.length"
              class="text-muted-foreground mt-1.5 text-xs"
            >
              …and {{ token.source_count - token.sources.length }} more
            </p>
          </section>
        </div>
      </template>

      <!-- Nothing to bring is an answer, and a short one. It still says which of the two
        "nothing"s it is. -->
      <p v-else class="text-muted-foreground text-sm">
        <template v-if="uncheckedCount > 0">
          None of the cards checked so far make tokens, and {{ uncheckedCount }} of them
          {{ uncheckedCount === 1 ? "hasn't" : "haven't" }} been checked yet — card data is still
          syncing.
        </template>
        <template v-else>None of this deck's cards make tokens.</template>
      </p>
    </CardContent>
  </Card>
</template>
