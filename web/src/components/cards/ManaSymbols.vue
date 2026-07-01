<script setup lang="ts">
import { computed } from 'vue'
import { parseManaText } from '@/lib/mana'

// Renders card text with its `{…}` mana/cost symbols shown as mana-font icons and
// the surrounding words left as plain text. The root is an inline <span>, so it
// drops into a mana-cost line, a colour-identity row, or a block of oracle text
// (inheriting `whitespace-pre-line`/`leading-*` from the parent) unchanged.
const props = defineProps<{ text: string }>()

const tokens = computed(() => parseManaText(props.text))
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
