<script setup lang="ts">
import { computed, ref, toRef, useId } from 'vue'
import { ChevronDown, ExternalLink, InfinityIcon } from '@lucide/vue'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import ManaSymbols from '@/components/cards/ManaSymbols.vue'
import StaleNotice from '@/components/cards/StaleNotice.vue'
import UpdatingCue from '@/components/cards/UpdatingCue.vue'
import {
  useDeckCombosQuery,
  usePreconCombosQuery,
  usePublicDeckCombosQuery,
} from '@/composables/useDeckAnalysis'
import { useDetailModalLink } from '@/composables/useDetailModalLink'
import type { DeckCombo, DeckComboMissing, DeckComboPiece } from '@/lib/api'

// **Combos** (issue #683): which of Commander Spellbook's combos this deck can assemble,
// and which it is one card short of — the two questions a deck page is asked about a pile
// of cards that a card list can't answer on its own.
//
// Every claim is the server's (`GET /api/decks/{game}/{deck_id}/combos`, or its public and
// precon mirrors), and every claim is *curated data*, never inferred from rules text: what
// several cards do together is a fact that only a combo database holds, which is why this
// panel names its source and links every combo back to it.
//
// Three silences the panel inherits from the response and must keep:
//
// * **No data is not "no combos".** `available: false` means the dataset hasn't been synced
//   (a fresh self-host, or an instance that opted out); an empty list then says nothing at
//   all about this deck, exactly as `DeckTokens` refuses to read an unchecked card as
//   "makes none".
// * **A template is not a card you have.** "A free sacrifice outlet" is a wildcard this app
//   can't evaluate, so it is worded as something the combo *also needs* — never as a
//   requirement already met.
// * **No invented totals.** `combo_count` / `almost_count` are the exact numbers; the lists
//   are capped, and a capped list says how many it left out rather than implying it is whole.
//
// `handle` / `preconSlug` pick the surface, chosen ONCE at mount exactly as `DeckTokens` and
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
const combosQuery = props.preconSlug
  ? usePreconCombosQuery(game, preconSlug)
  : props.handle
    ? usePublicDeckCombosQuery(handle, deckId)
    : useDeckCombosQuery(game, deckId)

const combos = computed(() => combosQuery.data.value?.combos ?? [])
const almost = computed(() => combosQuery.data.value?.almost ?? [])
const comboCount = computed(() => combosQuery.data.value?.combo_count ?? 0)
const almostCount = computed(() => combosQuery.data.value?.almost_count ?? 0)
/** `false` until a response says otherwise — "we don't know" is the honest default while
 *  nothing has loaded, and it is what the empty state is worded from. */
const available = computed(() => combosQuery.data.value?.available ?? false)
const source = computed(() => combosQuery.data.value?.source ?? 'Commander Spellbook')
const sourceUrl = computed(
  () => combosQuery.data.value?.source_url ?? 'https://commanderspellbook.com',
)

const pending = computed(() => combosQuery.isPending.value)
const updating = computed(() => combosQuery.isFetching.value && !pending.value)
const anything = computed(() => combos.value.length > 0 || almost.value.length > 0)

/** The two lists, as one loop: they render identically apart from their heading and the
 *  "Needs:" line only an incomplete combo has. An empty one is dropped rather than headed,
 *  so a deck that assembles nothing shows "One card short" alone. */
const sections = computed(() =>
  [
    {
      key: 'complete',
      title: 'In this deck',
      entries: combos.value,
      total: comboCount.value,
      needs: false,
    },
    {
      key: 'almost',
      title: 'One card short',
      entries: almost.value,
      total: almostCount.value,
      needs: true,
    },
  ].filter((section) => section.entries.length > 0),
)

const plural = (n: number) => (n === 1 ? '' : 's')

/** The header's one-line answer, counted from the exact totals rather than the capped
 *  lists — a deck of staples is one card from thousands of combos. */
const subline = computed(() => {
  if (!available.value) return 'Card interactions, from a curated combo database.'
  const can = comboCount.value
  const short = almostCount.value
  if (can > 0 && short > 0) {
    return `This deck can assemble ${can} combo${plural(can)} and is one card short of ${short} more.`
  }
  if (can > 0) return `This deck can assemble ${can} combo${plural(can)}.`
  if (short > 0) return `This deck is one card short of ${short} combo${plural(short)}.`
  return 'Card interactions, from a curated combo database.'
})

/** A piece's chip: muted and dashed when the deck doesn't hold it (the point of the
 *  "one card short" list), and only underlined on hover when there is a card to open. */
function pieceClass(piece: DeckComboPiece): string {
  const held = piece.in_deck ? '' : 'text-muted-foreground border-dashed'
  return piece.card_id ? `${held} hover:underline` : held
}

/** What a combo still needs, split by whether the app could check it. A template is a
 *  Scryfall query this app can't run ("any free sacrifice outlet"), so it is listed apart
 *  and worded as something the combo *also* needs — never as a box already ticked. */
const cardMisses = (combo: DeckCombo): DeckComboMissing[] =>
  combo.missing.filter((m) => m.kind !== 'template')
const templateMisses = (combo: DeckCombo): DeckComboMissing[] =>
  combo.missing.filter((m) => m.kind === 'template')

/** One combo's steps, open on demand: the description is several lines of rules work, and
 *  a page of them unfolded is a wall rather than a list of combos. */
const expanded = ref(new Set<string>())
const detailsId = useId()
const isExpanded = (id: string) => expanded.value.has(id)
function toggle(id: string) {
  const next = new Set(expanded.value)
  if (!next.delete(id)) next.add(id)
  expanded.value = next
}

const { hrefFor, onActivate, warm } = useDetailModalLink()
</script>

<template>
  <Card class="mt-8 gap-3 py-4" :aria-busy="updating || undefined">
    <CardHeader class="pb-0">
      <CardTitle class="flex items-center gap-2 text-base">
        <InfinityIcon class="size-4" aria-hidden="true" /> Combos
      </CardTitle>
      <p class="text-muted-foreground text-xs" aria-live="polite">
        <template v-if="updating"><UpdatingCue label="Rechecking…" /></template>
        <template v-else>{{ subline }}</template>
      </p>
    </CardHeader>

    <CardContent class="space-y-4">
      <div v-if="pending" class="space-y-2">
        <Skeleton v-for="n in 3" :key="n" class="h-16 w-full rounded-lg" />
      </div>

      <!-- `isLoadingError`, not `isError` (issue #622): query-core flips `status` to 'error'
        on ANY failed fetch while keeping `data`, so a hiccuped background refetch would
        otherwise swap a good list for "couldn't work out". A failed *refetch* keeps the
        lists and says so above them instead. -->
      <p v-else-if="combosQuery.isLoadingError.value" class="text-muted-foreground text-sm">
        Couldn't work out which combos are in this deck.
      </p>

      <template v-else-if="anything">
        <StaleNotice
          v-if="combosQuery.isRefetchError.value"
          label="Couldn't refresh — showing the combos as they last loaded."
        />

        <section v-for="section in sections" :key="section.key">
          <h3 class="text-muted-foreground mb-2 text-xs font-semibold tracking-wide uppercase">
            {{ section.title }}
          </h3>
          <ul class="space-y-3">
            <li v-for="combo in section.entries" :key="combo.id" class="rounded-lg border p-3">
              <!-- The pieces, in the combo's own order. A piece the deck doesn't hold is
                muted rather than hidden: it is the point of the "one card short" list. -->
              <ul class="flex flex-wrap items-center gap-1.5">
                <li v-for="piece in combo.pieces" :key="piece.oracle_id">
                  <component
                    :is="piece.card_id ? 'a' : 'span'"
                    :href="piece.card_id ? hrefFor('card', game, piece.card_id) : undefined"
                    class="inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-xs"
                    :class="pieceClass(piece)"
                    @click="
                      piece.card_id ? onActivate($event, 'card', game, piece.card_id) : undefined
                    "
                    @pointerenter="warm('card')"
                    @focusin="warm('card')"
                  >
                    {{ piece.name }}
                    <span v-if="piece.quantity > 1" class="text-muted-foreground tabular-nums"
                      >×{{ piece.quantity }}</span
                    >
                    <span
                      v-if="piece.must_be_commander"
                      class="bg-info/15 text-info rounded px-1 text-[0.625rem] leading-4"
                      >commander</span
                    >
                  </component>
                </li>
              </ul>

              <!-- What it does. The one line most readers came for, so it sits directly
                under the pieces rather than behind the disclosure. -->
              <ul v-if="combo.produces.length" class="mt-2 flex flex-wrap gap-1.5">
                <li
                  v-for="result in combo.produces"
                  :key="result"
                  class="bg-success/15 text-success rounded-md px-1.5 py-0.5 text-xs"
                >
                  {{ result }}
                </li>
              </ul>

              <p v-if="combo.mana_needed" class="text-muted-foreground mt-2 text-xs">
                Needs mana: <ManaSymbols :text="combo.mana_needed" />
              </p>

              <!-- The builder's shopping list. Only on the "one card short" side, where the
                response says what is lacking. -->
              <template v-if="section.needs">
                <p v-if="cardMisses(combo).length" class="mt-2 text-xs">
                  <span class="text-muted-foreground">Needs:</span>
                  <template v-for="(miss, index) in cardMisses(combo)" :key="miss.name">
                    <span v-if="index > 0" class="text-muted-foreground">, </span>
                    <a
                      v-if="miss.card_id"
                      :href="hrefFor('card', game, miss.card_id)"
                      class="bg-warning/15 text-warning ml-1 rounded-md px-1.5 py-0.5 hover:underline"
                      @click="onActivate($event, 'card', game, miss.card_id)"
                      @pointerenter="warm('card')"
                      @focusin="warm('card')"
                      >{{ miss.name }}</a
                    >
                    <span v-else class="bg-warning/15 text-warning ml-1 rounded-md px-1.5 py-0.5">{{
                      miss.name
                    }}</span>
                    <span v-if="miss.kind === 'commander'" class="text-muted-foreground">
                      (in the command zone)</span
                    >
                  </template>
                </p>
                <!-- A template is a wildcard the app can't evaluate, so it is never reported
                  as met — it is stated as a further requirement, in the combo's own words. -->
                <p
                  v-for="miss in templateMisses(combo)"
                  :key="miss.name"
                  class="text-muted-foreground mt-1 text-xs"
                >
                  …also needs: {{ miss.name }} — a requirement we can't check for you.
                </p>
              </template>

              <div class="mt-2 flex flex-wrap items-center gap-3">
                <button
                  type="button"
                  class="text-muted-foreground hover:text-foreground flex items-center gap-1 text-xs font-medium"
                  :aria-expanded="isExpanded(combo.id)"
                  :aria-controls="isExpanded(combo.id) ? `${detailsId}-${combo.id}` : undefined"
                  @click="toggle(combo.id)"
                >
                  How it works
                  <ChevronDown
                    class="size-3.5 transition-transform"
                    :class="isExpanded(combo.id) ? 'rotate-180' : ''"
                    aria-hidden="true"
                  />
                </button>
                <a
                  :href="combo.url"
                  target="_blank"
                  rel="noopener noreferrer"
                  class="text-muted-foreground hover:text-foreground flex items-center gap-1 text-xs"
                >
                  View on {{ source }}
                  <ExternalLink class="size-3" aria-hidden="true" />
                </a>
              </div>

              <div
                v-if="isExpanded(combo.id)"
                :id="`${detailsId}-${combo.id}`"
                class="mt-2 space-y-2 border-t pt-2"
              >
                <p v-if="combo.prerequisites" class="text-muted-foreground text-xs">
                  <span class="font-medium">Before you start:</span>
                  <ManaSymbols :text="combo.prerequisites" />
                </p>
                <p class="text-xs leading-relaxed whitespace-pre-line">
                  <ManaSymbols :text="combo.description" />
                </p>
              </div>
            </li>
          </ul>

          <p
            v-if="section.total > section.entries.length"
            class="text-muted-foreground mt-2 text-xs"
          >
            …and {{ section.total - section.entries.length }} more
          </p>
        </section>
      </template>

      <!-- The two "nothing"s, kept apart. An unsynced dataset says nothing about this deck;
        only a synced one can report that a deck has no combos. -->
      <p v-else class="text-muted-foreground text-sm">
        <template v-if="!available">
          Combo data hasn't been synced yet, so nothing can be said about this deck's combos.
        </template>
        <template v-else>
          None of {{ source }}'s combos are in this deck, and none is one card away.
        </template>
      </p>

      <p class="text-muted-foreground border-t pt-3 text-xs">
        Combo data from
        <a
          :href="sourceUrl"
          target="_blank"
          rel="noopener noreferrer"
          class="underline underline-offset-2"
          >{{ source }}</a
        >.
      </p>
    </CardContent>
  </Card>
</template>
