<script setup lang="ts">
import { computed } from 'vue'
import KeywordTooltip from '@/components/cards/KeywordTooltip.vue'
import { useKeywordGlossary } from '@/composables/useKeywords'
import { splitKeywords, type KeywordSegment, type KeywordSegmentToken } from '@/lib/keywords'
import { parseManaText, type ManaToken } from '@/lib/mana'

// Renders card text with its `{…}` mana/cost symbols shown as mana-font icons and
// the surrounding words left as plain text. The root is an inline <span>, so it
// drops into a mana-cost line, a colour-identity row, or a block of oracle text
// (inheriting `whitespace-pre-line`/`leading-*` from the parent) unchanged.
//
// With `keywords`, the text is additionally scanned for rules keywords and each first
// mention becomes a hover/tap explanation (`KeywordTooltip`). That is opt-in per call
// site because most of them aren't prose: a mana cost or a colour identity has no words
// to explain, and the deck list renders one of these per row, where neither the glossary
// fetch nor the matching would earn its keep. Keeping it a prop on this component rather
// than a wrapper is deliberate — keywords and mana symbols are interleaved in the same
// string, so a wrapper could only re-implement this same loop.
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

/** The flat token list the template loops over once — every join between tokens has to
 * stay whitespace-free (see the template), so one loop is what keeps that manageable. */
type Token = ManaToken | KeywordSegment

/**
 * Keywords are matched over the **whole** text first, and only the plain runs between
 * the matches are then split into mana symbols.
 *
 * The obvious order — symbols first, keywords per run — is subtly wrong, because
 * `parseManaText` cuts the string at every `{…}`. A reminder text holding a mana symbol
 * ("Unearth {B} ({B}: Return this card … It gains haste. …)") would arrive as fragments:
 * the fragment after the symbol has a `)` with no `(`, so the reminder-text guard finds
 * nothing and marks the `haste` inside it, and the first-mention rule restarts per
 * fragment, marking `Unearth` twice. Matching first keeps all four guards looking at the
 * real text. Nothing is lost this way round: a keyword name never contains braces, so a
 * keyword segment has no symbols left to find.
 */
const tokens = computed<Token[]>(() => {
  const segments: KeywordSegmentToken[] =
    props.keywords && entries.value.length > 0
      ? splitKeywords(props.text, entries.value, props.cardName)
      : [{ type: 'text', value: props.text }]
  return segments.flatMap((segment): Token[] =>
    segment.type === 'keyword' ? [segment] : parseManaText(segment.value),
  )
})
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
