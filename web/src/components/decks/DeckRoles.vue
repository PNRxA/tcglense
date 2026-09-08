<script setup lang="ts">
import { computed, ref, useId } from 'vue'
import { ChevronDown } from '@lucide/vue'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import StaleNotice from '@/components/cards/StaleNotice.vue'
import UpdatingCue from '@/components/cards/UpdatingCue.vue'
import DeckStatBars from '@/components/decks/DeckStatBars.vue'
import { useDetailModalLink } from '@/composables/useDetailModalLink'
import type { DeckRole, DeckRoleGroup, DeckRoles, DeckStatItem } from '@/lib/api'

// **Card roles** (issue #671): how much of the deck ramps, draws, removes, wipes, counters,
// tutors, recurs and protects — the eight questions a deckbuilder asks about a list they
// can't remember card by card.
//
// It rests **compact**, in `DeckBracket`'s shape: one row of chips, one per role with its
// count — and the chips are the filter toggles, so the resting panel is the whole control.
// Behind "Details" sit the bars (the same eight numbers, drawn to scale), what each role
// counts, and every card it counted. Two panels above this one already rest collapsed on the
// same page, and a third full-height card of bars on a page whose subject is the card list
// was the thing this shape replaces.
//
// Every claim is the server's (`GET /api/decks/{game}/{deck_id}/roles`, or its public and
// precon mirrors), including each role's label and its description, so a CLI asking the same
// question gets the same eight buckets. This component does not fetch: the view owns the
// query, because the same response feeds two things — these bars, and the card list's role
// filter below them (`useDeckCardDisplay`'s `filterRole`). Fetching it here would either
// duplicate the read or leave the two able to disagree.
//
// Two honesty constraints, both inherited from the response:
//
// * **The roles are not a partition of the deck.** Most creatures and every land fill none,
//   and a card can fill several, so the bars neither sum to the deck nor to each other. The
//   line under them says how many cards fill *at least one* role rather than leaving the
//   reader to add the bars up and wonder where the rest went.
// * **The bars count copies, the details count names.** `copies` is what a 60-card builder
//   means by "ten pieces of ramp" (`DeckStatBars`' own label spells the word out), while the
//   detail list is per card name — so a capped list says how many names it left out, never
//   how many copies.
const props = defineProps<{
  /** Only for the detail list's card links — the panel is handed its data, it never fetches. */
  game: string
  roles?: DeckRoles
  /** The first fetch is in flight — nothing to draw yet. */
  pending: boolean
  /** The read never loaded (`isLoadingError`, issue #622) — there are no bars to keep. */
  failed: boolean
  /** A background refetch failed while good bars are on screen (`isRefetchError`): keep
   *  them, say so, and leave the filter they drive untouched. */
  stale?: boolean
}>()

/** The selected role, or null. The resting chips and the expanded bars (`DeckStatBars`'
 * `selectable` mode) are both bound to it — two views of one control, never two controls —
 * so the panel owns no toggle of its own. */
const role = defineModel<DeckRole | null>('role', { default: null })

/** "Ramp: 25 copies" — the chip's tooltip, in the same words the bar's label uses. */
function chipTitle(group: DeckRoleGroup): string {
  return `${group.label}: ${group.copies} ${group.copies === 1 ? 'copy' : 'copies'}`
}

function toggle(next: DeckRole) {
  role.value = role.value === next ? null : next
}

// Every role, zeroes included — a deck with no board wipes says so, and a bar that vanished
// when it hit zero would be read as "not counted" rather than "none".
const items = computed<DeckStatItem[]>(() =>
  (props.roles?.roles ?? []).map((group) => ({
    key: group.role,
    label: group.label,
    count: group.copies,
    color: null,
  })),
)

/** Distinct names filling at least one role, out of the deck proper's distinct names. */
const classifiedCount = computed(() =>
  props.roles ? props.roles.card_count - props.roles.unclassified_count : 0,
)

// A deck with no cards in it has no roles to report, so the panel isn't there at all.
const hasDeck = computed(() => (props.roles?.card_count ?? 0) > 0)

const expanded = ref(false)
const detailsId = useId()

const { hrefFor, onActivate, warm } = useDetailModalLink()
</script>

<template>
  <!-- The header is shared across all three bodies, exactly as `DeckStats` does it: the bars
    land in space the card already occupies instead of shoving the deck down the page. -->
  <Card v-if="pending || failed || hasDeck" class="mb-6" :aria-busy="pending || undefined">
    <CardHeader class="flex flex-row items-center justify-between gap-3 space-y-0">
      <CardTitle class="text-base">Card roles</CardTitle>
      <div class="flex shrink-0 items-center gap-3">
        <span class="text-muted-foreground text-xs" aria-live="polite">
          <UpdatingCue v-if="pending" label="Reading the deck…" />
        </span>
        <!-- `aria-label` because this page carries two other buttons reading exactly
          "Details" (the bracket's and the analytics panel's). -->
        <button
          v-if="!failed"
          type="button"
          class="text-muted-foreground hover:text-foreground focus-visible:ring-ring/50 flex shrink-0 items-center gap-1 rounded-sm text-xs font-medium outline-none focus-visible:ring-3 disabled:opacity-50"
          aria-label="Details for card roles"
          :aria-expanded="expanded"
          :aria-controls="expanded ? detailsId : undefined"
          :disabled="pending"
          @click="expanded = !expanded"
        >
          Details
          <ChevronDown
            class="size-3.5 transition-transform"
            :class="expanded ? 'rotate-180' : ''"
            aria-hidden="true"
          />
        </button>
      </div>
    </CardHeader>

    <CardContent v-if="failed">
      <p class="text-destructive text-sm">These roles couldn't be worked out. Please retry.</p>
    </CardContent>

    <CardContent v-else-if="pending" class="space-y-2">
      <Skeleton class="h-3 w-full max-w-lg" />
      <Skeleton class="h-24 w-full" />
    </CardContent>

    <CardContent v-else-if="roles" class="space-y-3">
      <StaleNotice v-if="stale" label="Couldn't refresh — showing the roles as they last loaded." />

      <!-- The resting row: every role as a chip, zeroes included (an absent "Board wipes"
        would read as "not counted"). Each chip is the toggle for its role — pressed when the
        list below is filtered to it, and the same chip clears it. -->
      <ul class="flex flex-wrap gap-1.5" aria-label="Filter the card list by role">
        <li v-for="group in roles.roles" :key="group.role">
          <button
            type="button"
            class="focus-visible:ring-ring/50 inline-flex items-center gap-1.5 rounded-md border px-2 py-0.5 text-xs outline-none focus-visible:ring-3"
            :class="[
              role === group.role
                ? 'bg-muted ring-ring/50 ring-2'
                : 'hover:bg-muted/40 border-transparent',
              group.copies === 0 ? 'text-muted-foreground' : '',
            ]"
            :aria-pressed="role === group.role"
            :title="chipTitle(group)"
            @click="toggle(group.role)"
          >
            {{ group.label }}
            <span
              class="inline-flex items-center rounded px-1 font-semibold tabular-nums"
              :class="group.copies === 0 ? '' : 'bg-muted'"
              >{{ group.copies }}</span
            >
          </button>
        </li>
      </ul>

      <p class="text-muted-foreground text-xs">
        <span class="tabular-nums">{{ classifiedCount }}</span> of
        <span class="tabular-nums">{{ roles.card_count }}</span> distinct cards fill at least one
        role — a card can fill several roles. Click a role to filter the list.
      </p>

      <!-- The evidence: the same eight numbers drawn to scale, what each role counts (the
        only place the near-miss each role deliberately excludes is spelled out), and the
        cards it counted — the panel's claim to being checkable. -->
      <div v-if="expanded" :id="detailsId" class="space-y-4 border-t pt-4">
        <p class="text-muted-foreground text-xs">
          Read off each card's rules text. A card the grammar isn't sure about is left out rather
          than guessed, so a bar is a floor.
        </p>

        <DeckStatBars
          v-model:selected="role"
          title="Copies by role"
          layout="rows"
          selectable
          :items="items"
        />

        <div class="grid gap-3 sm:grid-cols-2">
          <section v-for="group in roles.roles" :key="group.role" class="rounded-md border p-3">
            <div class="flex items-center justify-between gap-2">
              <h3 class="text-sm font-medium">{{ group.label }}</h3>
              <span
                class="inline-flex shrink-0 items-center rounded-md px-1.5 py-0.5 text-xs font-semibold tabular-nums"
                :class="group.count === 0 ? 'text-muted-foreground' : 'bg-muted'"
                :title="`${group.count} card${group.count === 1 ? '' : 's'}, ${group.copies} ${
                  group.copies === 1 ? 'copy' : 'copies'
                }`"
                >{{ group.count }}</span
              >
            </div>
            <p class="text-muted-foreground mt-1 text-xs">{{ group.description }}</p>
            <ul v-if="group.cards.length" class="mt-2 flex flex-wrap gap-1.5">
              <li v-for="card in group.cards" :key="card.card_id">
                <a
                  :href="hrefFor('card', game, card.card_id)"
                  class="inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-xs hover:underline"
                  @click="onActivate($event, 'card', game, card.card_id)"
                  @pointerenter="warm('card')"
                  @focusin="warm('card')"
                >
                  {{ card.name }}
                  <span v-if="card.quantity > 1" class="text-muted-foreground tabular-nums"
                    >×{{ card.quantity }}</span
                  >
                </a>
              </li>
            </ul>
            <p v-if="group.count > group.cards.length" class="text-muted-foreground mt-1.5 text-xs">
              …and {{ group.count - group.cards.length }} more
            </p>
          </section>
        </div>
      </div>
    </CardContent>
  </Card>
</template>
