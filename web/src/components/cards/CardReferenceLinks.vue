<script setup lang="ts">
import { computed } from 'vue'
import { ExternalLink } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import type { CardDetailOrTile } from '@/lib/api'
import { cardReferenceLinksFor } from '@/lib/buyLinks'

// "More about this card" — the pages that describe the card rather than sell it (issue
// #686): Gatherer by the printing's multiverse id, EDHREC by name. Sits under "Where to
// buy" in the rail. A reference whose id the printing lacks (a card Gatherer never listed)
// simply isn't offered — the row never shows a dead button — and a game with no
// references renders nothing.
const props = defineProps<{ game: string; card: CardDetailOrTile }>()

const links = computed(() => cardReferenceLinksFor(props.game, props.card))
</script>

<template>
  <Card v-if="links.length" class="gap-4 py-4">
    <CardHeader>
      <CardTitle class="text-sm font-semibold">More about this card</CardTitle>
    </CardHeader>
    <CardContent>
      <div class="flex flex-wrap gap-2">
        <Button
          v-for="link in links"
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
    </CardContent>
  </Card>
</template>
