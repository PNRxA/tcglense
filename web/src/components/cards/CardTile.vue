<script lang="ts">
// Warm the shared card-detail dialog chunk on the first hover/focus of ANY tile (module
// flag → once per session), so the click that opens ?card= finds the chunk already
// fetched. import() itself dedupes, but the flag skips even the repeat call.
let dialogWarmed = false
</script>

<script setup lang="ts">
import { computed } from 'vue'
import { useRoute, useRouter, type LocationQueryRaw } from 'vue-router'
import type { Card } from '@/lib/api'
import { displayUsdPrice } from '@/lib/cardPrice'
import CardImage from '@/components/cards/CardImage.vue'
import { loadCardDetailDialog } from '@/components/cards/detailDialogLoader'
import { useGhostDisplayStore } from '@/stores/ghostDisplay'

const props = defineProps<{
  game: string
  card: Card
  // "Ghost" a card the viewer doesn't own (the collection view's show-ghosts mode, issue
  // #112): the image + text are dimmed and desaturated so owned cards stand out and the
  // gaps in a set read at a glance. Hovering brings the ghost back to full colour (it's
  // still a live link, and its quick-add "+" — a crisp, un-dimmed sibling — stays usable).
  ghost?: boolean
}>()

// Show the regular USD price, falling back to the foil price for foil-only cards.
const price = computed(() => displayUsdPrice(props.card.prices))
const to = computed(() => `/cards/${props.game}/cards/${props.card.id}`)

// A plain left-click opens the card in the detail modal over the current page — the
// URL gains `?card=<id>` (see CardDetailDialog in App.vue), so the list underneath
// keeps its scroll/search/page state and the browser's Back closes the modal. The
// tile is a hand-rendered <a> (not a RouterLink, whose own click handler would race
// this one) whose href stays the real card page, so modifier/middle clicks, "open in
// new tab", and crawlers still get the full page.
const route = useRoute()
const router = useRouter()
const href = computed(() => router.resolve(to.value).href)
function onClick(event: MouseEvent) {
  if (event.defaultPrevented) return
  // Let the browser handle anything that isn't a plain left-click.
  if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
    return
  }
  event.preventDefault()
  // A grid under a route without a `:game` path param (the public deck page,
  // `/u/:handle/decks/:id`) can't feed the shared CardDetailDialog its game from the URL
  // path, so carry it in the query there. Pages that already have `:game` in the path are
  // left untouched (the dialog reads the param and this stays absent).
  const query: LocationQueryRaw = { ...route.query, card: props.card.id }
  if (typeof route.params.game !== 'string' || !route.params.game) query.game = props.game
  void router.push({ query })
}

// Fire-and-forget prefetch of the detail-dialog chunk on first hover/focus.
function warmCardDetailDialog() {
  if (dialogWarmed) return
  dialogWarmed = true
  void loadCardDetailDialog()
}

// The image dims + (optionally) desaturates; the text block dims with opacity only.
// Crucially any grayscale goes on the IMAGE, never on the RouterLink below: a `filter` other
// than `none` makes an element the containing block for its abs-positioned descendants, which
// would collapse the link's stretched `after:inset-0` overlay onto just the text box and stop
// a click on the artwork from navigating. Opacity creates no containing block, so the text
// link can dim safely (and the #badge slot is never dimmed — its "+" stays crisp).
//
// The desaturation is a display preference (issue #213): 'grayscale' (default) drains the
// colour and restores it on hover; 'color' keeps the artwork's colour and only dims. Both
// modes always dim, so owned cards still stand out against the ghosts either way.
const ghostDisplay = useGhostDisplayStore()
const ghostImageClass = computed(() => {
  if (!props.ghost) return ''
  const base = 'opacity-45 transition group-hover:opacity-100 motion-reduce:transition-none'
  return ghostDisplay.style === 'grayscale' ? `${base} grayscale group-hover:grayscale-0` : base
})
const ghostTextClass = computed(() =>
  props.ghost ? 'opacity-60 transition group-hover:opacity-100 motion-reduce:transition-none' : '',
)
</script>

<template>
  <!-- Stretched-link card: a single RouterLink (the text block) whose `after:` overlay
    covers the whole tile, so the entire card is clickable and there's exactly one link /
    tab stop whose accessible name is the card text. Crucially the #badge overlay is a
    SIBLING of that link — not nested inside the <a> — so an interactive control there
    (the quick-add popover trigger) is valid HTML and its clicks don't navigate. -->
  <div class="group relative" @pointerenter="warmCardDetailDialog" @focusin="warmCardDetailDialog">
    <!-- On hover the card lifts: it scales up slightly and the resting shadow deepens.
      `group-hover:z-10` raises the (already `relative`) frame above its grid neighbours so
      the enlarged card and its shadow aren't clipped by later siblings painting on top.
      The light-mode `shadow-md` is invisible on dark's near-black background, so dark mode
      gets a larger, higher-opacity shadow instead. Reduced-motion users get neither the
      grow nor the transition. -->
    <div class="relative">
      <CardImage
        :game="game"
        :id="card.id"
        :name="card.name"
        :has-image="card.has_image"
        size="normal"
        class="transition duration-200 ease-out group-hover:z-10 group-hover:scale-[1.03] group-hover:shadow-md dark:group-hover:shadow-[0_8px_24px_rgba(0,0,0,0.85)] motion-reduce:transition-none motion-reduce:group-hover:scale-100"
        :class="ghostImageClass"
      />
      <!-- The image lifts to `group-hover:z-10` on hover, so overlay content must carry a
        higher z-index (the badge/quick-add control uses z-20) or the enlarged card paints
        over it. It sits above the stretched-link `after:` (z-10) too, so its buttons take
        the click instead of navigating. z-20 stays *below* the sticky search/filter bars
        (z-30), so a scrolled-under badge can't paint over that persistent chrome (issue
        #120). Browse views pass no slot, so nothing renders. -->
      <slot name="badge" />
    </div>
    <a
      :href="href"
      class="mt-1.5 block px-0.5 after:absolute after:inset-0 after:z-10 after:content-['']"
      :class="ghostTextClass"
      @click="onClick"
    >
      <p class="truncate text-sm font-medium group-hover:underline" :title="card.name">
        {{ card.name }}
      </p>
      <p class="text-muted-foreground flex items-center justify-between gap-2 text-xs">
        <span class="truncate"
          >{{ card.set_code.toUpperCase() }} · #{{ card.collector_number }}</span
        >
        <span v-if="price" class="shrink-0 tabular-nums"
          >${{ price.amount
          }}<span
            v-if="price.foil"
            class="ml-1 text-[0.65rem] tracking-wide uppercase opacity-70"
            title="Foil price (no regular printing)"
            >foil</span
          ></span
        >
      </p>
    </a>
  </div>
</template>
