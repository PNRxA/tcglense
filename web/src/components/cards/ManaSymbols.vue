<script setup lang="ts">
import { computed } from 'vue'
import KeywordTooltip from '@/components/cards/KeywordTooltip.vue'
import { useKeywordGlossary } from '@/composables/useKeywords'
import { splitKeywords, type KeywordSegmentToken } from '@/lib/keywords'
import { parseManaText, type ManaToken } from '@/lib/mana'

// Renders card text with its `{…}` mana/cost symbols shown as mana-font icons and
// the surrounding words left as plain text. The root is an inline <span>, so it
// drops into a mana-cost line, a colour-identity row, or a block of oracle text
// (inheriting `whitespace-pre-line`/`leading-*` from the parent) unchanged.
//
// With `keywords`, the plain-text runs are additionally scanned for rules keywords and
// each first mention becomes a hover/tap explanation (`KeywordTooltip`). That is opt-in
// per call site because most of them aren't prose: a mana cost or a colour identity has
// no words to explain, and the deck list renders one of these per row, where neither the
// glossary fetch nor the matching would earn its keep. Keeping it a prop on this
// component rather than a wrapper is deliberate — keywords live *inside* the text runs
// between the symbols, so a wrapper could only re-implement this same loop.
const props = defineProps<{
  text: string
  /** Explain rules keywords found in the text (oracle text and rulings — not costs). */
  keywords?: boolean
  /** The card's name, when known, so its own title is never mistaken for a keyword
   * ("Fear of Isolation" is not the keyword Fear). */
  cardName?: string
}>()

// Only a `keywords` call site subscribes to the glossary. The prop is a static literal
// at every call site — a mana cost never turns into prose — so reading it once here is
// safe, and it buys two things a reactive `enabled` couldn't: the ~100 mana-cost rows in
// a deck list create no query observers at all, and this component still mounts without
// a QueryClient (vue-query needs one even for a disabled query).
//
// The glossary resolves after first paint, so keywords render as plain text until then —
// a marker appearing is a smaller jolt than the text reflowing.
const glossary = props.keywords ? useKeywordGlossary() : undefined
const entries = computed(() => glossary?.entries.value ?? [])

/** The symbol/text split, with each text run further split on keywords when asked. A
 * flat token list keeps the template a single loop, which matters here: every join
 * between tokens has to stay whitespace-free (see the template). */
type Token = Exclude<ManaToken, { type: 'text' }> | KeywordSegmentToken

const tokens = computed<Token[]>(() =>
  parseManaText(props.text).flatMap((token): Token[] => {
    if (token.type !== 'text') return [token]
    if (!props.keywords || entries.value.length === 0) return [token]
    return splitKeywords(token.value, entries.value, props.cardName)
  }),
)
</script>

<template>
  <span
    ><template v-for="(token, index) in tokens" :key="index"
      ><i
        v-if="token.type === 'symbol'"
        :class="['ms', token.className, 'ms-cost']"
        role="img"
        :aria-label="token.label"
        :title="token.label"
      /><KeywordTooltip
        v-else-if="token.type === 'keyword'"
        :entry="token.entry"
        :label="token.value"
      /><template v-else>{{ token.value }}</template></template
    ></span
  >
</template>

<style scoped>
/* A hair of space between adjacent pips (e.g. {2}{W}{U}) and before following text,
 * matching how Scryfall renders costs; scales with the surrounding font size. */
.ms {
  margin-right: 0.08em;
}
</style>
