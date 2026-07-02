<script setup lang="ts">
import { computed } from 'vue'
import { ExternalLink } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import type { Card as CardModel } from '@/lib/api'
import { buyLinksFor } from '@/lib/buyLinks'

// "Where to buy" — outbound card-name search links per store, grouped by region
// (issue #175). No per-store prices are shown (we don't ingest them); the
// buttons just land the user on each store's results for this card. Renders
// nothing for a game with no store registry.
const props = defineProps<{ game: string; card: CardModel }>()

const sections = computed(() => buyLinksFor(props.game, props.card))
</script>

<template>
  <Card v-if="sections.length" class="mt-6">
    <CardHeader>
      <CardTitle class="text-sm font-semibold">Where to buy</CardTitle>
    </CardHeader>
    <CardContent class="space-y-4">
      <section v-for="section in sections" :key="section.title">
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
