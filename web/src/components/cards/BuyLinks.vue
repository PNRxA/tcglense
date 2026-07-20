<script setup lang="ts">
import { computed, ref } from 'vue'
import { ChevronDown, ExternalLink } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import type { BuyLinkSection } from '@/lib/buyLinks'

// Presentational "Where to buy" card: outbound store search links grouped by
// region (issue #175). Shared by the card detail page (CardBuyLinks) and the
// sealed-product page (ProductBuyLinks) — the builder that produces `sections`
// differs, the rendering doesn't. Renders nothing for an empty section list.
//
// Collapsed by default to the registry's featured stores (TCGplayer, Card
// Kingdom, and MTG Mate for AU singles — see `featured` in lib/buyLinks.ts);
// "Show all stores" reveals the rest, including any region with no featured
// store (the sealed AU list). A registry with no featured stores at all has
// nothing sensible to collapse to, so it just shows everything.
const props = defineProps<{ sections: BuyLinkSection[] }>()

const expanded = ref(false)

const featuredSections = computed(() =>
  props.sections
    .map((section) => ({ ...section, links: section.links.filter((link) => link.featured) }))
    .filter((section) => section.links.length > 0),
)
const collapsible = computed(() => featuredSections.value.length > 0)
const visibleSections = computed(() =>
  expanded.value || !collapsible.value ? props.sections : featuredSections.value,
)

// How many stores the collapsed view hides — labels the toggle so it's an
// honest affordance rather than a mystery-meat "more".
const hiddenCount = computed(
  () =>
    props.sections.reduce((sum, section) => sum + section.links.length, 0) -
    featuredSections.value.reduce((sum, section) => sum + section.links.length, 0),
)
</script>

<template>
  <Card v-if="props.sections.length" class="gap-4 py-4">
    <CardHeader>
      <CardTitle class="text-sm font-semibold">Where to buy</CardTitle>
    </CardHeader>
    <CardContent class="space-y-4">
      <section v-for="section in visibleSections" :key="section.title">
        <h3 class="text-muted-foreground mb-2 text-xs font-medium tracking-wide uppercase">
          {{ section.title }}
        </h3>
        <div class="flex flex-wrap gap-2">
          <Button
            v-for="link in section.links"
            :key="link.name"
            as="a"
            :href="link.href"
            target="_blank"
            rel="noopener noreferrer"
            variant="outline"
            size="sm"
          >
            {{ link.name }}
            <ExternalLink class="text-muted-foreground size-3.5" aria-hidden="true" />
          </Button>
        </div>
      </section>

      <Button
        v-if="collapsible && hiddenCount > 0"
        type="button"
        variant="ghost"
        size="sm"
        class="text-muted-foreground hover:text-foreground -ml-2"
        :aria-expanded="expanded"
        @click="expanded = !expanded"
      >
        <ChevronDown
          class="size-3.5 transition-transform"
          :class="expanded ? 'rotate-180' : ''"
          aria-hidden="true"
        />
        {{ expanded ? 'Show fewer stores' : `Show all stores (${hiddenCount} more)` }}
      </Button>
    </CardContent>
  </Card>
</template>
