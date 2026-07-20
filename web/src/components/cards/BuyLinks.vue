<script setup lang="ts">
import { ExternalLink } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import type { BuyLinkSection } from '@/lib/buyLinks'

// Presentational "Where to buy" card: outbound store search links grouped by
// region (issue #175). Shared by the card detail page (CardBuyLinks) and the
// sealed-product page (ProductBuyLinks) — the builder that produces `sections`
// differs, the rendering doesn't. Renders nothing for an empty section list.
const props = defineProps<{ sections: BuyLinkSection[] }>()
</script>

<template>
  <Card v-if="props.sections.length" class="gap-4 py-4">
    <CardHeader>
      <CardTitle class="text-sm font-semibold">Where to buy</CardTitle>
    </CardHeader>
    <CardContent class="space-y-4">
      <section v-for="section in props.sections" :key="section.title">
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
    </CardContent>
  </Card>
</template>
