<script setup lang="ts">
import DetailDialogShell from '@/components/shared/DetailDialogShell.vue'
import CardDetailContent from '@/components/cards/CardDetailContent.vue'
import { useCardNavStore } from '@/stores/cardNav'

// The card-detail modal (issue #275): the shared shell, opened on `?card=<id>`, wrapped around
// the card body. The frame — URL-driven open/close, prev/next through the grid underneath, the
// arrow keys, the escape hatches — is DetailDialogShell's; this names the card surface's half.
const nav = useCardNavStore()

// The canonical, crawlable card page the "Open full page" escape hatch links to.
const canonical = (game: string, id: string) => `/cards/${game}/cards/${id}`
</script>

<template>
  <DetailDialogShell query-key="card" noun="card" :canonical="canonical" :nav="nav">
    <template #default="{ game, id }">
      <CardDetailContent :game="game" :id="id" />
    </template>
  </DetailDialogShell>
</template>
